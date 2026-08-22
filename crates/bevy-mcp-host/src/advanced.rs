use std::fs;
use std::path::{Path, PathBuf};

use bevy::camera::RenderTarget;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::schedule::{Schedule, Schedules};
use bevy::prelude::*;
use bevy::reflect::serde::ReflectSerializer;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy_mcp_core::advanced::{
    AdvancedEntityQuery, AdvancedRequest, CaptureOptions, CaptureRect, QueryCondition,
    decode_advanced_request,
};
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};
use serde_json::{Value, json};

use crate::agent_api::{
    McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemAccessRegistry,
    McpSystemAccessSpec, McpSystemTimings,
};
use crate::change_tracking::{WorldChangeTracker, component_name_matches};
use crate::checkpoint::{McpRecorder, RecordedAction};
use crate::entity_handle::{entity_to_uri, resolve_entity};
use crate::permissions::{McpPermissions, PermissionLevel};
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;

/// Intercepts high-value agent operations before the legacy MCP dispatcher.
/// Ordinary commands are re-queued unchanged for the existing ingress system.
pub fn advanced_ingress_system(world: &mut World) {
    let entries = world.resource::<McpIngressQueue>().drain();

    for entry in entries {
        let request_id = entry.request_id;
        match entry.command {
            McpCommand::CaptureGame | McpCommand::CaptureCamera => {
                let options = CaptureOptions {
                    camera: None,
                    crop: None,
                    ui_only: false,
                    name: None,
                };
                handle_advanced_request(world, request_id, AdvancedRequest::Capture { options });
            }
            McpCommand::OperationStatus {
                operation_id: Some(operation_id),
            } => match decode_advanced_request(&operation_id) {
                Some(Ok(request)) => handle_advanced_request(world, request_id, request),
                Some(Err(error)) => push_error(
                    world,
                    request_id,
                    "INVALID_ADVANCED_REQUEST",
                    error.to_string(),
                ),
                None => world.resource::<McpIngressQueue>().push(
                    request_id,
                    McpCommand::OperationStatus {
                        operation_id: Some(operation_id),
                    },
                ),
            },
            command => world
                .resource::<McpIngressQueue>()
                .push(request_id, command),
        }
    }
}

