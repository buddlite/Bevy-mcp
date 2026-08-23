use super::*;

pub(crate) fn asset_list(world: &World, filter: Option<&str>) -> McpResult {
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

pub(crate) fn load_state_json(state: &bevy::asset::LoadState) -> Value {
    match state {
        bevy::asset::LoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::LoadState::Loading => json!({"state": "loading"}),
        bevy::asset::LoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::LoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

pub(crate) fn dependency_load_state_json(state: &bevy::asset::DependencyLoadState) -> Value {
    match state {
        bevy::asset::DependencyLoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::DependencyLoadState::Loading => json!({"state": "loading"}),
        bevy::asset::DependencyLoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::DependencyLoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

pub(crate) fn recursive_dependency_load_state_json(
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

pub(crate) fn asset_type_name(world: &World, type_id: std::any::TypeId) -> Option<String> {
    let registry = world.get_resource::<AppTypeRegistry>()?.read();
    registry
        .iter()
        .find(|registration| registration.type_id() == type_id)
        .map(|registration| {
            registration
                .type_info()
                .type_path_table()
                .path()
                .to_string()
        })
}

pub(crate) fn asset_path_snapshot(world: &World, path: &str) -> McpResult {
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

pub(crate) fn asset_get(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

pub(crate) fn asset_status(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

pub(crate) fn asset_reload(world: &World, path: &str) -> McpResult {
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
    let loaded = ids.iter().any(|id| {
        asset_server
            .get_load_state(*id)
            .is_some_and(|state| state.is_loaded())
    });
    if !loaded {
        return McpResult::error(
            "ASSET_NOT_LOADED",
            format!("Asset path '{path}' is active but is not currently loaded"),
        );
    }

    asset_server.reload(path.to_owned());
    McpResult::success(json!({
        "path": path,
        "reload_queued": true,
        "active_asset_count": ids.len(),
    }))
}
