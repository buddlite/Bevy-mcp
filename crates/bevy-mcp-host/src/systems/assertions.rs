use super::*;

pub(crate) fn playtest_run(
    _world: &World,
    _steps: &[bevy_mcp_core::command::PlaytestStep],
) -> McpResult {
    McpResult::error("NOT_IMPLEMENTED", "Playtest execution is not implemented")
}

pub(crate) fn reflect_serialized_root(value: &Value) -> &Value {
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

pub(crate) fn serialize_assert_component(
    world: &World,
    entity: Entity,
    component: &str,
) -> Result<Value, String> {
    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| "Entity not found".to_owned())?;
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| {
            let path = registration.type_info().type_path_table();
            path.short_path() == component || path.path() == component
        })
        .ok_or_else(|| format!("Component '{component}' is not registered"))?;
    let reflect_component = registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .ok_or_else(|| format!("Component '{component}' is not reflected"))?;
    let reflected = reflect_component
        .reflect(entity_ref)
        .ok_or_else(|| format!("Entity does not have component '{component}'"))?;
    let serializer =
        bevy::reflect::serde::ReflectSerializer::new(reflected.as_reflect(), &registry);
    serde_json::to_value(&serializer)
        .map_err(|error| format!("Failed to serialize component '{component}': {error}"))
}

pub(crate) fn serialize_assert_resource(world: &World, resource: &str) -> Result<Value, String> {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| {
            let path = registration.type_info().type_path_table();
            path.short_path() == resource || path.path() == resource
        })
        .ok_or_else(|| format!("Resource '{resource}' is not registered"))?;
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

pub(crate) fn equality_assertion(
    assertion_name: &str,
    subject: Value,
    field: &str,
    expected: &Value,
    context: Value,
) -> McpResult {
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

pub(crate) fn assert_condition(
    world: &World,
    assertion: &bevy_mcp_core::command::Assertion,
) -> McpResult {
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
        Assertion::ComponentExists {
            entity_id,
            component,
        } => {
            let entity = match resolve_entity_by_index(world, *entity_id) {
                Some(entity) => entity,
                None => {
                    return McpResult::success(
                        json!({"passed": false, "assertion": "component_exists", "error": "Entity not found"}),
                    );
                }
            };
            let registry = world.resource::<AppTypeRegistry>().read();
            let has_component = registry.iter().any(|registration| {
                let path = registration.type_info().type_path_table();
                (path.short_path() == component || path.path() == component)
                    && registration
                        .data::<bevy::ecs::reflect::ReflectComponent>()
                        .and_then(|rc| rc.reflect(world.get_entity(entity).ok()?))
                        .is_some()
            });
            McpResult::success(
                json!({"passed": has_component, "assertion": "component_exists", "entity_id": entity_id, "component": component}),
            )
        }
        Assertion::ComponentEquals {
            entity_id,
            component,
            field,
            value,
        } => {
            let entity = match resolve_entity_by_index(world, *entity_id) {
                Some(entity) => entity,
                None => {
                    return McpResult::success(
                        json!({"passed": false, "assertion": "component_equals", "entity_id": entity_id, "component": component, "field": field, "expected": value, "error": "Entity not found"}),
                    );
                }
            };
            match serialize_assert_component(world, entity, component) {
                Ok(serialized) => equality_assertion(
                    "component_equals",
                    serialized,
                    field,
                    value,
                    json!({"entity_id": entity_id, "component": component}),
                ),
                Err(error) => McpResult::success(
                    json!({"passed": false, "assertion": "component_equals", "entity_id": entity_id, "component": component, "field": field, "expected": value, "error": error}),
                ),
            }
        }
        Assertion::EntityCount { expected } => {
            let count = world.iter_entities().count() as u32;
            McpResult::success(
                json!({"passed": count == *expected, "assertion": "entity_count", "expected": expected, "actual": count}),
            )
        }
        Assertion::ResourceEquals {
            resource,
            field,
            value,
        } => match serialize_assert_resource(world, resource) {
            Ok(serialized) => equality_assertion(
                "resource_equals",
                serialized,
                field,
                value,
                json!({"resource": resource}),
            ),
            Err(error) => McpResult::success(
                json!({"passed": false, "assertion": "resource_equals", "resource": resource, "field": field, "expected": value, "error": error}),
            ),
        },
    }
}
