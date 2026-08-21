use bevy::ecs::entity::Entity;
use bevy::prelude::World;
use bevy_mcp_core::entity_handle::EntityHandle;

/// Resolve a fully-qualified MCP entity handle.
///
/// Entity indices are recycled by Bevy, so both the instance/world namespace and
/// generation are validated before returning an entity.
pub fn resolve_entity(world: &World, handle: &EntityHandle) -> Option<Entity> {
    if handle.instance != "default" || handle.world != "main" {
        return None;
    }

    let index = u32::try_from(handle.id).ok()?;
    let generation = u32::try_from(handle.generation).ok()?;
    let index = bevy::ecs::entity::EntityIndex::from_raw_u32(index)?;
    let entity = Entity::from_index_and_generation(
        index,
        bevy::ecs::entity::EntityGeneration::from_bits(generation),
    );
    if world.get_entity(entity).is_ok() {
        Some(entity)
    } else {
        None
    }
}

/// Resolve an index-only entity reference for legacy assertion parameters.
/// MCP tools that accept a handle must use `resolve_entity` instead.
pub fn resolve_entity_by_index(world: &World, index: u32) -> Option<Entity> {
    world
        .iter_entities()
        .map(|entity| entity.id())
        .find(|entity| entity.index().index() == index)
}

/// Build an entity handle URI from a live entity.
pub fn entity_to_uri(entity: Entity) -> String {
    format!(
        "entity://default/main/{}/{}",
        entity.index().index(),
        entity.generation()
    )
}
