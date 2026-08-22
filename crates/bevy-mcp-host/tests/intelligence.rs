use bevy::prelude::*;
use bevy_mcp_host::{
    McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpSystemAccessRegistry,
    McpSystemAccessSpec,
};
use bevy_mcp_host::change_tracking::WorldChangeTracker;
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
    assert!(status["components"].as_array().unwrap().contains(&serde_json::json!("Health")));
    assert!(
        status["dynamic_components"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Cargo"))
    );
}
