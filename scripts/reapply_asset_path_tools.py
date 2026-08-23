from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}")
    file.write_text(text.replace(old, new, 1))


def replace_between(path: str, start: str, end: str, replacement: str) -> None:
    file = Path(path)
    text = file.read_text()
    i = text.index(start)
    j = text.index(end, i)
    file.write_text(text[:i] + replacement + text[j:])


systems = "crates/bevy-mcp-host/src/systems.rs"

replace_once(
    systems,
    '''        | McpCommand::CameraFrameEntity { .. }
        | McpCommand::CameraSetTransform { .. }
        | McpCommand::CameraLookAt { .. } => permissions.can_control_runtime(),
''',
    '''        | McpCommand::CameraFrameEntity { .. }
        | McpCommand::CameraSetTransform { .. }
        | McpCommand::CameraLookAt { .. }
        | McpCommand::AssetReload { .. } => permissions.can_control_runtime(),
''',
)

replace_once(
    systems,
    '''    let reflected_types_available = world.contains_resource::<AppTypeRegistry>();
    let tracker_available = world.contains_resource::<crate::change_tracking::WorldChangeTracker>();
''',
    '''    let reflected_types_available = world.contains_resource::<AppTypeRegistry>();
    let asset_server_available = world.contains_resource::<bevy::asset::AssetServer>();
    let tracker_available = world.contains_resource::<crate::change_tracking::WorldChangeTracker>();
''',
)

replace_once(
    systems,
    '''        "assets": {
            "list": capability(false, false, false),
            "inspect": capability(false, false, false),
            "status": capability(false, false, false),
            "reload": capability(false, false, false),
        },
''',
    '''        "assets": {
            "list": capability(false, false, false),
            "inspect": capability(true, asset_server_available, can_read),
            "status": capability(true, asset_server_available, can_read),
            "reload": capability(true, asset_server_available, can_runtime),
        },
''',
)

replace_between(
    systems,
    "fn asset_list(world: &World, filter: Option<&str>) -> McpResult {",
    "\nfn capture_game(world: &World) -> McpResult {",
    '''fn asset_list(world: &World, filter: Option<&str>) -> McpResult {
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };
    let _ = filter;
    McpResult::error(
        "NOT_IMPLEMENTED",
        "Global asset enumeration is not exposed by Bevy's public AssetServer API; inspect known paths with asset_get/asset_status",
    )
}

fn load_state_json(state: &bevy::asset::LoadState) -> Value {
    match state {
        bevy::asset::LoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::LoadState::Loading => json!({"state": "loading"}),
        bevy::asset::LoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::LoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn dependency_load_state_json(state: &bevy::asset::DependencyLoadState) -> Value {
    match state {
        bevy::asset::DependencyLoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::DependencyLoadState::Loading => json!({"state": "loading"}),
        bevy::asset::DependencyLoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::DependencyLoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn recursive_dependency_load_state_json(
    state: &bevy::asset::RecursiveDependencyLoadState,
) -> Value {
    match state {
        bevy::asset::RecursiveDependencyLoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::RecursiveDependencyLoadState::Loading => json!({"state": "loading"}),
        bevy::asset::RecursiveDependencyLoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::RecursiveDependencyLoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn asset_type_name(world: &World, type_id: std::any::TypeId) -> Option<String> {
    let registry = world.get_resource::<AppTypeRegistry>()?.read();
    registry
        .iter()
        .find(|registration| registration.type_id() == type_id)
        .map(|registration| registration.type_info().type_path_table().path().to_string())
}

fn asset_path_snapshot(world: &World, path: &str) -> McpResult {
    let Some(asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    let ids = asset_server.get_path_ids(path.to_owned());
    if ids.is_empty() {
        return McpResult::success(json!({
            "path": path,
            "active": false,
            "status": "not_loaded",
            "assets": [],
        }));
    }

    let mut any_failed = false;
    let mut all_ready = true;
    let mut rows = Vec::with_capacity(ids.len());

    for id in ids {
        let type_id = id.type_id();
        let type_name = asset_type_name(world, type_id);
        let id_debug = format!("{id:?}");
        match asset_server.get_load_states(id) {
            Some((root, dependencies, recursive_dependencies)) => {
                let ready = root.is_loaded() && recursive_dependencies.is_loaded();
                any_failed |= root.is_failed()
                    || dependencies.is_failed()
                    || recursive_dependencies.is_failed();
                all_ready &= ready;
                rows.push(json!({
                    "id": id_debug,
                    "type_id": format!("{type_id:?}"),
                    "type_name": type_name,
                    "ready": ready,
                    "load": load_state_json(&root),
                    "dependencies": dependency_load_state_json(&dependencies),
                    "recursive_dependencies": recursive_dependency_load_state_json(&recursive_dependencies),
                }));
            }
            None => {
                all_ready = false;
                rows.push(json!({
                    "id": id_debug,
                    "type_id": format!("{type_id:?}"),
                    "type_name": type_name,
                    "ready": false,
                    "load": {"state": "unknown"},
                    "dependencies": {"state": "unknown"},
                    "recursive_dependencies": {"state": "unknown"},
                }));
            }
        }
    }

    let status = if any_failed {
        "failed"
    } else if all_ready {
        "loaded"
    } else {
        "loading"
    };

    McpResult::success(json!({
        "path": path,
        "active": true,
        "status": status,
        "assets": rows,
    }))
}

fn asset_get(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

fn asset_status(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

fn asset_reload(world: &World, path: &str) -> McpResult {
    let Some(asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    let ids = asset_server.get_path_ids(path.to_owned());
    if ids.is_empty() {
        return McpResult::error(
            "ASSET_NOT_ACTIVE",
            format!("Asset path '{path}' has no active AssetServer handle"),
        );
    }
    let loaded = ids
        .iter()
        .any(|id| asset_server.get_load_state(*id).is_some_and(|state| state.is_loaded()));
    if !loaded {
        return McpResult::error(
            "ASSET_NOT_LOADED",
            format!("Asset path '{path}' is active but is not currently loaded"),
        );
    }

    asset_server.reload(path);
    McpResult::success(json!({
        "path": path,
        "reload_queued": true,
        "active_asset_count": ids.len(),
    }))
}
''',
)

