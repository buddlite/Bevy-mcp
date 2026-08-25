use super::resources::resource_update;
use super::*;

pub(crate) fn mutation_operation_name(operation: &MutationOperation) -> &'static str {
    match operation {
        MutationOperation::ComponentInsert { .. } => "component_insert",
        MutationOperation::ComponentUpdate { .. } => "component_update",
        MutationOperation::ComponentRemove { .. } => "component_remove",
        MutationOperation::ResourceUpdate { .. } => "resource_update",
    }
}

pub(crate) fn transaction_validation_error(
    index: usize,
    operation: &MutationOperation,
    error: McpResult,
) -> McpResult {
    match error {
        McpResult::Error { code, message } => McpResult::error(
            "TRANSACTION_VALIDATION_FAILED",
            format!(
                "Operation {index} ({}) failed validation [{code}]: {message}",
                mutation_operation_name(operation)
            ),
        ),
        McpResult::Success(_) => McpResult::error(
            "TRANSACTION_VALIDATION_FAILED",
            format!(
                "Operation {index} ({}) returned an invalid validation result",
                mutation_operation_name(operation)
            ),
        ),
    }
}

pub(crate) fn validate_component_write(
    world: &World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
    component: &str,
    value: &Value,
) -> Result<(), McpResult> {
    if resolve_entity(world, entity_handle).is_none() {
        return Err(McpResult::error(
            "ENTITY_NOT_FOUND",
            format!("Entity {entity_handle} not found"),
        ));
    }

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == component)
        .ok_or_else(|| {
            McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .is_none()
    {
        return Err(McpResult::error(
            "COMPONENT_NOT_REFLECTED",
            format!("Component '{component}' does not have ReflectComponent data"),
        ));
    }

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json = wrapped.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    reflect_deserializer
        .deserialize(&mut deserializer)
        .map_err(|error| {
            McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize '{component}': {error}"),
            )
        })?;

    Ok(())
}

