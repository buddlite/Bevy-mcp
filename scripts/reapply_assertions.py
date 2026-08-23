from pathlib import Path


def replace_between(path: str, start: str, end: str, replacement: str) -> None:
    file = Path(path)
    text = file.read_text()
    i = text.index(start)
    j = text.index(end, i)
    file.write_text(text[:i] + replacement + text[j:])


server = "crates/bevy-mcp-server/src/tools.rs"
replace_between(
    server,
    "#[derive(Debug, Deserialize, schemars::JsonSchema)]\npub struct AssertParams {",
    "\n#[derive(Debug, Deserialize, schemars::JsonSchema)]\npub struct OperationStatusParams {",
    '''#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssertParams {
    #[schemars(description = "Assertion type: 'entity_exists', 'component_exists', 'component_equals', 'resource_equals', or 'entity_count'")]
    pub assertion_type: String,
    #[schemars(description = "Entity ID for entity/component assertions")]
    pub entity_id: Option<u32>,
    #[schemars(description = "Component name for component assertions")]
    pub component: Option<String>,
    #[schemars(description = "Resource name for resource_equals")]
    pub resource: Option<String>,
    #[schemars(description = "Dot-separated reflected field path; array indices are supported")]
    pub field: Option<String>,
    #[schemars(description = "Expected JSON value for component_equals/resource_equals")]
    pub expected_value: Option<serde_json::Value>,
    #[schemars(description = "Expected entity count")]
    pub expected_count: Option<u32>,
}
''',
)

replace_between(
    server,
    '    #[tool(description = "Assert a condition about the game state")]\n',
    '    #[tool(description = "List installed Bevy plugins and their capabilities")]\n',
    '''    #[tool(description = "Assert entity, component, resource, or entity-count state")]
    async fn assert(&self, Parameters(params): Parameters<AssertParams>) -> String {
        let assertion_type = params.assertion_type;
        let assertion = match assertion_type.as_str() {
            "entity_exists" => bevy_mcp_core::command::Assertion::EntityExists {
                entity_id: match params.entity_id {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "entity_exists requires entity_id"),
                },
            },
            "component_exists" => bevy_mcp_core::command::Assertion::ComponentExists {
                entity_id: match params.entity_id {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "component_exists requires entity_id"),
                },
                component: match params.component {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "component_exists requires component"),
                },
            },
            "component_equals" => bevy_mcp_core::command::Assertion::ComponentEquals {
                entity_id: match params.entity_id {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "component_equals requires entity_id"),
                },
                component: match params.component {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "component_equals requires component"),
                },
                field: params.field.unwrap_or_default(),
                value: match params.expected_value {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "component_equals requires expected_value"),
                },
            },
            "resource_equals" => bevy_mcp_core::command::Assertion::ResourceEquals {
                resource: match params.resource {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "resource_equals requires resource"),
                },
                field: params.field.unwrap_or_default(),
                value: match params.expected_value {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "resource_equals requires expected_value"),
                },
            },
            "entity_count" => bevy_mcp_core::command::Assertion::EntityCount {
                expected: match params.expected_count {
                    Some(value) => value,
                    None => return error("MISSING_PARAMS", "entity_count requires expected_count"),
                },
            },
            _ => return error("INVALID_ASSERTION", format!("Unknown assertion type: {assertion_type}")),
        };
        self.state.call(McpCommand::Assert { assertion }).await
    }

''',
)

