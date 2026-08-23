use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult, MutationOperation};
use bevy_mcp_core::entity_handle::EntityHandle;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};

#[derive(Component, Reflect, Debug, PartialEq)]
#[reflect(Component)]
struct Health {
    current: i32,
}

#[derive(Component, Reflect, Debug, PartialEq)]
#[reflect(Component)]
struct Tag {
    value: i32,
}

#[derive(Resource, Reflect, Debug, PartialEq)]
#[reflect(Resource)]
struct GameConfig {
    difficulty: i32,
}

fn handle(entity: Entity) -> EntityHandle {
    EntityHandle::from_uri(&format!(
        "entity://default/main/{}/{}",
        entity.index().index(),
        entity.generation()
    ))
    .unwrap()
}

fn setup() -> (App, McpIngressQueue, McpResultQueue, Entity) {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(McpPermissions::write()),
        )
        .register_type::<Health>()
        .register_type::<Tag>()
        .register_type::<GameConfig>();
    app.insert_resource(GameConfig { difficulty: 1 });
    let entity = app.world_mut().spawn(Health { current: 100 }).id();
    (app, ingress, results, entity)
}

fn result_for(results: &McpResultQueue, request_id: u64) -> McpResult {
    results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response")
        .result
}

#[test]
fn atomic_batch_commits_all_prevalidated_mutations() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        1,
        McpCommand::AtomicMutationBatch {
            operations: vec![
                MutationOperation::ComponentUpdate {
                    entity: handle(entity),
                    component: "Health".into(),
                    value: serde_json::json!({ "current": 75 }),
                },
                MutationOperation::ComponentInsert {
                    entity: handle(entity),
                    component: "Tag".into(),
                    value: serde_json::json!({ "value": 9 }),
                },
                MutationOperation::ResourceUpdate {
                    resource: "GameConfig".into(),
                    value: serde_json::json!({ "difficulty": 3 }),
                },
            ],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 1) else {
        panic!("expected transaction success");
    };
    assert_eq!(value["committed"], true);
    assert_eq!(value["operation_count"], 3);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 75);
    assert_eq!(app.world().get::<Tag>(entity).unwrap().value, 9);
    assert_eq!(app.world().resource::<GameConfig>().difficulty, 3);
}

#[test]
fn atomic_batch_validation_failure_leaves_earlier_operations_unapplied() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        2,
        McpCommand::AtomicMutationBatch {
            operations: vec![
                MutationOperation::ComponentUpdate {
                    entity: handle(entity),
                    component: "Health".into(),
                    value: serde_json::json!({ "current": 10 }),
                },
                MutationOperation::ResourceUpdate {
                    resource: "GameConfig".into(),
                    value: serde_json::json!({ "difficulty": "impossible" }),
                },
            ],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Error { code, message } = result_for(&results, 2) else {
        panic!("expected transaction validation failure");
    };
    assert_eq!(code, "TRANSACTION_VALIDATION_FAILED");
    assert!(message.contains("Operation 1"));
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 100);
    assert_eq!(app.world().resource::<GameConfig>().difficulty, 1);
}

#[test]
fn atomic_batch_dry_run_validates_without_committing() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        3,
        McpCommand::AtomicMutationBatch {
            operations: vec![MutationOperation::ComponentUpdate {
                entity: handle(entity),
                component: "Health".into(),
                value: serde_json::json!({ "current": 25 }),
            }],
            dry_run: true,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 3) else {
        panic!("expected dry-run validation success");
    };
    assert_eq!(value["validated"], true);
    assert_eq!(value["committed"], false);
    assert_eq!(value["mode"], "atomic_dry_run");
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 100);
}

#[test]
fn atomic_batch_can_remove_reflected_components() {
    let (mut app, ingress, results, entity) = setup();
    app.world_mut().entity_mut(entity).insert(Tag { value: 4 });
    ingress.push(
        4,
        McpCommand::AtomicMutationBatch {
            operations: vec![MutationOperation::ComponentRemove {
                entity: handle(entity),
                component: "Tag".into(),
            }],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 4) else {
        panic!("expected remove transaction success");
    };
    assert_eq!(value["committed"], true);
    assert!(app.world().get::<Tag>(entity).is_none());
}
