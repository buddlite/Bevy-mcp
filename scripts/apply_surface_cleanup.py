from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}: {old[:80]!r}")
    file.write_text(text.replace(old, new, 1))


# Core command: capabilities must be answered by the Bevy host, not guessed by the server.
replace_once(
    "crates/bevy-mcp-core/src/command.rs",
    "    WorldSummary,\n    // -- World context --",
    "    WorldSummary,\n    Capabilities,\n    // -- World context --",
)

# Host capability contract.
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    "use crate::permissions::McpPermissions;",
    "use crate::permissions::{McpPermissions, PermissionLevel};",
)
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    "        McpCommand::WorldSummary => world_summary(world),\n        McpCommand::WorldContextScan => world_context_scan(world, registry),",
    "        McpCommand::WorldSummary => world_summary(world),\n        McpCommand::Capabilities => capabilities(world),\n        McpCommand::WorldContextScan => world_context_scan(world, registry),",
)

capabilities_impl = r'''fn capability(implemented: bool, available: bool, allowed: bool) -> Value {
    json!({
        "implemented": implemented,
        "available": available,
        "allowed": allowed,
        "operational": implemented && available && allowed,
    })
}

fn capabilities(world: &World) -> McpResult {
    let permissions = world.resource::<McpPermissions>();
    let can_read = permissions.level != PermissionLevel::None;
    let can_mutate = permissions.can_mutate();
    let can_input = permissions.can_inject_input();
    let can_runtime = permissions.can_control_runtime();
    let can_build = permissions.can_build();
    let permission_level = match permissions.level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
        PermissionLevel::Full => "full",
    };

    let key_input_available = world.contains_resource::<ButtonInput<KeyCode>>();
    let mouse_button_available = world.contains_resource::<ButtonInput<MouseButton>>();
    let gamepad_button_available = world.contains_resource::<ButtonInput<GamepadButton>>();
    let primary_window_available = world
        .iter_entities()
        .any(|entity| entity.contains::<bevy::window::PrimaryWindow>());
    let camera_target_available = world
        .iter_entities()
        .any(|entity| entity.contains::<bevy::camera::RenderTarget>());
    let ui_capture_available = world
        .get_resource::<crate::agent_api::McpCaptureTargets>()
        .and_then(|targets| targets.ui_target())
        .is_some();
    let mesh_spawn_available = world.contains_resource::<Assets<Mesh>>()
        && world.contains_resource::<Assets<bevy::pbr::StandardMaterial>>();
    let reflected_types_available = world.contains_resource::<AppTypeRegistry>();
    let tracker_available = world.contains_resource::<crate::change_tracking::WorldChangeTracker>();
    let system_access_available =
        world.contains_resource::<crate::agent_api::McpSystemAccessRegistry>();
    let timings_available = world.contains_resource::<crate::agent_api::McpSystemTimings>();
    let debugger_available = world.contains_resource::<crate::debugger::McpDebugger>();
    let checkpoints_available =
        world.contains_resource::<crate::checkpoint::McpCheckpointRegistry>()
            && world.contains_resource::<crate::checkpoint::McpCheckpointStore>();
    let recorder_available = world.contains_resource::<crate::checkpoint::McpRecorder>();

    McpResult::success(json!({
        "schema_version": 2,
        "permissions": {
            "level": permission_level,
            "ecs_mutation": can_mutate,
            "input": can_input,
            "runtime_control": can_runtime,
            "build": can_build,
        },
        "transport": {
            "concurrent_correlated_requests": capability(true, true, can_read),
        },
        "ecs": {
            "inspect": capability(true, true, can_read),
            "query": capability(true, true, can_read),
            "hierarchy": capability(true, true, can_read),
            "reflection": capability(true, reflected_types_available, can_read),
            "mutate": capability(true, reflected_types_available, can_mutate),
            "entity_duplicate": capability(false, false, false),
        },
        "runtime": {
            "pause": capability(true, true, can_runtime),
            "resume": capability(true, true, can_runtime),
            "step": capability(true, true, can_runtime),
            "time_scale": capability(true, true, can_runtime),
            "launch": capability(false, false, false),
            "stop": capability(false, false, false),
            "restart": capability(false, false, false),
        },
        "input": {
            "key": capability(true, key_input_available, can_input),
            "mouse_button": capability(true, mouse_button_available, can_input),
            "mouse_move": capability(false, false, false),
            "action": capability(false, false, false),
            "gamepad_button": capability(true, gamepad_button_available, can_input),
        },
        "capture": {
            "viewport": capability(true, primary_window_available, can_read),
            "camera_target": capability(true, camera_target_available, can_read),
            "ui_only": capability(true, ui_capture_available, can_read),
        },
        "diagnostics": {
            "logs": capability(true, true, can_read),
            "events": capability(true, true, can_read),
            "change_tracking": capability(true, tracker_available, can_read),
            "system_access": capability(true, system_access_available, can_read),
            "system_timings": capability(true, timings_available, can_read),
        },
        "debugger": {
            "watchpoints": capability(true, debugger_available, can_read),
            "playtests": capability(true, debugger_available, can_runtime && can_input),
            "checkpoint_create": capability(true, checkpoints_available, can_read),
            "checkpoint_restore": capability(true, checkpoints_available, can_mutate),
            "recording": capability(true, recorder_available, can_read),
            "replay": capability(true, recorder_available, can_runtime && can_input),
        },
        "ui": {
            "query": capability(true, true, can_read),
            "inspect": capability(true, true, can_read),
            "click": capability(false, false, false),
            "type_text": capability(false, false, false),
        },
        "camera": {
            "list": capability(true, true, can_read),
            "inspect": capability(true, true, can_read),
            "frame_entity": capability(false, false, false),
            "set_transform": capability(false, false, false),
            "look_at": capability(false, false, false),
        },
        "assets": {
            "list": capability(false, false, false),
            "inspect": capability(false, false, false),
            "status": capability(false, false, false),
            "reload": capability(false, false, false),
        },
        "procedural": {
            "mesh_spawn": capability(true, mesh_spawn_available, can_mutate),
            "template_save": capability(true, reflected_types_available, can_read),
            "template_load": capability(true, reflected_types_available, can_mutate),
        },
        "build": {
            "check": capability(false, false, can_build),
            "build": capability(false, false, can_build),
            "test": capability(false, false, can_build),
        },
        "deprecations": [
            {
                "tool": "capture_game",
                "status": "deprecated_alias",
                "functional": true,
                "replacement": "capture_viewport"
            },
            {
                "tool": "capture_camera",
                "status": "deprecated_alias",
                "functional": true,
                "replacement": "capture_viewport"
            },
            {
                "tool": "playtest_run",
                "status": "deprecated_unavailable",
                "functional": false,
                "replacement": "playtest_start"
            }
        ]
    }))
}

'''
replace_once(
    "crates/bevy-mcp-host/src/systems.rs",
    "fn world_summary(world: &World) -> McpResult {",
    capabilities_impl + "fn world_summary(world: &World) -> McpResult {",
)