server = "crates/bevy-mcp-server/src/tools.rs"
replace_once(
    server,
    '    #[tool(description = "Reserved: asset metadata inspection is not implemented yet.")]\n',
    '    #[tool(description = "Inspect a known asset path, including active asset IDs, runtime type information, and load/dependency states.")]\n',
)
replace_once(
    server,
    '    #[tool(description = "Reserved: asset loading-status inspection is not implemented yet.")]\n',
    '    #[tool(description = "Get root, direct-dependency, and recursive-dependency load state for a known asset path.")]\n',
)
replace_once(
    server,
    '    #[tool(description = "Reserved: asset reload is not implemented yet.")]\n',
    '    #[tool(description = "Queue a reload for an active loaded asset path. Requires runtime-control permission.")]\n',
)

replace_once(
    "docs/tool-capabilities.md",
    "Native pointer motion/picking, UI click/type, and camera framing/transform/look-at are implemented by the Agent Interaction layer; their `available`, `allowed`, and `operational` fields still reflect the live app and permission state. Asset inspection/reload and embedded cargo build/test surfaces remain unimplemented and report false.",
    "Native pointer motion/picking, UI click/type, and camera framing/transform/look-at are implemented by the Agent Interaction layer; their `available`, `allowed`, and `operational` fields still reflect the live app and permission state. Path-targeted asset inspection, status, and reload are implemented when `AssetServer` is present; global asset enumeration remains unavailable because Bevy's public `AssetServer` API does not expose an all-path iterator. Embedded cargo build/test surfaces remain unimplemented and report false.",
)

Path("crates/bevy-mcp-host/tests/assets.rs").write_text(r'''use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::Value;

fn response_for(results: &McpResultQueue, request_id: u64) -> McpResult {
    results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response")
        .result
}

fn success_for(results: &McpResultQueue, request_id: u64) -> Value {
    match response_for(results, request_id) {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    }
}

fn asset_app(permissions: McpPermissions) -> (App, McpIngressQueue, McpResultQueue) {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(permissions),
        );
    (app, ingress, results)
}

#[test]
fn asset_capabilities_reflect_asset_server_and_permissions() {
    let (mut app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(1, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 1);

    assert_eq!(capabilities["assets"]["list"]["implemented"], false);
    for key in ["inspect", "status"] {
        assert_eq!(capabilities["assets"][key]["implemented"], true);
        assert_eq!(capabilities["assets"][key]["available"], true);
        assert_eq!(capabilities["assets"][key]["allowed"], true);
        assert_eq!(capabilities["assets"][key]["operational"], true);
    }
    assert_eq!(capabilities["assets"]["reload"]["implemented"], true);
    assert_eq!(capabilities["assets"]["reload"]["available"], true);
    assert_eq!(capabilities["assets"]["reload"]["allowed"], false);
    assert_eq!(capabilities["assets"]["reload"]["operational"], false);
}

#[test]
fn asset_status_reports_unknown_path_as_not_loaded() {
    let (mut app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(
        2,
        McpCommand::AssetStatus {
            path: "textures/not-loaded.png".into(),
        },
    );
    app.update();
    let status = success_for(&results, 2);
    assert_eq!(status["path"], "textures/not-loaded.png");
    assert_eq!(status["active"], false);
    assert_eq!(status["status"], "not_loaded");
    assert_eq!(status["assets"].as_array().unwrap().len(), 0);
}

#[test]
fn asset_reload_requires_runtime_permission_and_an_active_loaded_path() {
    let (mut read_app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(
        3,
        McpCommand::AssetReload {
            path: "textures/not-loaded.png".into(),
        },
    );
    read_app.update();
    match response_for(&results, 3) {
        McpResult::Error { code, .. } => assert_eq!(code, "PERMISSION_DENIED"),
        result => panic!("expected permission denial, got {result:?}"),
    }

    let (mut full_app, ingress, results) = asset_app(McpPermissions::full());
    ingress.push(
        4,
        McpCommand::AssetReload {
            path: "textures/not-loaded.png".into(),
        },
    );
    full_app.update();
    match response_for(&results, 4) {
        McpResult::Error { code, .. } => assert_eq!(code, "ASSET_NOT_ACTIVE"),
        result => panic!("expected inactive-path error, got {result:?}"),
    }
}
''')