fn handle_advanced_request(world: &mut World, request_id: u64, request: AdvancedRequest) {
    let permissions = world.resource::<McpPermissions>().clone();
    if !advanced_request_allowed(&request, &permissions) {
        push_error(
            world,
            request_id,
            "PERMISSION_DENIED",
            "The configured MCP permissions do not allow this operation",
        );
        return;
    }

    match request {
        AdvancedRequest::Capture { options } => start_capture(world, request_id, options),
        AdvancedRequest::ChangesSince { frame } => push_result(
            world,
            request_id,
            tracker_result(world, |tracker| tracker.changes_since(frame)),
        ),
        AdvancedRequest::EntityChanges { frame, entity } => {
            let resolved = match entity.as_ref() {
                Some(handle) => match resolve_entity(world, handle) {
                    Some(entity) => Some(entity),
                    None => {
                        push_error(
                            world,
                            request_id,
                            "ENTITY_NOT_FOUND",
                            format!("Entity {handle} not found"),
                        );
                        return;
                    }
                },
                None => None,
            };
            push_result(
                world,
                request_id,
                tracker_result(world, |tracker| {
                    tracker.entity_changes_since(frame, resolved)
                }),
            );
        }
        AdvancedRequest::ComponentChanges { frame, component } => push_result(
            world,
            request_id,
            tracker_result(world, |tracker| {
                tracker.component_changes_since(frame, component.as_deref())
            }),
        ),
        AdvancedRequest::ResourceChanges { frame, resource } => push_result(
            world,
            request_id,
            tracker_result(world, |tracker| {
                tracker.resource_changes_since(frame, resource.as_deref())
            }),
        ),
        AdvancedRequest::ScheduleList => push_result(world, request_id, schedule_list(world)),
        AdvancedRequest::ScheduleInspect { schedule } => {
            push_result(world, request_id, schedule_inspect(world, &schedule))
        }
        AdvancedRequest::SystemList { schedule } => {
            push_result(world, request_id, system_list(world, schedule.as_deref()))
        }
        AdvancedRequest::SystemInspect { system, schedule } => push_result(
            world,
            request_id,
            system_inspect(world, &system, schedule.as_deref()),
        ),
        AdvancedRequest::SystemAccess { system, schedule } => push_result(
            world,
            request_id,
            system_access(world, &system, schedule.as_deref()),
        ),
        AdvancedRequest::ComponentWriters {
            component,
            schedule,
        } => push_result(
            world,
            request_id,
            writers_for(world, &component, schedule.as_deref(), "component"),
        ),
        AdvancedRequest::ResourceWriters { resource, schedule } => push_result(
            world,
            request_id,
            writers_for(world, &resource, schedule.as_deref(), "resource"),
        ),
        AdvancedRequest::TrackingConfig {
            mode,
            history_frames,
            components,
            resources,
            exclude_components,
            exclude_resources,
        } => {
            let result = world.resource_mut::<WorldChangeTracker>().configure(
                mode.as_deref(),
                history_frames,
                components,
                resources,
                exclude_components,
                exclude_resources,
            );
            push_result(
                world,
                request_id,
                result
                    .map(McpResult::success)
                    .unwrap_or_else(|e| McpResult::error("INVALID_TRACKING_CONFIG", e)),
            );
        }
        AdvancedRequest::TrackingStatus => {
            push_result(
                world,
                request_id,
                McpResult::success(world.resource::<WorldChangeTracker>().status_json()),
            );
        }
        AdvancedRequest::SystemTimings { schedule } => push_result(
            world,
            request_id,
            system_timings(world, schedule.as_deref()),
        ),
        AdvancedRequest::StateGet { state } => {
            push_result(world, request_id, state_get(world, state.as_deref()))
        }
        AdvancedRequest::StateTransition { state, value } => {
            let recorded_value = value.clone();
            let result = world.resource_scope(|world, registry: Mut<McpStateRegistry>| {
                registry.set(&state, world, value)
            });
            if result.is_ok() {
                let frame = world
                    .get_resource::<McpRegistry>()
                    .map(|r| r.frame)
                    .unwrap_or_default();
                world.resource_mut::<McpRecorder>().record(
                    frame,
                    RecordedAction::StateTransition {
                        state: state.clone(),
                        value: recorded_value,
                    },
                );
            }
            push_result(
                world,
                request_id,
                result
                    .map(McpResult::success)
                    .unwrap_or_else(|message| McpResult::error("STATE_TRANSITION_FAILED", message)),
            );
        }
        AdvancedRequest::EntityQuery { query } => {
            push_result(world, request_id, advanced_entity_query(world, &query))
        }
        AdvancedRequest::SemanticActionList => {
            push_result(world, request_id, semantic_action_list(world))
        }
        AdvancedRequest::SemanticActionInvoke { action, args } => {
            let recorded_args = args.clone();
            let result = world.resource_scope(|world, registry: Mut<McpActionRegistry>| {
                registry.invoke(&action, world, args)
            });
            if result.is_ok() {
                let frame = world
                    .get_resource::<McpRegistry>()
                    .map(|r| r.frame)
                    .unwrap_or_default();
                world.resource_mut::<McpRecorder>().record(
                    frame,
                    RecordedAction::SemanticAction {
                        action: action.clone(),
                        args: recorded_args,
                    },
                );
            }
            push_result(
                world,
                request_id,
                result
                    .map(|value| McpResult::success(json!({ "action": action, "result": value })))
                    .unwrap_or_else(|message| McpResult::error("ACTION_FAILED", message)),
            );
        }
    }
}

fn advanced_request_allowed(request: &AdvancedRequest, permissions: &McpPermissions) -> bool {
    match request {
        AdvancedRequest::StateTransition { .. } | AdvancedRequest::SemanticActionInvoke { .. } => {
            permissions.can_mutate()
        }
        _ => permissions.level != PermissionLevel::None,
    }
}

fn tracker_result<F>(world: &World, callback: F) -> McpResult
where
    F: FnOnce(&WorldChangeTracker) -> Value,
{
    match world.get_resource::<WorldChangeTracker>() {
        Some(tracker) => McpResult::success(callback(tracker)),
        None => McpResult::error(
            "CHANGE_TRACKING_NOT_AVAILABLE",
            "WorldChangeTracker is not installed; add BevyMcpPlugin",
        ),
    }
}