pub(crate) fn validate_component_remove(
    world: &World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
    component: &str,
) -> Result<(), McpResult> {
    if resolve_entity(world, entity_handle).is_none() {
        return Err(McpResult::error(
            "ENTITY_NOT_FOUND",
            format!("Entity {entity_handle} not found"),
        ));
    }

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == component)
        .ok_or_else(|| {
            McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .is_none()
    {
        return Err(McpResult::error(
            "COMPONENT_NOT_REFLECTED",
            format!("Component '{component}' does not have ReflectComponent data"),
        ));
    }

    Ok(())
}

pub(crate) fn validate_resource_write(
    world: &World,
    resource: &str,
    value: &Value,
) -> Result<(), McpResult> {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == resource)
        .ok_or_else(|| {
            McpResult::error(
                "RESOURCE_NOT_REGISTERED",
                format!("Resource '{resource}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::reflect::ReflectFromPtr>()
        .is_none()
    {
        return Err(McpResult::error(
            "RESOURCE_NOT_REFLECTED",
            format!("Resource '{resource}' does not have ReflectFromPtr data"),
        ));
    }

    if world.components().get_id(registration.type_id()).is_none() {
        return Err(McpResult::error(
            "RESOURCE_NOT_PRESENT",
            format!("Resource '{resource}' is not registered as a component"),
        ));
    }

    let type_id = registration.type_id();
    if !world
        .iter_resources()
        .any(|(info, _)| info.type_id() == Some(type_id))
    {
        return Err(McpResult::error(
            "RESOURCE_NOT_PRESENT",
            format!("Resource '{resource}' is not present in the world"),
        ));
    }

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json = wrapped.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    reflect_deserializer
        .deserialize(&mut deserializer)
        .map_err(|error| {
            McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize resource '{resource}': {error}"),
            )
        })?;

    Ok(())
}

pub(crate) fn apply_atomic_mutation_batch(
    world: &mut World,
    operations: &[MutationOperation],
    dry_run: bool,
) -> McpResult {
    if operations.is_empty() {
        return McpResult::error(
            "EMPTY_TRANSACTION",
            "Atomic mutation batches require at least one operation",
        );
    }
    if operations.len() > 256 {
        return McpResult::error(
            "TRANSACTION_TOO_LARGE",
            "Atomic mutation batches are limited to 256 operations",
        );
    }

    // Validate the entire transaction against one exclusive World snapshot before
    // applying the first mutation. Supported operations do not despawn entities,
    // change the type registry, or advance the schedule, so successful validation
    // makes the commit phase deterministic within this exclusive system call.
    for (index, operation) in operations.iter().enumerate() {
        let validation = match operation {
            MutationOperation::ComponentInsert {
                entity,
                component,
                value,
            }
            | MutationOperation::ComponentUpdate {
                entity,
                component,
                value,
            } => validate_component_write(world, entity, component, value),
            MutationOperation::ComponentRemove { entity, component } => {
                validate_component_remove(world, entity, component)
            }
            MutationOperation::ResourceUpdate { resource, value } => {
                validate_resource_write(world, resource, value)
            }
        };

        if let Err(error) = validation {
            return transaction_validation_error(index, operation, error);
        }
    }

    if dry_run {
        return McpResult::success(json!({
            "mode": "atomic_dry_run",
            "validated": true,
            "committed": false,
            "operation_count": operations.len(),
            "operations": operations
                .iter()
                .enumerate()
                .map(|(index, operation)| json!({
                    "index": index,
                    "operation": mutation_operation_name(operation),
                }))
                .collect::<Vec<_>>(),
        }));
    }

    let mut applied = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        let result = match operation {
            MutationOperation::ComponentInsert {
                entity,
                component,
                value,
            }
            | MutationOperation::ComponentUpdate {
                entity,
                component,
                value,
            } => {
                let Some(entity) = resolve_entity(world, entity) else {
                    return McpResult::error(
                        "TRANSACTION_COMMIT_INVARIANT_FAILED",
                        format!("Validated entity disappeared before operation {index}"),
                    );
                };
                insert_component_by_reflect(world, entity, component, value)
            }
            MutationOperation::ComponentRemove { entity, component } => {
                let Some(entity) = resolve_entity(world, entity) else {
                    return McpResult::error(
                        "TRANSACTION_COMMIT_INVARIANT_FAILED",
                        format!("Validated entity disappeared before operation {index}"),
                    );
                };
                remove_component_by_reflect(world, entity, component)
            }
            MutationOperation::ResourceUpdate { resource, value } => {
                resource_update(world, resource, value)
            }
        };

        match result {
            McpResult::Success(_) => applied.push(json!({
                "index": index,
                "operation": mutation_operation_name(operation),
            })),
            McpResult::Error { code, message } => {
                return McpResult::error(
                    "TRANSACTION_COMMIT_INVARIANT_FAILED",
                    format!(
                        "Prevalidated operation {index} ({}) unexpectedly failed [{code}]: {message}",
                        mutation_operation_name(operation)
                    ),
                );
            }
        }
    }

    McpResult::success(json!({
        "mode": "atomic",
        "validated": true,
        "committed": true,
        "operation_count": operations.len(),
        "operations": applied,
    }))
}

pub(crate) fn insert_component_by_reflect(
    world: &mut World,
    entity: Entity,
    component: &str,
    value: &Value,
) -> McpResult {
    tracing::debug!(component, ?value, "insert_component_by_reflect");

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match find_type_registration(&registry, component) {
        Some(r) => r,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            );
        }
    };

    let reflect_component = match registration.data::<bevy::ecs::reflect::ReflectComponent>() {
        Some(rc) => rc,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REFLECTED",
                format!("Component '{component}' does not have ReflectComponent data"),
            );
        }
    };

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json_str = wrapped.to_string();
    tracing::debug!(json_str, "deserializing");

    let mut deserializer = serde_json::Deserializer::from_str(&json_str);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    let reflected = match reflect_deserializer.deserialize(&mut deserializer) {
        Ok(r) => r,
        Err(e) => {
            return McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize '{component}': {e}"),
            );
        }
    };

    let mut entity_ref = match world.get_entity_mut(entity) {
        Ok(e) => e,
        Err(_) => return McpResult::error("ENTITY_NOT_FOUND", "Entity not found"),
    };
    reflect_component.insert(&mut entity_ref, reflected.as_ref(), &registry);

    McpResult::success(json!({ "inserted": component }))
}

pub(crate) fn remove_component_by_reflect(
    world: &mut World,
    entity: Entity,
    component: &str,
) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match find_type_registration(&registry, component) {
        Some(r) => r,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            );
        }
    };

    let reflect_component = match registration.data::<bevy::ecs::reflect::ReflectComponent>() {
        Some(rc) => rc,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REFLECTED",
                format!("Component '{component}' does not have ReflectComponent data"),
            );
        }
    };

    let mut entity_ref = match world.get_entity_mut(entity) {
        Ok(e) => e,
        Err(_) => return McpResult::error("ENTITY_NOT_FOUND", "Entity not found"),
    };
    reflect_component.remove(&mut entity_ref);

    McpResult::success(json!({ "removed": component }))
}

pub(crate) fn entity_reparent(
    world: &mut World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
    parent_handle: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
) -> McpResult {
    use bevy::ecs::hierarchy::ChildOf;

    let entity = match resolve_entity(world, entity_handle) {
        Some(entity) => entity,
        None => {
            return McpResult::error(
                "ENTITY_NOT_FOUND",
                format!("Entity {entity_handle} not found"),
            );
        }
    };
    let parent = match parent_handle {
        Some(parent_handle) => match resolve_entity(world, parent_handle) {
            Some(parent) => Some(parent),
            None => {
                return McpResult::error(
                    "ENTITY_NOT_FOUND",
                    format!("Parent entity {parent_handle} not found"),
                );
            }
        },
        None => None,
    };

    if let Some(parent) = parent {
        if parent == entity {
            return McpResult::error("INVALID_HIERARCHY", "An entity cannot be its own parent");
        }
        let mut cursor = Some(parent);
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = cursor {
            if current == entity {
                return McpResult::error(
                    "INVALID_HIERARCHY",
                    "Reparenting would create a hierarchy cycle",
                );
            }
            if !visited.insert(current) {
                return McpResult::error(
                    "INVALID_HIERARCHY",
                    "Existing parent chain contains a hierarchy cycle",
                );
            }
            cursor = world.get::<ChildOf>(current).map(ChildOf::parent);
        }
        let Ok(mut entity_ref) = world.get_entity_mut(entity) else {
            return McpResult::error(
                "ENTITY_NOT_FOUND",
                format!("Entity {entity_handle} not found"),
            );
        };
        entity_ref.insert(ChildOf(parent));
        McpResult::success(json!({
            "reparented": entity_to_uri(world, entity),
            "new_parent": entity_to_uri(world, parent)
        }))
    } else {
        let Ok(mut entity_ref) = world.get_entity_mut(entity) else {
            return McpResult::error(
                "ENTITY_NOT_FOUND",
                format!("Entity {entity_handle} not found"),
            );
        };
        entity_ref.remove::<ChildOf>();
        McpResult::success(
            json!({ "reparented": entity_to_uri(world, entity), "new_parent": null }),
        )
    }
}

