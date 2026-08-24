use bevy::ecs::entity::Entity;
use bevy::prelude::World;
use bevy_mcp_core::command::{McpCommand, McpResult, MutationOperation};
use bevy_mcp_core::entity_handle::EntityHandle;

use crate::instance::McpInstanceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityResolveError {
    StaleInstance,
    InvalidWorld,
    NotFound,
}

fn current_instance(world: &World) -> &str {
    world
        .get_resource::<McpInstanceId>()
        .map(McpInstanceId::as_str)
        .unwrap_or("default")
}

pub fn validate_entity_namespace(
    world: &World,
    handle: &EntityHandle,
) -> Result<(), EntityResolveError> {
    if handle.instance != current_instance(world) {
        return Err(EntityResolveError::StaleInstance);
    }
    if handle.world != "main" {
        return Err(EntityResolveError::InvalidWorld);
    }
    Ok(())
}

pub fn resolve_entity_checked(
    world: &World,
    handle: &EntityHandle,
) -> Result<Entity, EntityResolveError> {
    validate_entity_namespace(world, handle)?;
    let index = u32::try_from(handle.id).map_err(|_| EntityResolveError::NotFound)?;
    let generation = u32::try_from(handle.generation).map_err(|_| EntityResolveError::NotFound)?;
    let index =
        bevy::ecs::entity::EntityIndex::from_raw_u32(index).ok_or(EntityResolveError::NotFound)?;
    let entity = Entity::from_index_and_generation(
        index,
        bevy::ecs::entity::EntityGeneration::from_bits(generation),
    );
    world
        .get_entity(entity)
        .map(|_| entity)
        .map_err(|_| EntityResolveError::NotFound)
}

pub fn resolve_entity(world: &World, handle: &EntityHandle) -> Option<Entity> {
    resolve_entity_checked(world, handle).ok()
}

fn namespace_error(world: &World, handle: &EntityHandle) -> Option<McpResult> {
    match validate_entity_namespace(world, handle) {
        Ok(()) => None,
        Err(EntityResolveError::StaleInstance) => Some(McpResult::error(
            "STALE_INSTANCE",
            format!(
                "Entity handle belongs to game instance {}; current instance is {}",
                handle.instance,
                current_instance(world)
            ),
        )),
        Err(EntityResolveError::InvalidWorld) => Some(McpResult::error(
            "INVALID_WORLD",
            format!("Entity handle world '{}' is not supported", handle.world),
        )),
        Err(EntityResolveError::NotFound) => None,
    }
}

pub fn validate_command_entity_handles(world: &World, command: &McpCommand) -> Option<McpResult> {
    let check = |handle: &EntityHandle| namespace_error(world, handle);
    match command {
        McpCommand::EntityGet { entity }
        | McpCommand::EntityDespawn { entity }
        | McpCommand::ComponentGet { entity, .. }
        | McpCommand::ComponentInsert { entity, .. }
        | McpCommand::ComponentUpdate { entity, .. }
        | McpCommand::ComponentRemove { entity, .. }
        | McpCommand::CameraFrameEntity { entity, .. }
        | McpCommand::CameraLookAt { entity }
        | McpCommand::UiInspect { entity }
        | McpCommand::UiClick { entity }
        | McpCommand::UiType { entity, .. }
        | McpCommand::EntityDuplicate { entity }
        | McpCommand::TemplateSave { entity, .. } => check(entity),
        McpCommand::Hierarchy { root, .. } | McpCommand::UiQuery { root, .. } => {
            root.as_ref().and_then(check)
        }
        McpCommand::EntityReparent { entity, parent } => {
            check(entity).or_else(|| parent.as_ref().and_then(check))
        }
        McpCommand::MeshSpawn { parent, .. } | McpCommand::TemplateLoad { parent, .. } => {
            parent.as_ref().and_then(check)
        }
        McpCommand::AtomicMutationBatch { operations, .. } => {
            operations.iter().find_map(|operation| {
                let entity = match operation {
                    MutationOperation::ComponentInsert { entity, .. }
                    | MutationOperation::ComponentUpdate { entity, .. }
                    | MutationOperation::ComponentRemove { entity, .. } => Some(entity),
                    MutationOperation::ResourceUpdate { .. } => None,
                };
                entity.and_then(check)
            })
        }
        _ => None,
    }
}

/// Resolve an index-only entity reference for legacy assertion parameters.
pub fn resolve_entity_by_index(world: &World, index: u32) -> Option<Entity> {
    world
        .iter_entities()
        .map(|entity| entity.id())
        .find(|entity| entity.index().index() == index)
}

pub fn entity_to_uri_for_instance(instance_id: &str, entity: Entity) -> String {
    format!(
        "entity://{}/main/{}/{}",
        instance_id,
        entity.index().index(),
        entity.generation()
    )
}

/// Build an entity handle URI scoped to the current game-process instance.
pub fn entity_to_uri(world: &World, entity: Entity) -> String {
    entity_to_uri_for_instance(current_instance(world), entity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_instance_is_distinct_from_missing_entity() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-new"));
        let entity = world.spawn_empty().id();
        let stale = EntityHandle::new(
            "run-old",
            "main",
            entity.index().index() as u64,
            entity.generation().to_bits() as u64,
        );
        assert_eq!(
            resolve_entity_checked(&world, &stale),
            Err(EntityResolveError::StaleInstance)
        );
        let result =
            validate_command_entity_handles(&world, &McpCommand::EntityGet { entity: stale })
                .unwrap();
        assert!(matches!(
            result,
            McpResult::Error { ref code, .. } if code == "STALE_INSTANCE"
        ));
    }

    #[test]
    fn generated_handle_uses_current_instance() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-current"));
        let entity = world.spawn_empty().id();
        assert!(entity_to_uri(&world, entity).starts_with("entity://run-current/main/"));
    }
}
