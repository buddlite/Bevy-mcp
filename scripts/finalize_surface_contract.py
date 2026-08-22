from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:100]!r}")
    file.write_text(text.replace(old, new, 1))


# Capabilities are safe introspection even when all operational permissions are disabled.
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    "    match command {\n        McpCommand::EntitySpawn { .. }",
    "    match command {\n        McpCommand::Capabilities => true,\n        McpCommand::EntitySpawn { .. }",
)

# Runtime capture availability requires an actual renderer as well as a target.
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    '''    let primary_window_available = world
        .iter_entities()
        .any(|entity| entity.contains::<bevy::window::PrimaryWindow>());
    let camera_target_available = world
        .iter_entities()
        .any(|entity| entity.contains::<bevy::camera::RenderTarget>());
    let ui_capture_available = world
        .get_resource::<crate::agent_api::McpCaptureTargets>()
        .and_then(|targets| targets.ui_target())
        .is_some();
''',
    '''    let renderer_available = world
        .get_resource::<bevy::render::renderer::RenderDevice>()
        .is_some();
    let primary_window_available = renderer_available
        && world
            .iter_entities()
            .any(|entity| entity.contains::<bevy::window::PrimaryWindow>());
    let camera_target_available = renderer_available
        && world
            .iter_entities()
            .any(|entity| entity.contains::<bevy::camera::RenderTarget>());
    let ui_capture_available = renderer_available
        && world
            .get_resource::<crate::agent_api::McpCaptureTargets>()
            .and_then(|targets| targets.ui_target())
            .is_some();
''',
)
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    '''    McpResult::success(json!({
        "schema_version": 2,
        "permissions": {
''',
    '''    McpResult::success(json!({
        "schema_version": 2,
        "connected": true,
        "permissions": {
''',
)

# Preserve useful capability introspection while disconnected instead of returning NOT_CONNECTED.
replace_once(
    "crates/bevy-mcp-server/src/tools.rs",
    '''    async fn capabilities(&self) -> String {
        self.state.call(McpCommand::Capabilities).await
    }
''',
    '''    async fn capabilities(&self) -> String {
        if !self.state.connected.load(Ordering::Relaxed) {
            return serde_json::json!({
                "schema_version": 2,
                "connected": false,
                "message": "Bevy host is not connected; runtime availability and permissions are unknown"
            })
            .to_string();
        }
        self.state.call(McpCommand::Capabilities).await
    }
''',
)

# Add a zero-permission regression: capability discovery itself must still succeed and report
# everything operationally disabled.
test_path = Path("crates/bevy-mcp-host/tests/surface_contract.rs")
text = test_path.read_text()
replace_once(
    "crates/bevy-mcp-host/tests/surface_contract.rs",
    "use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};",
    "use bevy_mcp_host::{BevyMcpPlugin, McpPermissions, PermissionLevel};",
)
text = test_path.read_text()
anchor = '''#[test]
fn full_permissions_expose_installed_raw_input_and_gamepad_command_works() {
'''
new_test = r'''#[test]
fn capabilities_remain_discoverable_with_no_operational_permissions() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions {
                level: PermissionLevel::None,
                allow_input: false,
                allow_runtime_control: false,
                allow_build: false,
            }),
    );

    ingress.push(41, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 41);
    assert_eq!(capabilities["connected"], true);
    assert_eq!(capabilities["permissions"]["level"], "none");
    assert_eq!(capabilities["ecs"]["inspect"]["allowed"], false);
    assert_eq!(capabilities["ecs"]["inspect"]["operational"], false);
}

#[test]
fn full_permissions_expose_installed_raw_input_and_gamepad_command_works() {
'''
if text.count(anchor) != 1:
    raise SystemExit("surface_contract.rs: full-permission test anchor mismatch")
test_path.write_text(text.replace(anchor, new_test, 1))

# Documentation reflects the final behavior.
path = Path("docs/tool-capabilities.md")
text = path.read_text()
text += r'''

`capabilities` remains available when the runtime permission level is `none`; it reports the live contract with `allowed: false` instead of denying the discovery request. When the MCP server is not attached to a Bevy host, it returns a minimal `connected: false` contract rather than fabricating runtime availability.

Capture availability is renderer-aware: a window or camera target alone is not considered operational unless Bevy's `RenderDevice` is present.
'''
path.write_text(text)