fn start_capture(world: &mut World, request_id: u64, options: CaptureOptions) {
    let screenshot = if options.ui_only {
        let Some(target) = world
            .get_resource::<McpCaptureTargets>()
            .and_then(McpCaptureTargets::ui_target)
        else {
            push_error(
                world,
                request_id,
                "UI_CAPTURE_TARGET_NOT_CONFIGURED",
                "UI-only capture requires App::set_mcp_ui_capture_target with a dedicated UI render target",
            );
            return;
        };
        Screenshot::image(target)
    } else if let Some(camera_handle) = options.camera.as_ref() {
        let Some(camera_entity) = resolve_entity(world, camera_handle) else {
            push_error(
                world,
                request_id,
                "ENTITY_NOT_FOUND",
                format!("Camera entity {camera_handle} not found"),
            );
            return;
        };
        let Some(target) = world.get::<RenderTarget>(camera_entity).cloned() else {
            push_error(
                world,
                request_id,
                "CAMERA_TARGET_NOT_FOUND",
                format!("Entity {camera_handle} does not have a RenderTarget component"),
            );
            return;
        };
        Screenshot(target)
    } else {
        Screenshot::primary_window()
    };

    let frame = world
        .get_resource::<McpRegistry>()
        .map(|registry| registry.frame)
        .unwrap_or_default();
    let capture_dir = PathBuf::from(".bevy-mcp").join("captures");
    if let Err(error) = fs::create_dir_all(&capture_dir) {
        push_error(
            world,
            request_id,
            "CAPTURE_DIRECTORY_FAILED",
            error.to_string(),
        );
        return;
    }

    let filename = capture_filename(options.name.as_deref(), frame, request_id);
    let path = capture_dir.join(filename);
    let response_path = path.clone();
    let crop = options.crop.clone();
    let ui_only = options.ui_only;
    let camera = options.camera.as_ref().map(ToString::to_string);

    world.spawn(screenshot).observe(
        move |captured: On<ScreenshotCaptured>, results: Res<McpResultQueue>| {
            let result = save_capture(&captured.image, &response_path, crop.as_ref())
                .map(|(width, height, output_width, output_height)| {
                    let absolute =
                        fs::canonicalize(&response_path).unwrap_or_else(|_| response_path.clone());
                    McpResult::success(json!({
                        "path": response_path.to_string_lossy(),
                        "absolute_path": absolute.to_string_lossy(),
                        "width": width,
                        "height": height,
                        "output_width": output_width,
                        "output_height": output_height,
                        "crop": crop,
                        "ui_only": ui_only,
                        "camera": camera,
                    }))
                })
                .unwrap_or_else(|message| McpResult::error("CAPTURE_FAILED", message));
            results.push(McpResponse { request_id, result });
        },
    );
}

fn save_capture(
    image: &Image,
    path: &Path,
    crop: Option<&CaptureRect>,
) -> Result<(u32, u32, u32, u32), String> {
    let width = image.width();
    let height = image.height();
    let dynamic = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| format!("Could not convert captured image: {error:?}"))?;

    let output = if let Some(crop) = crop {
        if crop.width == 0 || crop.height == 0 || crop.x >= width || crop.y >= height {
            return Err(format!(
                "Invalid crop rectangle x={}, y={}, width={}, height={} for {}x{} capture",
                crop.x, crop.y, crop.width, crop.height, width, height
            ));
        }
        let crop_width = crop.width.min(width - crop.x);
        let crop_height = crop.height.min(height - crop.y);
        dynamic.crop_imm(crop.x, crop.y, crop_width, crop_height)
    } else {
        dynamic
    };

    let output_width = output.width();
    let output_height = output.height();
    output
        .to_rgb8()
        .save(path)
        .map_err(|error| format!("Could not save capture to {}: {error}", path.display()))?;
    Ok((width, height, output_width, output_height))
}

fn capture_filename(name: Option<&str>, frame: u64, request_id: u64) -> String {
    let stem = name
        .map(sanitize_filename)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("frame-{frame}-request-{request_id}"));
    format!("{stem}.png")
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .take(96)
        .collect()
}

