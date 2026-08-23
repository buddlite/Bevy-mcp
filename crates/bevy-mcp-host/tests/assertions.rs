use bevy::prelude::*;
use bevy_mcp_core::command::{Assertion, McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::{Value, json};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Vitals {
    health: i32,
    shield: i32,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct MatchState {
    wave: u32,
    active: bool,
}

fn assert_result(
    app: &mut App,
    ingress: &McpIngressQueue,
    results: &McpResultQueue,
    request_id: u64,
    assertion: Assertion,
) -> Value {
    ingress.push(request_id, McpCommand::Assert { assertion });
    app.update();
    let response = results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("assertion response");
    match response.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    }
}

fn test_app() -> (App, McpIngressQueue, McpResultQueue, u32) {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .register_type::<Vitals>()
        .register_type::<MatchState>()
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(McpPermissions::read_only()),
        )
        .insert_resource(MatchState {
            wave: 4,
            active: true,
        });
    let entity = app
        .world_mut()
        .spawn(Vitals {
            health: 75,
            shield: 25,
        })
        .id();
    (app, ingress, results, entity.index().index())
}

#[test]
fn component_equals_reports_actual_expected_and_missing_fields() {
    let (mut app, ingress, results, entity_id) = test_app();
    let passed = assert_result(
        &mut app,
        &ingress,
        &results,
        1,
        Assertion::ComponentEquals {
            entity_id,
            component: "Vitals".into(),
            field: "health".into(),
            value: json!(75),
        },
    );
    assert_eq!(passed["passed"], true);
    assert_eq!(passed["actual"], 75);
    let failed = assert_result(
        &mut app,
        &ingress,
        &results,
        2,
        Assertion::ComponentEquals {
            entity_id,
            component: "Vitals".into(),
            field: "shield".into(),
            value: json!(99),
        },
    );
    assert_eq!(failed["passed"], false);
    assert_eq!(failed["actual"], 25);
    let missing = assert_result(
        &mut app,
        &ingress,
        &results,
        3,
        Assertion::ComponentEquals {
            entity_id,
            component: "Vitals".into(),
            field: "missing".into(),
            value: Value::Null,
        },
    );
    assert_eq!(missing["passed"], false);
    assert!(missing["error"].as_str().unwrap().contains("not found"));
}

#[test]
fn resource_equals_checks_reflected_resource_fields() {
    let (mut app, ingress, results, _) = test_app();
    let passed = assert_result(
        &mut app,
        &ingress,
        &results,
        10,
        Assertion::ResourceEquals {
            resource: "MatchState".into(),
            field: "wave".into(),
            value: json!(4),
        },
    );
    assert_eq!(passed["passed"], true);
    assert_eq!(passed["actual"], 4);
    let failed = assert_result(
        &mut app,
        &ingress,
        &results,
        11,
        Assertion::ResourceEquals {
            resource: "MatchState".into(),
            field: "active".into(),
            value: json!(false),
        },
    );
    assert_eq!(failed["passed"], false);
    assert_eq!(failed["actual"], true);
}