# S2: writer kind is defined by the API the caller selected, not by whether a resource instance
# happens to be present in the world at this frame.
replace_once(
    "crates/bevy-mcp-host/src/advanced.rs",
    "    let is_resource = world.contains_resource_by_id(info.id());",
    "    let is_resource = requested_kind == \"resource\";",
)

# Server capabilities now ask the runtime for the actual contract.
old_capabilities = '''    #[tool(description = "List capabilities this server instance provides")]
    fn capabilities(&self) -> String {
        serde_json::json!({
            "ecs": { "inspect": true, "mutate": true, "query": true, "hierarchy": true, "reflection": true },
            "runtime": { "control": true, "step": true, "time_scale": true, "pause": true },
            "input": { "raw": true, "actions": false },
            "capture": { "game": false, "camera": false },
            "assets": { "inspect": false, "reload": false },
            "diagnostics": { "render": false, "performance": true, "logs": true, "observe_events": true },
            "ui": { "query": true, "inspect": true, "click": true, "type_text": true },
            "procedural": { "mesh_spawn": true, "template_save": true, "template_load": true },
            "plugins": { "list": true },
            "build": { "cargo": false, "check": false, "test": false }
        })
        .to_string()
    }
'''
new_capabilities = '''    #[tool(
        description = "Report the live MCP capability contract from the Bevy host, including implementation, runtime availability, permission allowance, and deprecations."
    )]
    async fn capabilities(&self) -> String {
        self.state.call(McpCommand::Capabilities).await
    }
'''
replace_once("crates/bevy-mcp-server/src/tools.rs", old_capabilities, new_capabilities)

