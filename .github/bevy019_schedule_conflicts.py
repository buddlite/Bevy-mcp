from pathlib import Path

advanced_path = Path("crates/bevy-mcp-host/src/advanced.rs")
text = advanced_path.read_text()

text = text.replace(
    "use std::fs;\nuse std::path::{Path, PathBuf};\n\nuse bevy::camera::RenderTarget;\nuse bevy::ecs::hierarchy::{ChildOf, Children};\nuse bevy::ecs::schedule::{Schedule, Schedules};",
    "use std::collections::HashMap;\nuse std::fs;\nuse std::path::{Path, PathBuf};\n\nuse bevy::camera::RenderTarget;\nuse bevy::ecs::hierarchy::{ChildOf, Children};\nuse bevy::ecs::schedule::{Schedule, Schedules, SystemKey};",
)

old = '''    let systems = schedule_system_rows(schedule);\n    let conflicts: Vec<Value> = schedule\n        .graph()\n        .conflicting_systems()\n        .to_string(schedule.graph(), world.components())\n        .map(|(left, right, components)| {\n            json!({\n                "left": left,\n                "right": right,\n                "components": components.iter().map(ToString::to_string).collect::<Vec<_>>(),\n            })\n        })\n        .collect();'''
new = '''    let systems = schedule_system_rows(schedule);\n    let conflicts: Vec<Value> = schedule_conflicts(world, schedule)\n        .into_iter()\n        .map(|(left, right, components)| {\n            json!({\n                "left": left,\n                "right": right,\n                "components": components,\n            })\n        })\n        .collect();'''
if old not in text:
    raise SystemExit("schedule_inspect conflict block not found")
text = text.replace(old, new)

old = '''fn schedule_system_rows(schedule: &Schedule) -> Vec<Value> {\n    match schedule.systems() {\n        Ok(systems) => systems\n            .map(|(key, system)| {\n                let run_condition_count = schedule\n                    .graph()\n                    .systems\n                    .get_conditions(key)\n                    .map(|conditions| conditions.len())\n                    .unwrap_or_default();\n                json!({\n                    "name": system.name().to_string(),\n                    "key": format!("{key:?}"),\n                    "is_send": system.is_send(),\n                    "is_exclusive": system.is_exclusive(),\n                    "has_deferred": system.has_deferred(),\n                    "last_run_tick": system.get_last_run().get(),\n                    "run_condition_count": run_condition_count,\n                })\n            })\n            .collect(),\n        Err(_) => schedule\n            .graph()\n            .systems\n            .iter()\n            .map(|(key, system, conditions)| {\n                json!({\n                    "name": system.name().to_string(),\n                    "key": format!("{key:?}"),\n                    "is_send": system.is_send(),\n                    "is_exclusive": system.is_exclusive(),\n                    "has_deferred": system.has_deferred(),\n                    "last_run_tick": system.get_last_run().get(),\n                    "run_condition_count": conditions.len(),\n                })\n            })\n            .collect(),\n    }\n}\n'''
new = '''fn schedule_system_rows(schedule: &Schedule) -> Vec<Value> {\n    match schedule.systems() {\n        Ok(systems) => systems\n            .map(|(key, system)| {\n                json!({\n                    "name": system.name().to_string(),\n                    "key": format!("{key:?}"),\n                    "is_send": system.is_send(),\n                    "is_exclusive": system.is_exclusive(),\n                    "has_deferred": system.has_deferred(),\n                    "last_run_tick": system.get_last_run().get(),\n                    // Bevy 0.19 moves run conditions out of ScheduleGraph when a schedule is built,\n                    // and does not expose them through Schedule::systems(). Do not report a false zero.\n                    "run_condition_count": Value::Null,\n                    "run_condition_count_available": false,\n                })\n            })\n            .collect(),\n        Err(_) => schedule\n            .graph()\n            .systems\n            .iter()\n            .map(|(key, system, conditions)| {\n                json!({\n                    "name": system.name().to_string(),\n                    "key": format!("{key:?}"),\n                    "is_send": system.is_send(),\n                    "is_exclusive": system.is_exclusive(),\n                    "has_deferred": system.has_deferred(),\n                    "last_run_tick": system.get_last_run().get(),\n                    "run_condition_count": conditions.len(),\n                    "run_condition_count_available": true,\n                })\n            })\n            .collect(),\n    }\n}\n\nfn schedule_system_name_map(schedule: &Schedule) -> HashMap<SystemKey, String> {\n    let mut names = HashMap::new();\n    if let Ok(systems) = schedule.systems() {\n        names.extend(systems.map(|(key, system)| (key, system.name().to_string())));\n    }\n    // Newly-added systems live in ScheduleGraph until the next build. Combining both stores\n    // also makes this robust while a previously-built schedule is marked changed.\n    for (key, system, _) in schedule.graph().systems.iter() {\n        names.entry(key).or_insert_with(|| system.name().to_string());\n    }\n    names\n}\n\nfn schedule_conflicts(\n    world: &World,\n    schedule: &Schedule,\n) -> Vec<(String, String, Vec<String>)> {\n    let names = schedule_system_name_map(schedule);\n    schedule\n        .graph()\n        .conflicting_systems()\n        .0\n        .iter()\n        .map(|(left, right, components)| {\n            let left_name = names\n                .get(left)\n                .cloned()\n                .unwrap_or_else(|| format!("{left:?}"));\n            let right_name = names\n                .get(right)\n                .cloned()\n                .unwrap_or_else(|| format!("{right:?}"));\n            let component_names = components\n                .iter()\n                .map(|component| {\n                    world\n                        .components()\n                        .get_name(*component)\n                        .map(|name| name.to_string())\n                        .unwrap_or_else(|| format!("{component:?}"))\n                })\n                .collect();\n            (left_name, right_name, component_names)\n        })\n        .collect()\n}\n'''
if old not in text:
    raise SystemExit("schedule_system_rows block not found")