pub(crate) fn entity_duplicate(
    world: &mut World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    if resolve_entity(world, entity_handle).is_none() {
        return McpResult::error(
            "ENTITY_NOT_FOUND",
            format!("Entity {entity_handle} not found"),
        );
    }
    McpResult::error(
        "NOT_IMPLEMENTED",
        "Entity duplication is disabled until component cloning is implemented",
    )
}

pub(crate) fn entity_spawn(_components: &[(String, Value)]) -> McpResult {
    McpResult::error("INTERNAL", "Entity spawn not yet wired to deferred queue")
}

pub(crate) fn entity_despawn(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    McpResult::error("INTERNAL", "Entity despawn not yet wired to deferred queue")
}

pub(crate) fn component_insert(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
    _component: &str,
    _value: &Value,
) -> McpResult {
    McpResult::error(
        "INTERNAL",
        "Component insert not yet wired to deferred queue",
    )
}

pub(crate) fn component_update(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
    _component: &str,
    _value: &Value,
) -> McpResult {
    McpResult::error(
        "INTERNAL",
        "Component update not yet wired to deferred queue",
    )
}

pub(crate) fn component_remove(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
    _component: &str,
) -> McpResult {
    McpResult::error(
        "INTERNAL",
        "Component remove not yet wired to deferred queue",
    )
}

#[cfg(test)]
mod bevy_019_hierarchy_tests {
    use super::*;
    use crate::instance::McpInstanceId;
    use bevy::ecs::hierarchy::{ChildOf, Children};

    fn handle(entity: Entity) -> bevy_mcp_core::entity_handle::EntityHandle {
        bevy_mcp_core::entity_handle::EntityHandle::new(
            "test",
            "main",
            entity.index().index() as u64,
            entity.generation().to_bits() as u64,
        )
    }

    #[test]
    fn invalid_parent_preserves_existing_relationship() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let old_parent = world.spawn_empty().id();
        let child = world.spawn(ChildOf(old_parent)).id();
        let missing = world.spawn_empty().id();
        let missing_handle = handle(missing);
        world.despawn(missing);
        let result = entity_reparent(&mut world, &handle(child), Some(&missing_handle));
        assert!(matches!(result, McpResult::Error { ref code, .. } if code == "ENTITY_NOT_FOUND"));
        assert_eq!(
            world.get::<ChildOf>(child).map(ChildOf::parent),
            Some(old_parent)
        );
        assert!(
            world
                .get::<Children>(old_parent)
                .is_some_and(|children| children.contains(&child))
        );
    }

    #[test]
    fn reparent_rejects_self_and_cycles() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let root = world.spawn_empty().id();
        let child = world.spawn(ChildOf(root)).id();
        let self_result = entity_reparent(&mut world, &handle(root), Some(&handle(root)));
        assert!(
            matches!(self_result, McpResult::Error { ref code, .. } if code == "INVALID_HIERARCHY")
        );
        let cycle_result = entity_reparent(&mut world, &handle(root), Some(&handle(child)));
        assert!(
            matches!(cycle_result, McpResult::Error { ref code, .. } if code == "INVALID_HIERARCHY")
        );
        assert_eq!(world.get::<ChildOf>(child).map(ChildOf::parent), Some(root));
        assert!(world.get::<ChildOf>(root).is_none());
    }

    #[test]
    fn valid_reparent_uses_bevy_relationship_hooks() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let old_parent = world.spawn_empty().id();
        let new_parent = world.spawn_empty().id();
        let child = world.spawn(ChildOf(old_parent)).id();
        let result = entity_reparent(&mut world, &handle(child), Some(&handle(new_parent)));
        assert!(matches!(result, McpResult::Success(_)));
        assert_eq!(
            world.get::<ChildOf>(child).map(ChildOf::parent),
            Some(new_parent)
        );
        assert!(
            world
                .get::<Children>(new_parent)
                .is_some_and(|children| children.contains(&child))
        );
        assert!(
            world
                .get::<Children>(old_parent)
                .is_none_or(|children| !children.contains(&child))
        );
    }
}
