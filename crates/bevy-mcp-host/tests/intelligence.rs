use bevy::prelude::*;
use bevy_mcp_core::advanced::{AdvancedRequest, encode_advanced_request};
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::change_tracking::WorldChangeTracker;
use bevy_mcp_host::{
    BevyMcpPlugin, McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpPermissions,
    McpRecorder, McpSystemAccessRegistry, McpSystemAccessSpec, RecordedAction,
};
use serde::{Deserialize, Serialize};

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct SimSeed(u64);

#[derive(Component)]
struct Health;

#[derive(Resource)]
struct CombatStats;

#[test]
fn checkpoint_resource_round_trip_is_deterministic() {
    let mut app = App::new();
    app.insert_resource(SimSeed(42));
    app.register_mcp_checkpoint_resource::<SimSeed>("sim_seed", "deterministic simulation seed");

    let values = app
        .world()
        .resource::<McpCheckpointRegistry>()
        .capture(app.world())
        .unwrap();
    app.world_mut().resource_mut::<SimSeed>().0 = 99;
    app.world_mut()
        .resource_scope(|world, registry: Mut<McpCheckpointRegistry>| {
            registry.restore(world, &values)
        })
        .unwrap();
    assert_eq!(app.world().resource::<SimSeed>().0, 42);

    app.world_mut().init_resource::<McpCheckpointStore>();
}

#[test]
fn system_access_registry_records_exact_typed_writes() {
    let mut app = App::new();
    app.register_mcp_system_access(
        McpSystemAccessSpec::new("combat::apply_damage")
            .schedule("Update")
            .write::<Health>()
            .write_resource::<CombatStats>(),
    );

    let registry = app.world().resource::<McpSystemAccessRegistry>();
    let spec = registry.iter().next().expect("registered system access");
    assert_eq!(spec.system, "combat::apply_damage");
    assert_eq!(spec.schedule.as_deref(), Some("Update"));
    assert!(spec.writes.iter().any(|name| name.ends_with("Health")));
    assert!(
        spec.resource_writes
            .iter()
            .any(|name| name.ends_with("CombatStats"))
    );
}

#[test]
fn scoped_tracking_reports_static_and_dynamic_interests() {
    let mut tracker = WorldChangeTracker::default();
    tracker
        .configure(
            Some("scoped"),
            Some(300),
            Some(vec!["Health".into()]),
            Some(vec!["Economy".into()]),
            Some(vec!["Transform".into()]),
            None,
        )
        .unwrap();
    tracker.add_dynamic_interests(vec!["Cargo".into()], vec!["SimulationClock".into()]);

    let status = tracker.status_json();
    assert_eq!(status["mode"], "scoped");
    assert_eq!(status["history_frames"], 300);
    assert!(
        status["components"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Health"))
    );
    assert!(
        status["dynamic_components"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Cargo"))
    );
}

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct RestoreFirst(i32);

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct RestoreSecond(i32);

#[test]
fn scoped_dynamic_interests_replace_stale_subscriptions() {
    let mut tracker = WorldChangeTracker::default();
    tracker
        .configure(Some("scoped"), None, None, None, None, None)
        .unwrap();
    tracker.add_dynamic_interests(vec!["Cargo".into()], vec!["Clock".into()]);
    tracker.set_dynamic_interests(vec!["Health".into()], Vec::<String>::new());

    let status = tracker.status_json();
    let components = status["dynamic_components"].as_array().unwrap();
    assert!(components.contains(&serde_json::json!("Health")));
    assert!(!components.contains(&serde_json::json!("Cargo")));
    assert!(status["dynamic_resources"].as_array().unwrap().is_empty());
}

#[test]
fn checkpoint_restore_rolls_back_partial_failure() {
    let mut app = App::new();
    app.insert_resource(RestoreFirst(1));
    app.insert_resource(RestoreSecond(2));
    app.register_mcp_checkpoint_resource::<RestoreFirst>("a_first", "first restore resource");
    app.world_mut()
        .resource_mut::<McpCheckpointRegistry>()
        .register_adapter(
            "b_second",
            "adapter that fails only for the requested target",
            |world| Ok(serde_json::json!(world.resource::<RestoreSecond>().0)),
            |world, value| {
                let value = value
                    .as_i64()
                    .ok_or_else(|| "RestoreSecond value must be an integer".to_string())?
                    as i32;
                world.resource_mut::<RestoreSecond>().0 = value;
                if value == 20 {
                    Err("intentional restore failure".into())
                } else {
                    Ok(())
                }
            },
        );

    let mut target = serde_json::Map::new();
    target.insert("a_first".into(), serde_json::json!(10));
    target.insert("b_second".into(), serde_json::json!(20));
    let error = app
        .world_mut()
        .resource_scope(|world, registry: Mut<McpCheckpointRegistry>| {
            registry.restore(world, &target)
        })
        .unwrap_err();

    assert!(error.contains("rolled back"));
    assert_eq!(app.world().resource::<RestoreFirst>(), &RestoreFirst(1));
    assert_eq!(app.world().resource::<RestoreSecond>(), &RestoreSecond(2));
}

#[test]
fn recordings_preserve_frame_offsets_for_replay() {
    let mut recorder = McpRecorder::default();
    let recording_id = recorder.start("route test".into(), 100).unwrap();
    recorder.record(
        103,
        RecordedAction::SemanticAction {
            action: "spawn_ship".into(),
            args: serde_json::json!({"id": 7}),
        },
    );
    recorder.record(
        111,
        RecordedAction::StateTransition {
            state: "phase".into(),
            value: serde_json::json!("running"),
        },
    );
    let recording = recorder.stop().unwrap();
    assert_eq!(recording.id, recording_id);
    assert_eq!(recording.events[0].offset_frames, 3);
    assert_eq!(recording.events[1].offset_frames, 11);

    assert!(recorder.validate_replay_start("missing-recording").is_err());
    assert!(recorder.validate_replay_start(&recording.id).is_ok());

    let replay_id = recorder
        .start_replay(recording.id.clone(), Some("checkpoint-1".into()), 500)
        .unwrap();
    assert!(recorder.validate_replay_start(&recording.id).is_err());
    let replay = recorder.replays.get(&replay_id).unwrap();
    assert_eq!(replay.start_frame, 500);
    assert_eq!(replay.next_event, 0);
    assert_eq!(replay.checkpoint_id.as_deref(), Some("checkpoint-1"));
}

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
    assert!(
        value["writers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|writer| { writer["system"].as_str() == Some("economy::write_dormant_stats") })
    );
}