text = text.replace(old, new)

old = '''fn conflict_rows_for_system(\n    world: &World,\n    schedule: &Schedule,\n    requested_system: &str,\n) -> Vec<Value> {\n    schedule\n        .graph()\n        .conflicting_systems()\n        .to_string(schedule.graph(), world.components())\n        .filter_map(|(left, right, components)| {\n            let left_matches = system_name_matches(&left, requested_system);\n            let right_matches = system_name_matches(&right, requested_system);\n            if !left_matches && !right_matches {\n                return None;\n            }\n            Some(json!({\n                "other_system": if left_matches { right } else { left },\n                "components": components.iter().map(ToString::to_string).collect::<Vec<_>>(),\n            }))\n        })\n        .collect()\n}\n'''
new = '''fn conflict_rows_for_system(\n    world: &World,\n    schedule: &Schedule,\n    requested_system: &str,\n) -> Vec<Value> {\n    schedule_conflicts(world, schedule)\n        .into_iter()\n        .filter_map(|(left, right, components)| {\n            let left_matches = system_name_matches(&left, requested_system);\n            let right_matches = system_name_matches(&right, requested_system);\n            if !left_matches && !right_matches {\n                return None;\n            }\n            Some(json!({\n                "other_system": if left_matches { right } else { left },\n                "components": components,\n            }))\n        })\n        .collect()\n}\n'''
if old not in text:
    raise SystemExit("conflict_rows_for_system block not found")
text = text.replace(old, new)

old = '''            for (left, right, components) in schedule\n                .graph()\n                .conflicting_systems()\n                .to_string(schedule.graph(), world.components())\n            {\n                let touches_target = components\n                    .iter()\n                    .map(ToString::to_string)\n                    .any(|name| component_name_matches(&name, &canonical));'''
new = '''            for (left, right, components) in schedule_conflicts(world, schedule) {\n                let touches_target = components\n                    .iter()\n                    .any(|name| component_name_matches(name, &canonical));'''
if old not in text:
    raise SystemExit("writers_for conflict block not found")
text = text.replace(old, new)

advanced_path.write_text(text)

test_path = Path("crates/bevy-mcp-host/tests/intelligence.rs")
test = test_path.read_text()
append = r'''

#[derive(Resource, Default)]
struct ConflictProbe;

fn read_conflict_probe(_probe: Res<ConflictProbe>) {}
fn write_conflict_probe(_probe: ResMut<ConflictProbe>) {}

#[test]
fn initialized_schedule_conflicts_use_executable_system_names_without_panicking() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::read_only()),
    );
    app.init_resource::<ConflictProbe>();
    app.add_systems(Update, (read_conflict_probe, write_conflict_probe));

    // Build and run Update once. Bevy 0.19 moves initialized systems out of ScheduleGraph
    // and into the executable schedule at this point.
    app.update();

    for (request_id, request) in [
        (
            80,
            AdvancedRequest::ScheduleInspect {
                schedule: "Update".into(),
            },
        ),
        (
            81,
            AdvancedRequest::SystemAccess {
                system: "read_conflict_probe".into(),
                schedule: Some("Update".into()),
            },
        ),
        (
            82,
            AdvancedRequest::ResourceWriters {
                resource: "ConflictProbe".into(),
                schedule: Some("Update".into()),
            },
        ),
    ] {
        let operation_id = encode_advanced_request(&request).unwrap();
        ingress.push(
            request_id,
            McpCommand::OperationStatus {
                operation_id: Some(operation_id),
            },
        );
    }

    app.update();
    let responses = results.drain();

    let schedule = responses
        .iter()
        .find(|response| response.request_id == 80)
        .expect("schedule response");
    let schedule = match &schedule.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    };
    let conflicts = schedule["conflicts"].as_array().unwrap();
    assert!(conflicts.iter().any(|conflict| {
        let left = conflict["left"].as_str().unwrap_or_default();
        let right = conflict["right"].as_str().unwrap_or_default();
        (left.contains("read_conflict_probe") && right.contains("write_conflict_probe"))
            || (left.contains("write_conflict_probe") && right.contains("read_conflict_probe"))
    }));

    let access = responses
        .iter()
        .find(|response| response.request_id == 81)
        .expect("system access response");
    let access = match &access.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    };
    assert!(access["matches"].as_array().unwrap().iter().any(|entry| {
        entry["conflicts"].as_array().is_some_and(|conflicts| {
            conflicts.iter().any(|conflict| {
                conflict["other_system"]
                    .as_str()
                    .is_some_and(|name| name.contains("write_conflict_probe"))
            })
        })
    }));

    let writers = responses
        .iter()
        .find(|response| response.request_id == 82)
        .expect("resource writers response");
    let writers = match &writers.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    };
    assert!(writers["conflict_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| candidate["system"]
            .as_str()
            .is_some_and(|name| name.contains("write_conflict_probe"))));
}
'''
if "initialized_schedule_conflicts_use_executable_system_names_without_panicking" in test:
    raise SystemExit("schedule conflict regression test already present")
test_path.write_text(test + append)

print("Applied safe Bevy 0.19 schedule conflict introspection fix")
