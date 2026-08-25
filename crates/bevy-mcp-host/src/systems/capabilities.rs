use super::camera::active_camera_entity;
use super::*;

pub(crate) fn capability(implemented: bool, available: bool, allowed: bool) -> Value {
    json!({
        "implemented": implemented,
        "available": available,
        "allowed": allowed,
        "operational": implemented && available && allowed,
    })
}

pub(crate) fn capabilities(world: &World) -> McpResult {
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
    let gamepad_button_available = world
        .iter_entities()
        .any(|entity| entity.contains::<bevy::input::gamepad::Gamepad>());
    let pointer_available = crate::interaction::pointer_available(world);
    let camera_available = active_camera_entity(world).is_some();
    let camera_frame_available = active_camera_entity(world).is_some_and(|camera| {
        matches!(
            world.get::<bevy::camera::Projection>(camera),
            Some(
                bevy::camera::Projection::Perspective(_)
                    | bevy::camera::Projection::Orthographic(_)
            )
        )
    });
    let renderer_available = world
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
    let mesh_spawn_available = world.contains_resource::<Assets<Mesh>>()
        && world.contains_resource::<Assets<bevy::pbr::StandardMaterial>>();
    let reflected_types_available = world.contains_resource::<AppTypeRegistry>();
    let asset_server_available = world.contains_resource::<bevy::asset::AssetServer>();
    let tracker_available = world.contains_resource::<crate::change_tracking::WorldChangeTracker>();
    let system_access_available =
        world.contains_resource::<crate::agent_api::McpSystemAccessRegistry>();
    let timings_available = world.contains_resource::<crate::agent_api::McpSystemTimings>();
    let debugger_available = world.contains_resource::<crate::debugger::McpDebugger>();
    let checkpoints_available = world
        .contains_resource::<crate::checkpoint::McpCheckpointRegistry>()
        && world.contains_resource::<crate::checkpoint::McpCheckpointStore>();
    let recorder_available = world.contains_resource::<crate::checkpoint::McpRecorder>();

    McpResult::success(json!({
        "schema_version": 2,
        "connected": true,
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
            "atomic_mutation_batch": capability(true, reflected_types_available, can_mutate),
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
            "mouse_move": capability(true, pointer_available, can_input),
            "action": capability(false, false, false),
            "gamepad_button": capability(true, gamepad_button_available, can_input),
        },
        "interaction": {
            "pick_at": capability(true, pointer_available, can_input),
            "pointer_move": capability(true, pointer_available, can_input),
            "pointer_click": capability(true, pointer_available, can_input),
            "pointer_drag": capability(true, pointer_available, can_input),
            "pointer_scroll": capability(true, pointer_available, can_input),
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
            "click": capability(true, pointer_available, can_input),
            "type_text": capability(true, true, can_input),
        },
        "camera": {
            "list": capability(true, true, can_read),
            "inspect": capability(true, true, can_read),
            "frame_entity": capability(true, camera_frame_available, can_runtime),
            "set_transform": capability(true, camera_available, can_runtime),
            "look_at": capability(true, camera_available, can_runtime),
        },
        "assets": {
            "list": capability(false, false, false),
            "inspect": capability(true, asset_server_available, can_read),
            "status": capability(true, asset_server_available, can_read),
            "reload": capability(true, asset_server_available, can_runtime),
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