# Wire through gamepad input that the host already implements.
old_gamepad = '''    #[tool(description = "Inject a gamepad button press/release")]
    async fn input_gamepad(&self, Parameters(params): Parameters<InputGamepadParams>) -> String {
        // Gamepad input requires GamepadInput resource which is not yet implemented.
        // For now, return a clear error.
        serde_json::json!({
            "error": "NOT_IMPLEMENTED",
            "message": format!("Gamepad injection not yet implemented (button={})", params.button)
        })
        .to_string()
    }
'''
new_gamepad = '''    #[tool(description = "Inject a gamepad button press/release when Bevy's ButtonInput<GamepadButton> resource is installed.")]
    async fn input_gamepad(&self, Parameters(params): Parameters<InputGamepadParams>) -> String {
        self.state
            .call(McpCommand::InputGamepad {
                button: params.button,
                pressed: params.pressed.unwrap_or(true),
            })
            .await
    }
'''
replace_once("crates/bevy-mcp-server/src/tools.rs", old_gamepad, new_gamepad)

# The legacy playtest tool never executed; make that explicit and direct agents to the debugger.
old_playtest = '''    #[tool(description = "Run a sequence of playtest steps (restart, input, assert, capture)")]
    async fn playtest_run(&self, Parameters(params): Parameters<PlaytestRunParams>) -> String {
        let steps: Vec<bevy_mcp_core::command::PlaytestStep> = params
            .steps
            .into_iter()
            .map(|s| match s.action.as_str() {
                "runtime_restart" => bevy_mcp_core::command::PlaytestStep::RuntimeRestart,
                "runtime_step" => bevy_mcp_core::command::PlaytestStep::RuntimeStep {
                    frames: s.frames.unwrap_or(1),
                },
                "input_action" => bevy_mcp_core::command::PlaytestStep::InputAction {
                    action: s.action_name.unwrap_or_default(),
                    duration_secs: s.duration.unwrap_or(1.0),
                },
                "capture_game" => bevy_mcp_core::command::PlaytestStep::CaptureGame {
                    name: s.name.unwrap_or_else(|| "unnamed".to_string()),
                },
                _ => bevy_mcp_core::command::PlaytestStep::RuntimeStep { frames: 1 },
            })
            .collect();

        self.state.call(McpCommand::PlaytestRun { steps }).await
    }
'''
new_playtest = '''    #[tool(
        description = "DEPRECATED and unavailable. Use playtest_start/playtest_status for the frame-driven debugger playtest engine."
    )]
    async fn playtest_run(&self, Parameters(_params): Parameters<PlaytestRunParams>) -> String {
        error(
            "DEPRECATED_TOOL",
            "playtest_run never had an executable host implementation; use playtest_start and playtest_status",
        )
    }
'''
replace_once("crates/bevy-mcp-server/src/tools.rs", old_playtest, new_playtest)