systems = "crates/bevy-mcp-host/src/systems.rs"
replace_between(
    systems,
    "fn assert_condition(world: &World, assertion: &bevy_mcp_core::command::Assertion) -> McpResult {",
    "\nfn list_plugins(world: &World) -> McpResult {",
    '''fn reflect_serialized_root(value: &Value) -> &Value {
    value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.values().next())
        .unwrap_or(value)
}

fn json_value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = reflect_serialized_root(value);
    if path.is_empty() {
        return Some(current);
    }
    for segment in path.split('.') {
        current = match current {
            Value::Object(object) => object.get(segment)?,
            Value::Array(array) => array.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn serialize_assert_component(world: &World, entity: Entity, component: &str) -> Result<Value, String> {
    let entity_ref = world.get_entity(entity).map_err(|_| "Entity not found".to_owned())?;
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();
    let registration = registry.iter().find(|registration| {
        let path = registration.type_info().type_path_table();
        path.short_path() == component || path.path() == component
    }).ok_or_else(|| format!("Component '{component}' is not registered"))?;
    let reflect_component = registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .ok_or_else(|| format!("Component '{component}' is not reflected"))?;
    let reflected = reflect_component
        .reflect(entity_ref)
        .ok_or_else(|| format!("Entity does not have component '{component}'"))?;
    let serializer = bevy::reflect::serde::ReflectSerializer::new(reflected.as_reflect(), &registry);
    serde_json::to_value(&serializer)
        .map_err(|error| format!("Failed to serialize component '{component}': {error}"))
}

fn serialize_assert_resource(world: &World, resource: &str) -> Result<Value, String> {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();
    let registration = registry.iter().find(|registration| {
        let path = registration.type_info().type_path_table();
        path.short_path() == resource || path.path() == resource
    }).ok_or_else(|| format!("Resource '{resource}' is not registered"))?;
    let type_id = registration.type_id();
    let reflect_from_ptr = registration
        .data::<bevy::reflect::ReflectFromPtr>()
        .ok_or_else(|| format!("Resource '{resource}' is not reflected"))?;
    for (info, ptr) in world.iter_resources() {
        if info.type_id() == Some(type_id) {
            let reflected = unsafe { reflect_from_ptr.as_reflect(ptr) };
            let serializer = bevy::reflect::serde::ReflectSerializer::new(reflected, &registry);
            return serde_json::to_value(&serializer)
                .map_err(|error| format!("Failed to serialize resource '{resource}': {error}"));
        }
    }
    Err(format!("Resource '{resource}' is not present"))
}

fn equality_assertion(assertion_name: &str, subject: Value, field: &str, expected: &Value, context: Value) -> McpResult {
    match json_value_at_path(&subject, field) {
        Some(actual) => McpResult::success(json!({
            "passed": actual == expected,
            "assertion": assertion_name,
            "field": field,
            "expected": expected,
            "actual": actual,
            "context": context,
        })),
        None => McpResult::success(json!({
            "passed": false,
            "assertion": assertion_name,
            "field": field,
            "expected": expected,
            "actual": Value::Null,
            "context": context,
            "error": format!("Field path '{field}' was not found"),
        })),
    }
}

fn assert_condition(world: &World, assertion: &bevy_mcp_core::command::Assertion) -> McpResult {
    use bevy_mcp_core::command::Assertion;
    match assertion {
        Assertion::EntityExists { entity_id } => {
            let exists = resolve_entity_by_index(world, *entity_id).is_some();
            McpResult::success(if exists {
                json!({"passed": true, "assertion": "entity_exists", "entity_id": entity_id})
            } else {
                json!({"passed": false, "assertion": "entity_exists", "entity_id": entity_id, "error": "Entity not found"})
            })
        }
        Assertion::ComponentExists { entity_id, component } => {
            let entity = match resolve_entity_by_index(world, *entity_id) {
                Some(entity) => entity,
                None => return McpResult::success(json!({"passed": false, "assertion": "component_exists", "error": "Entity not found"})),
            };
            let registry = world.resource::<AppTypeRegistry>().read();
            let has_component = registry.iter().any(|registration| {
                let path = registration.type_info().type_path_table();
                (path.short_path() == component || path.path() == component)
                    && registration.data::<bevy::ecs::reflect::ReflectComponent>()
                        .and_then(|rc| rc.reflect(world.get_entity(entity).ok()?)).is_some()
            });
            McpResult::success(json!({"passed": has_component, "assertion": "component_exists", "entity_id": entity_id, "component": component}))
        }
        Assertion::ComponentEquals { entity_id, component, field, value } => {
            let entity = match resolve_entity_by_index(world, *entity_id) {
                Some(entity) => entity,
                None => return McpResult::success(json!({"passed": false, "assertion": "component_equals", "entity_id": entity_id, "component": component, "field": field, "expected": value, "error": "Entity not found"})),
            };
            match serialize_assert_component(world, entity, component) {
                Ok(serialized) => equality_assertion("component_equals", serialized, field, value, json!({"entity_id": entity_id, "component": component})),
                Err(error) => McpResult::success(json!({"passed": false, "assertion": "component_equals", "entity_id": entity_id, "component": component, "field": field, "expected": value, "error": error})),
            }
        }
        Assertion::EntityCount { expected } => {
            let count = world.iter_entities().count() as u32;
            McpResult::success(json!({"passed": count == *expected, "assertion": "entity_count", "expected": expected, "actual": count}))
        }
        Assertion::ResourceEquals { resource, field, value } => match serialize_assert_resource(world, resource) {
            Ok(serialized) => equality_assertion("resource_equals", serialized, field, value, json!({"resource": resource})),
            Err(error) => McpResult::success(json!({"passed": false, "assertion": "resource_equals", "resource": resource, "field": field, "expected": value, "error": error})),
        },
    }
}
''',
)