fn schedule_list(world: &World) -> McpResult {
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return McpResult::error(
            "SCHEDULES_NOT_AVAILABLE",
            "Schedules resource is not available",
        );
    };
    let mut rows: Vec<Value> = schedules
        .iter()
        .map(|(label, schedule)| {
            json!({
                "name": format!("{label:?}"),
                "systems": schedule.systems_len(),
                "initialized": schedule.systems().is_ok(),
                "changed_since_build": schedule.is_changed(),
            })
        })
        .collect();
    rows.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    McpResult::success(json!({ "schedules": rows }))
}

fn schedule_inspect(world: &World, requested: &str) -> McpResult {
    let Some((label, schedule)) = find_schedule(world, requested) else {
        return McpResult::error(
            "SCHEDULE_NOT_FOUND",
            format!("Schedule '{requested}' not found"),
        );
    };
    let systems = schedule_system_rows(schedule);
    let conflicts: Vec<Value> = schedule
        .graph()
        .conflicting_systems()
        .to_string(schedule.graph(), world.components())
        .map(|(left, right, components)| {
            json!({
                "left": left,
                "right": right,
                "components": components.iter().map(ToString::to_string).collect::<Vec<_>>(),
            })
        })
        .collect();
    McpResult::success(json!({
        "name": format!("{label:?}"),
        "systems": systems,
        "system_count": schedule.systems_len(),
        "initialized": schedule.systems().is_ok(),
        "changed_since_build": schedule.is_changed(),
        "conflicts": conflicts,
    }))
}

fn system_list(world: &World, schedule_filter: Option<&str>) -> McpResult {
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return McpResult::error(
            "SCHEDULES_NOT_AVAILABLE",
            "Schedules resource is not available",
        );
    };
    let mut rows = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter)) {
            continue;
        }
        for mut row in schedule_system_rows(schedule) {
            row["schedule"] = Value::String(schedule_name.clone());
            rows.push(row);
        }
    }
    McpResult::success(json!({ "systems": rows }))
}

fn system_inspect(
    world: &World,
    requested_system: &str,
    schedule_filter: Option<&str>,
) -> McpResult {
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return McpResult::error(
            "SCHEDULES_NOT_AVAILABLE",
            "Schedules resource is not available",
        );
    };
    let mut matches = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter)) {
            continue;
        }
        for mut row in schedule_system_rows(schedule) {
            let name = row["name"].as_str().unwrap_or_default();
            if system_name_matches(name, requested_system) {
                row["schedule"] = Value::String(schedule_name.clone());
                matches.push(row);
            }
        }
    }
    if matches.is_empty() {
        McpResult::error(
            "SYSTEM_NOT_FOUND",
            format!("System '{requested_system}' not found"),
        )
    } else {
        McpResult::success(json!({ "matches": matches }))
    }
}

fn schedule_system_rows(schedule: &Schedule) -> Vec<Value> {
    match schedule.systems() {
        Ok(systems) => systems
            .map(|(key, system)| {
                let run_condition_count = schedule
                    .graph()
                    .systems
                    .get_conditions(key)
                    .map(|conditions| conditions.len())
                    .unwrap_or_default();
                json!({
                    "name": system.name().to_string(),
                    "key": format!("{key:?}"),
                    "is_send": system.is_send(),
                    "is_exclusive": system.is_exclusive(),
                    "has_deferred": system.has_deferred(),
                    "last_run_tick": system.get_last_run().get(),
                    "run_condition_count": run_condition_count,
                })
            })
            .collect(),
        Err(_) => schedule
            .graph()
            .systems
            .iter()
            .map(|(key, system, conditions)| {
                json!({
                    "name": system.name().to_string(),
                    "key": format!("{key:?}"),
                    "is_send": system.is_send(),
                    "is_exclusive": system.is_exclusive(),
                    "has_deferred": system.has_deferred(),
                    "last_run_tick": system.get_last_run().get(),
                    "run_condition_count": conditions.len(),
                })
            })
            .collect(),
    }
}

