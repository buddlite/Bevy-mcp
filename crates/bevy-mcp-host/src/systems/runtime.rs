use super::*;

pub fn runtime_system(mut registry: ResMut<McpRegistry>, mut time: ResMut<Time<Virtual>>) {
    if registry.paused {
        if registry.step_remaining > 0 {
            registry.step_remaining -= 1;
            time.unpause();
        } else {
            time.pause();
        }
    } else {
        time.unpause();
        time.set_relative_speed_f64(registry.time_scale);
    }
}

pub fn diagnostics_system(mut registry: ResMut<McpRegistry>) {
    registry.frame += 1;
}

pub(crate) fn runtime_pause(registry: &mut McpRegistry) -> McpResult {
    registry.paused = true;
    McpResult::success(json!({ "paused": true }))
}

pub(crate) fn runtime_resume(registry: &mut McpRegistry) -> McpResult {
    registry.paused = false;
    registry.step_remaining = 0;
    McpResult::success(json!({ "paused": false }))
}

pub(crate) fn runtime_step(registry: &mut McpRegistry, frames: u32) -> McpResult {
    registry.paused = true;
    registry.step_remaining = frames;
    McpResult::success(json!({ "paused": true, "step_frames": frames }))
}

pub(crate) fn runtime_time_scale(registry: &mut McpRegistry, scale: f64) -> McpResult {
    if !scale.is_finite() || scale < 0.0 {
        return McpResult::error(
            "INVALID_TIME_SCALE",
            "time scale must be finite and greater than or equal to zero",
        );
    }
    registry.time_scale = scale;
    McpResult::success(json!({ "time_scale": scale }))
}

pub(crate) fn logs(world: &World, level: &Option<String>, limit: u32) -> McpResult {
    let log_capture = match world.get_resource::<crate::log_capture::LogCapture>() {
        Some(lc) => lc,
        None => {
            return McpResult::error(
                "LOG_CAPTURE_NOT_AVAILABLE",
                "LogCapture resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    let entries = log_capture.get_entries(level.as_deref(), limit as usize);
    let log_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "level": entry.level,
                "message": entry.message,
                "target": entry.target,
                "timestamp": entry.timestamp,
            })
        })
        .collect();

    McpResult::success(json!({
        "logs": log_entries,
        "count": log_entries.len(),
    }))
}

pub(crate) fn diagnostics(world: &World, registry: &McpRegistry) -> McpResult {
    let entity_count = world.iter_entities().count();
    McpResult::success(json!({
        "frame": registry.frame,
        "entity_count": entity_count,
        "paused": registry.paused,
        "time_scale": registry.time_scale,
    }))
}

pub(crate) fn observe_events(world: &World, event_type: &Option<String>, limit: u32) -> McpResult {
    let event_capture = match world.get_resource::<crate::event_capture::EventCapture>() {
        Some(ec) => ec,
        None => {
            return McpResult::error(
                "EVENT_CAPTURE_NOT_AVAILABLE",
                "EventCapture resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    let entries = event_capture.get_events(event_type.as_deref(), limit as usize);
    let event_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "event_type": entry.event_type,
                "data": entry.data,
                "timestamp": entry.timestamp,
            })
        })
        .collect();

    McpResult::success(json!({
        "events": event_entries,
        "count": event_entries.len(),
    }))
}

pub(crate) fn list_plugins(world: &World) -> McpResult {
    let checks: Vec<(&str, &str, bool)> = vec![
        (
            "InputPlugin",
            "bevy_input",
            world
                .get_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>()
                .is_some(),
        ),
        (
            "PickingPlugin",
            "bevy_picking",
            world
                .get_resource::<bevy::picking::PickingSettings>()
                .is_some(),
        ),
        (
            "GizmoPlugin",
            "bevy_gizmos",
            world
                .get_resource::<bevy::gizmos::config::GizmoConfigStore>()
                .is_some(),
        ),
        (
            "UiPlugin",
            "bevy_ui",
            world.get_resource::<bevy::ui::UiScale>().is_some(),
        ),
        (
            "TimePlugin",
            "bevy_time",
            world.get_resource::<bevy::time::Time<()>>().is_some(),
        ),
    ];

    let plugins: Vec<_> = checks
        .into_iter()
        .map(|(name, crate_name, installed)| {
            json!({
                "name": name,
                "crate": crate_name,
                "installed": installed,
            })
        })
        .collect();

    McpResult::success(json!({
        "plugins": plugins,
        "count": plugins.len(),
    }))
}

pub(crate) fn operation_status(world: &World, operation_id: Option<&str>) -> McpResult {
    let tracker = match world.get_resource::<crate::operations::OperationTracker>() {
        Some(t) => t,
        None => {
            return McpResult::error(
                "OPERATION_TRACKER_NOT_AVAILABLE",
                "OperationTracker resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };
    McpResult::success(tracker.get_status(operation_id))
}

pub(crate) fn operation_cancel(world: &World, operation_id: &str) -> McpResult {
    let tracker = match world.get_resource::<crate::operations::OperationTracker>() {
        Some(t) => t,
        None => {
            return McpResult::error(
                "OPERATION_TRACKER_NOT_AVAILABLE",
                "OperationTracker resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    if tracker.cancel(operation_id) {
        McpResult::success(json!({ "cancelled": operation_id }))
    } else {
        McpResult::error("NOT_FOUND", format!("Operation '{operation_id}' not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_time_scales_are_rejected_without_mutation() {
        let mut registry = McpRegistry::default();
        registry.time_scale = 1.0;
        for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let result = runtime_time_scale(&mut registry, invalid);
            assert!(matches!(result, McpResult::Error { ref code, .. } if code == "INVALID_TIME_SCALE"));
            assert_eq!(registry.time_scale, 1.0);
        }
    }

    #[test]
    fn finite_nonnegative_time_scales_are_valid() {
        let mut registry = McpRegistry::default();
        for valid in [0.0, 0.5, 1.0, 2.0, f64::MAX] {
            let result = runtime_time_scale(&mut registry, valid);
            assert!(matches!(result, McpResult::Success(_)));
            assert_eq!(registry.time_scale, valid);
        }
    }
}