Path("crates/bevy-mcp-host/tests/assertions.rs").write_text(r'''use bevy::prelude::*;
use bevy_mcp_core::command::{Assertion, McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::{Value, json};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Vitals { health: i32, shield: i32 }

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct MatchState { wave: u32, active: bool }

fn assert_result(app: &mut App, ingress: &McpIngressQueue, results: &McpResultQueue, request_id: u64, assertion: Assertion) -> Value {
    ingress.push(request_id, McpCommand::Assert { assertion });
    app.update();
    let response = results.drain().into_iter().find(|response| response.request_id == request_id).expect("assertion response");
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
        .add_plugins(BevyMcpPlugin::new().with_queues(ingress.clone(), results.clone()).with_permissions(McpPermissions::read_only()))
        .insert_resource(MatchState { wave: 4, active: true });
    let entity = app.world_mut().spawn(Vitals { health: 75, shield: 25 }).id();
    (app, ingress, results, entity.index().index())
}

#[test]
fn component_equals_reports_actual_expected_and_missing_fields() {
    let (mut app, ingress, results, entity_id) = test_app();
    let passed = assert_result(&mut app, &ingress, &results, 1, Assertion::ComponentEquals { entity_id, component: "Vitals".into(), field: "health".into(), value: json!(75) });
    assert_eq!(passed["passed"], true);
    assert_eq!(passed["actual"], 75);
    let failed = assert_result(&mut app, &ingress, &results, 2, Assertion::ComponentEquals { entity_id, component: "Vitals".into(), field: "shield".into(), value: json!(99) });
    assert_eq!(failed["passed"], false);
    assert_eq!(failed["actual"], 25);
    let missing = assert_result(&mut app, &ingress, &results, 3, Assertion::ComponentEquals { entity_id, component: "Vitals".into(), field: "missing".into(), value: Value::Null });
    assert_eq!(missing["passed"], false);
    assert!(missing["error"].as_str().unwrap().contains("not found"));
}

#[test]
fn resource_equals_checks_reflected_resource_fields() {
    let (mut app, ingress, results, _) = test_app();
    let passed = assert_result(&mut app, &ingress, &results, 10, Assertion::ResourceEquals { resource: "MatchState".into(), field: "wave".into(), value: json!(4) });
    assert_eq!(passed["passed"], true);
    assert_eq!(passed["actual"], 4);
    let failed = assert_result(&mut app, &ingress, &results, 11, Assertion::ResourceEquals { resource: "MatchState".into(), field: "active".into(), value: json!(false) });
    assert_eq!(failed["passed"], false);
    assert_eq!(failed["actual"], true);
}
''')