# Make remaining known stubs/aliases truthful at MCP tool-discovery time.
description_updates = {
    '    #[tool(description = "Click a UI element (button, link, etc.)")]':
        '    #[tool(description = "Reserved for the Agent Interaction subsystem; UI click injection is not implemented yet.")]',
    '    #[tool(description = "Type text into a UI text input field")]':
        '    #[tool(description = "Reserved for the Agent Interaction subsystem; UI text injection is not implemented yet.")]',
    '    #[tool(description = "Duplicate an entity and all its components")]':
        '    #[tool(description = "Reserved: entity duplication is not implemented until safe reflected component cloning is available.")]',
    '    #[tool(description = "Launch/run the Bevy application")]':
        '    #[tool(description = "Unavailable in embedded mode: application launch is owned by the embedding process.")]',
    '    #[tool(description = "Stop the running Bevy application")]':
        '    #[tool(description = "Unavailable in embedded mode: application stop is owned by the embedding process.")]',
    '    #[tool(description = "Restart the Bevy application (stop + launch)")]':
        '    #[tool(description = "Unavailable in embedded mode: application restart is owned by the embedding process.")]',
    '    #[tool(description = "Inject a mouse event (motion or button)")]':
        '    #[tool(description = "Inject a mouse button event. The motion variant is reserved for Agent Interaction and currently returns NOT_IMPLEMENTED.")]',
    '    #[tool(description = "Inject a high-level input action (e.g. \'move_forward\', \'fire\')")]':
        '    #[tool(description = "Unavailable without a game-specific semantic action adapter; use semantic_action_invoke for registered game actions.")]',
    '    #[tool(description = "Capture a screenshot of the game viewport")]':
        '    #[tool(description = "DEPRECATED alias for capture_viewport using the primary window defaults.")]',
    '    #[tool(description = "List loaded assets, optionally filtered by type")]':
        '    #[tool(description = "Reserved: loaded-asset enumeration is not implemented yet.")]',
    '    #[tool(description = "Get asset metadata by path")]':
        '    #[tool(description = "Reserved: asset metadata inspection is not implemented yet.")]',
    '    #[tool(description = "Get asset loading status")]':
        '    #[tool(description = "Reserved: asset loading-status inspection is not implemented yet.")]',
    '    #[tool(description = "Force reload an asset")]':
        '    #[tool(description = "Reserved: asset reload is not implemented yet.")]',
    '    #[tool(description = "Move the inspection camera to frame a specific entity")]':
        '    #[tool(description = "Reserved for Agent Interaction: camera framing is not implemented yet.")]',
    '    #[tool(description = "Set camera position")]':
        '    #[tool(description = "Reserved for Agent Interaction: camera transform control is not implemented yet.")]',
    '    #[tool(description = "Point camera at an entity")]':
        '    #[tool(description = "Reserved for Agent Interaction: camera look-at control is not implemented yet.")]',
    '    #[tool(description = "Capture a screenshot from the active camera")]':
        '    #[tool(description = "DEPRECATED alias for capture_viewport. Use capture_viewport with an explicit camera entity for camera-target capture.")]',
    '    #[tool(description = "Run cargo check on the project. Returns structured errors.")]':
        '    #[tool(description = "Unavailable from the embedded MCP server; run cargo check in a trusted development shell.")]',
    '    #[tool(description = "Build the project with cargo. Returns structured build result.")]':
        '    #[tool(description = "Unavailable from the embedded MCP server; run cargo build in a trusted development shell.")]',
    '    #[tool(description = "Run cargo test. Returns structured test results.")]':
        '    #[tool(description = "Unavailable from the embedded MCP server; run cargo test in a trusted development shell.")]',
}
for old, new in description_updates.items():
    replace_once("crates/bevy-mcp-server/src/tools.rs", old, new)

# S2 regression coverage: a registered resource writer must remain discoverable after the resource
# instance is removed from the world.
intelligence_path = Path("crates/bevy-mcp-host/tests/intelligence.rs")
intelligence = intelligence_path.read_text()
imports_old = '''use bevy::prelude::*;
use bevy_mcp_host::change_tracking::WorldChangeTracker;
use bevy_mcp_host::{
'''
imports_new = '''use bevy::prelude::*;
use bevy_mcp_core::advanced::{AdvancedRequest, encode_advanced_request};
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::change_tracking::WorldChangeTracker;
use bevy_mcp_host::{
'''
if intelligence.count(imports_old) != 1:
    raise SystemExit("intelligence.rs: import anchor mismatch")
intelligence = intelligence.replace(imports_old, imports_new, 1)
intelligence = intelligence.replace(
    '''    McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpRecorder,
    McpSystemAccessRegistry, McpSystemAccessSpec, RecordedAction,
''',
    '''    BevyMcpPlugin, McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpPermissions,
    McpRecorder, McpSystemAccessRegistry, McpSystemAccessSpec, RecordedAction,
''',
    1,
)
intelligence += r'''

#[derive(Resource)]
struct DormantStats;

#[test]
fn resource_writers_remain_resource_typed_when_instance_is_absent() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::read_only()),
    );
    app.insert_resource(DormantStats);
    app.register_mcp_system_access(
        McpSystemAccessSpec::new("economy::write_dormant_stats")
            .schedule("Update")
            .write_resource::<DormantStats>(),
    );
    app.world_mut().remove_resource::<DormantStats>();

    let operation_id = encode_advanced_request(&AdvancedRequest::ResourceWriters {
        resource: "DormantStats".into(),
        schedule: None,
    })
    .unwrap();
    ingress.push(
        77,
        McpCommand::OperationStatus {
            operation_id: Some(operation_id),
        },
    );
    app.update();

    let response = results
        .drain()
        .into_iter()
        .find(|response| response.request_id == 77)
        .expect("resource writer response");
    let value = match response.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    };
    assert_eq!(value["kind"], "resource");
    assert!(value["writers"].as_array().unwrap().iter().any(|writer| {
        writer["system"].as_str() == Some("economy::write_dormant_stats")
    }));
}
'''
intelligence_path.write_text(intelligence)

