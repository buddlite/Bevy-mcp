use bevy::prelude::*;
use bevy_mcp_host::{McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore};
use serde::{Deserialize, Serialize};

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct SimSeed(u64);

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