fn system_access(
    world: &World,
    requested_system: &str,
    schedule_filter: Option<&str>,
) -> McpResult {
    let registered = world
        .get_resource::<McpSystemAccessRegistry>()
        .map(|registry| {
            registry
                .iter()
                .filter(|entry| registered_system_matches(entry, requested_system, schedule_filter))
                .map(McpSystemAccessSpec::as_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let Some(schedules) = world.get_resource::<Schedules>() else {
        if registered.is_empty() {
            return McpResult::error(
                "SCHEDULES_NOT_AVAILABLE",
                "Schedules resource is not available",
            );
        }
        return McpResult::success(json!({
            "matches": registered,
            "coverage": "registered_exact",
            "note": "Exact access is game-registered; Bevy schedule conflict fallback was unavailable.",
        }));
    };

    let mut runtime_matches = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter)) {
            continue;
        }
        let Ok(systems) = schedule.systems() else {
            continue;
        };
        for (_, system) in systems {
            let system_name = system.name().to_string();
            if !system_name_matches(&system_name, requested_system) {
                continue;
            }
            let exact = world
                .get_resource::<McpSystemAccessRegistry>()
                .and_then(|registry| {
                    registry.iter().find(|entry| {
                        registered_system_matches(entry, &system_name, Some(&schedule_name))
                    })
                })
                .map(McpSystemAccessSpec::as_json);
            runtime_matches.push(json!({
                "system": system_name,
                "schedule": schedule_name,
                "exact_access": exact,
                "conflicts": conflict_rows_for_system(world, schedule, requested_system),
                "coverage": if exact.is_some() { "registered_exact" } else { "conflict_only" },
            }));
        }
    }

    if runtime_matches.is_empty() && registered.is_empty() {
        McpResult::error(
            "SYSTEM_NOT_FOUND",
            format!("System '{requested_system}' not found or schedule is not initialized"),
        )
    } else {
        McpResult::success(json!({
            "matches": runtime_matches,
            "registered": registered,
            "note": "Bevy 0.19 does not publicly expose stored per-system access sets. exact_access is present for game-registered systems; conflicts are automatic public-API evidence and do not identify which side performed a write.",
        }))
    }
}

fn registered_system_matches(
    entry: &McpSystemAccessSpec,
    requested_system: &str,
    schedule_filter: Option<&str>,
) -> bool {
    system_name_matches(&entry.system, requested_system)
        && schedule_filter.is_none_or(|filter| {
            entry
                .schedule
                .as_deref()
                .is_some_and(|schedule| schedule_name_matches(schedule, filter))
        })
}

fn conflict_rows_for_system(
    world: &World,
    schedule: &Schedule,
    requested_system: &str,
) -> Vec<Value> {
    schedule
        .graph()
        .conflicting_systems()
        .to_string(schedule.graph(), world.components())
        .filter_map(|(left, right, components)| {
            let left_matches = system_name_matches(&left, requested_system);
            let right_matches = system_name_matches(&right, requested_system);
            if !left_matches && !right_matches {
                return None;
            }
            Some(json!({
                "other_system": if left_matches { right } else { left },
                "components": components.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }))
        })
        .collect()
}