# Live capability and gamepad contract tests.
Path("crates/bevy-mcp-host/tests/surface_contract.rs").write_text(r'''use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::Value;

fn success_for(results: &McpResultQueue, request_id: u64) -> Value {
    let response = results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response");
    match response.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected MCP error {code}: {message}"),
    }
}

#[test]
fn capabilities_are_live_and_permission_aware() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::read_only()),
    );

    ingress.push(1, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 1);

    assert_eq!(capabilities["schema_version"], 2);
    assert_eq!(capabilities["permissions"]["level"], "read");
    assert_eq!(capabilities["runtime"]["pause"]["implemented"], true);
    assert_eq!(capabilities["runtime"]["pause"]["allowed"], false);
    assert_eq!(capabilities["ui"]["click"]["implemented"], false);
    assert_eq!(capabilities["input"]["mouse_move"]["implemented"], false);
    assert_eq!(capabilities["capture"]["viewport"]["implemented"], true);
    assert_eq!(capabilities["capture"]["viewport"]["available"], false);
    assert_eq!(capabilities["debugger"]["watchpoints"]["implemented"], true);
    assert!(capabilities["deprecations"].as_array().unwrap().iter().any(|entry| {
        entry["tool"] == "playtest_run" && entry["replacement"] == "playtest_start"
    }));
}

#[test]
fn full_permissions_expose_installed_raw_input_and_gamepad_command_works() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(ButtonInput::<MouseButton>::default());
    app.insert_resource(ButtonInput::<GamepadButton>::default());

    ingress.push(2, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 2);
    for key in ["key", "mouse_button", "gamepad_button"] {
        assert_eq!(capabilities["input"][key]["implemented"], true);
        assert_eq!(capabilities["input"][key]["available"], true);
        assert_eq!(capabilities["input"][key]["allowed"], true);
        assert_eq!(capabilities["input"][key]["operational"], true);
    }

    ingress.push(
        3,
        McpCommand::InputGamepad {
            button: "south".into(),
            pressed: true,
        },
    );
    app.update();
    let result = success_for(&results, 3);
    assert_eq!(result["button"], "south");
    assert_eq!(result["pressed"], true);
    assert!(app
        .world()
        .resource::<ButtonInput<GamepadButton>>()
        .pressed(GamepadButton::South));
}
''')

Path("docs/tool-capabilities.md").write_text(r'''# MCP capability contract

`capabilities` is a live host query. It no longer returns a hard-coded server-side feature list.

Each capability reports four independent fields:

- `implemented`: the MCP has an implementation for this operation.
- `available`: the current Bevy app has the runtime resource/target needed by that implementation.
- `allowed`: current `McpPermissions` allow the operation.
- `operational`: all three conditions are true.

This distinction prevents misleading results. For example, viewport capture is implemented but can be unavailable in a `MinimalPlugins` app without a primary window; key input can be implemented and installed but disallowed under read-only permissions.

The response also includes a `deprecations` array. Legacy `capture_game` and `capture_camera` remain functional aliases for `capture_viewport`, while the old `playtest_run` surface is explicitly unavailable and points agents to the frame-driven `playtest_start`/`playtest_status` debugger API.

Known interaction surfaces reserved for the next Agent Interaction work—mouse motion, UI click/type, camera framing/transform/look-at—report `implemented: false` instead of being advertised as working. Asset inspection/reload and embedded cargo build/test surfaces likewise report false.

`resource_writers` and `component_writers` use the selected API kind to choose the exact registered access list. Resource-writer discovery therefore continues to work when a registered resource type currently has no live resource instance.
''')