fn writers_for(
    world: &World,
    requested: &str,
    schedule_filter: Option<&str>,
    requested_kind: &str,
) -> McpResult {
    let Some(info) = world
        .components()
        .iter_registered()
        .find(|info| component_name_matches(&info.name().to_string(), requested))
    else {
        return McpResult::error(
            "TYPE_NOT_REGISTERED",
            format!("'{requested}' is not registered in this world"),
        );
    };
    let canonical = info.name().to_string();
    let is_resource = world.contains_resource_by_id(info.id());

    let exact_writers = world
        .get_resource::<McpSystemAccessRegistry>()
        .map(|registry| {
            registry
                .iter()
                .filter(|entry| {
                    if schedule_filter.is_some_and(|filter| {
                        entry
                            .schedule
                            .as_deref()
                            .is_none_or(|schedule| !schedule_name_matches(schedule, filter))
                    }) {
                        return false;
                    }
                    let writes = if is_resource {
                        &entry.resource_writes
                    } else {
                        &entry.writes
                    };
                    entry.write_all
                        || writes
                            .iter()
                            .any(|name| component_name_matches(name, &canonical))
                })
                .map(|entry| {
                    json!({
                        "system": entry.system,
                        "schedule": entry.schedule,
                        "confidence": "registered_exact",
                        "write_all": entry.write_all,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut conflict_candidates = Vec::new();
    if let Some(schedules) = world.get_resource::<Schedules>() {
        for (label, schedule) in schedules.iter() {
            let schedule_name = format!("{label:?}");
            if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter))
            {
                continue;
            }
            for (left, right, components) in schedule
                .graph()
                .conflicting_systems()
                .to_string(schedule.graph(), world.components())
            {
                let touches_target = components
                    .iter()
                    .map(ToString::to_string)
                    .any(|name| component_name_matches(&name, &canonical));
                if !touches_target {
                    continue;
                }
                for system in [left, right] {
                    if !conflict_candidates.iter().any(|row: &Value| {
                        row["system"].as_str() == Some(system.as_str())
                            && row["schedule"].as_str() == Some(schedule_name.as_str())
                    }) {
                        conflict_candidates.push(json!({
                            "system": system,
                            "schedule": schedule_name,
                            "confidence": "conflict_candidate",
                        }));
                    }
                }
            }
        }
    }

    McpResult::success(json!({
        "requested": requested,
        "canonical": canonical,
        "kind": if is_resource { "resource" } else { requested_kind },
        "writers": exact_writers,
        "conflict_candidates": conflict_candidates,
        "count": exact_writers.len(),
        "coverage": "registered_exact_plus_conflict_fallback",
        "note": "writers contains exact opt-in declarations. conflict_candidates is automatic Bevy conflict evidence: either side may be the writer, and a sole writer with no conflicting system will not appear there.",
    }))
}

fn system_timings(world: &World, schedule_filter: Option<&str>) -> McpResult {
    let Some(timings) = world.get_resource::<McpSystemTimings>() else {
        return McpResult::error(
            "SYSTEM_TIMINGS_NOT_AVAILABLE",
            "McpSystemTimings resource is not available",
        );
    };
    let mut rows = Vec::new();
    for (name, timing) in timings.iter() {
        if schedule_filter.is_some_and(|filter| !name.contains(filter)) {
            continue;
        }
        rows.push(json!({
            "system": name,
            "timing": timing.as_json(),
        }));
    }
    rows.sort_by(|left, right| {
        right["timing"]["recent_average_ns"]
            .as_u64()
            .cmp(&left["timing"]["recent_average_ns"].as_u64())
    });
    McpResult::success(json!({
        "timings": rows,
        "instrumentation": "explicit",
        "note": "Bevy does not expose executor wall-clock timings for every initialized system. Register timings through McpSystemTimings::record for systems you want profiled.",
    }))
}

fn state_get(world: &World, requested: Option<&str>) -> McpResult {
    let Some(states) = world.get_resource::<McpStateRegistry>() else {
        return McpResult::error(
            "STATE_REGISTRY_NOT_AVAILABLE",
            "McpStateRegistry is not available",
        );
    };
    match requested {
        Some(name) => states
            .get(name, world)
            .map(|value| McpResult::success(json!({ "state": name, "value": value })))
            .unwrap_or_else(|message| McpResult::error("STATE_NOT_FOUND", message)),
        None => McpResult::success(json!({ "states": states.list(world) })),
    }
}

fn semantic_action_list(world: &World) -> McpResult {
    let Some(actions) = world.get_resource::<McpActionRegistry>() else {
        return McpResult::error(
            "ACTION_REGISTRY_NOT_AVAILABLE",
            "McpActionRegistry is not available",
        );
    };
    let mut rows: Vec<Value> = actions
        .names()
        .map(|(name, action)| json!({ "name": name, "description": action.description }))
        .collect();
    rows.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    McpResult::success(json!({ "actions": rows }))
}

fn advanced_entity_query(world: &World, query: &AdvancedEntityQuery) -> McpResult {
    let with_ids = match resolve_component_ids(world, &query.with_components) {
        Ok(ids) => ids,
        Err(result) => return result,
    };
    let without_ids = match resolve_component_ids(world, &query.without_components) {
        Ok(ids) => ids,
        Err(result) => return result,
    };
    let parent_ids = match resolve_component_ids(world, &query.parent_has) {
        Ok(ids) => ids,
        Err(result) => return result,
    };
    let child_ids = match resolve_component_ids(world, &query.child_has) {
        Ok(ids) => ids,
        Err(result) => return result,
    };

    let tracker = world.get_resource::<WorldChangeTracker>();
    let limit = if query.limit == 0 {
        100
    } else {
        query.limit.min(10_000)
    };
    let mut matches = Vec::new();

    for entity_ref in world.iter_entities() {
        if entity_ref.contains::<bevy::ecs::resource::IsResource>() {
            continue;
        }
        if !with_ids.iter().all(|id| entity_ref.contains_id(*id))
            || without_ids.iter().any(|id| entity_ref.contains_id(*id))
        {
            continue;
        }
        if query.name_contains.as_ref().is_some_and(|needle| {
            entity_ref.get::<Name>().is_none_or(|name| {
                !name
                    .as_str()
                    .to_lowercase()
                    .contains(&needle.to_lowercase())
            })
        }) {
            continue;
        }
        if !query.changed.iter().all(|component| {
            tracker.is_some_and(|tracker| {
                tracker.component_changed_last_frame(entity_ref.id(), component)
            })
        }) {
            continue;
        }
        if !parent_matches(world, &entity_ref, &parent_ids)
            || !children_match(world, &entity_ref, &child_ids)
        {
            continue;
        }
        if !query.predicates.iter().all(|(path, condition)| {
            predicate_matches(world, &entity_ref, path, condition).unwrap_or(false)
        }) {
            continue;
        }

        let mut row = serde_json::Map::new();
        row.insert(
            "entity".into(),
            Value::String(entity_to_uri(entity_ref.id())),
        );
        if let Some(name) = entity_ref.get::<Name>() {
            row.insert("name".into(), Value::String(name.as_str().to_owned()));
        }
        if !query.include.is_empty() {
            let mut included = serde_json::Map::new();
            for component in &query.include {
                if let Ok(Some(value)) = reflected_component_json(world, &entity_ref, component) {
                    included.insert(component.clone(), value);
                }
            }
            row.insert("components".into(), Value::Object(included));
        }
        matches.push(Value::Object(row));
        if matches.len() >= limit as usize {
            break;
        }
    }

    McpResult::success(json!({
        "entities": matches,
        "count": matches.len(),
        "limit": limit,
        "change_frame": tracker.and_then(WorldChangeTracker::latest_frame),
    }))
}

fn parent_matches(
    world: &World,
    entity_ref: &EntityRef<'_>,
    required: &[bevy::ecs::component::ComponentId],
) -> bool {
    if required.is_empty() {
        return true;
    }
    let Some(parent) = entity_ref.get::<ChildOf>().map(ChildOf::parent) else {
        return false;
    };
    let Ok(parent_ref) = world.get_entity(parent) else {
        return false;
    };
    required.iter().all(|id| parent_ref.contains_id(*id))
}

fn children_match(
    world: &World,
    entity_ref: &EntityRef<'_>,
    required: &[bevy::ecs::component::ComponentId],
) -> bool {
    if required.is_empty() {
        return true;
    }
    let Some(children) = entity_ref.get::<Children>() else {
        return false;
    };
    required.iter().all(|required_id| {
        children.iter().any(|child| {
            world
                .get_entity(child)
                .is_ok_and(|child_ref| child_ref.contains_id(*required_id))
        })
    })
}

fn predicate_matches(
    world: &World,
    entity_ref: &EntityRef<'_>,
    path: &str,
    condition: &QueryCondition,
) -> Result<bool, String> {
    let (component, field_path) = path
        .split_once('.')
        .ok_or_else(|| format!("Predicate '{path}' must use Component.field notation"))?;
    let Some(value) = reflected_component_json(world, entity_ref, component)? else {
        return Ok(false);
    };
    let Some(actual) = json_path(&value, field_path) else {
        return Ok(false);
    };
    compare_json(actual, &condition.op, &condition.value)
}

fn json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn compare_json(actual: &Value, op: &str, expected: &Value) -> Result<bool, String> {
    match op {
        "eq" | "==" => Ok(actual == expected),
        "ne" | "!=" => Ok(actual != expected),
        "lt" | "<" => compare_numbers(actual, expected, |a, b| a < b),
        "lte" | "<=" => compare_numbers(actual, expected, |a, b| a <= b),
        "gt" | ">" => compare_numbers(actual, expected, |a, b| a > b),
        "gte" | ">=" => compare_numbers(actual, expected, |a, b| a >= b),
        "contains" => match (actual, expected) {
            (Value::String(actual), Value::String(expected)) => Ok(actual.contains(expected)),
            (Value::Array(values), expected) => Ok(values.contains(expected)),
            _ => Ok(false),
        },
        other => Err(format!("Unsupported predicate operator '{other}'")),
    }
}

fn compare_numbers<F>(actual: &Value, expected: &Value, compare: F) -> Result<bool, String>
where
    F: FnOnce(f64, f64) -> bool,
{
    let actual = actual
        .as_f64()
        .ok_or_else(|| format!("Predicate value {actual} is not numeric"))?;
    let expected = expected
        .as_f64()
        .ok_or_else(|| format!("Predicate value {expected} is not numeric"))?;
    Ok(compare(actual, expected))
}

fn reflected_component_json(
    world: &World,
    entity_ref: &EntityRef<'_>,
    requested: &str,
) -> Result<Option<Value>, String> {
    let app_registry = world
        .get_resource::<AppTypeRegistry>()
        .ok_or_else(|| "AppTypeRegistry is not available".to_string())?;
    let registry = app_registry.read();
    let Some(registration) = registry.iter().find(|registration| {
        let path = registration.type_info().type_path_table();
        path.short_path() == requested || path.path() == requested
    }) else {
        return Ok(None);
    };
    let Some(reflect_component) = registration.data::<bevy::ecs::reflect::ReflectComponent>()
    else {
        return Ok(None);
    };
    let Some(reflected) = reflect_component.reflect(*entity_ref) else {
        return Ok(None);
    };
    let serializer = ReflectSerializer::new(reflected.as_reflect(), &registry);
    let serialized = serde_json::to_value(&serializer).map_err(|error| error.to_string())?;
    Ok(Some(unwrap_reflect_value(serialized)))
}

fn unwrap_reflect_value(value: Value) -> Value {
    match value {
        Value::Object(map) if map.len() == 1 => map
            .into_iter()
            .next()
            .map(|(_, value)| value)
            .unwrap_or(Value::Null),
        value => value,
    }
}

fn resolve_component_ids(
    world: &World,
    names: &[String],
) -> Result<Vec<bevy::ecs::component::ComponentId>, McpResult> {
    let Some(app_registry) = world.get_resource::<AppTypeRegistry>() else {
        return Err(McpResult::error(
            "TYPE_REGISTRY_NOT_AVAILABLE",
            "AppTypeRegistry is not available",
        ));
    };
    let registry = app_registry.read();
    names
        .iter()
        .map(|name| {
            registry
                .iter()
                .find(|registration| {
                    let path = registration.type_info().type_path_table();
                    path.short_path() == name || path.path() == name
                })
                .and_then(|registration| world.components().get_id(registration.type_id()))
                .ok_or_else(|| {
                    McpResult::error(
                        "COMPONENT_NOT_FOUND",
                        format!("Component '{name}' is not registered"),
                    )
                })
        })
        .collect()
}

fn find_schedule<'a>(
    world: &'a World,
    requested: &str,
) -> Option<(&'a dyn bevy::ecs::schedule::ScheduleLabel, &'a Schedule)> {
    world
        .get_resource::<Schedules>()?
        .iter()
        .find(|(label, _)| schedule_name_matches(&format!("{label:?}"), requested))
}

fn schedule_name_matches(actual: &str, requested: &str) -> bool {
    actual == requested
        || actual.rsplit("::").next() == Some(requested)
        || requested.rsplit("::").next() == actual.rsplit("::").next()
}

fn system_name_matches(actual: &str, requested: &str) -> bool {
    actual == requested || actual.ends_with(requested) || actual.contains(requested)
}

fn push_result(world: &World, request_id: u64, result: McpResult) {
    world
        .resource::<McpResultQueue>()
        .push(McpResponse { request_id, result });
}

fn push_error(world: &World, request_id: u64, code: impl Into<String>, message: impl Into<String>) {
    push_result(world, request_id, McpResult::error(code, message));
}
