use super::*;

pub(crate) fn ui_query(
    world: &World,
    root: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
    max_depth: u32,
) -> McpResult {
    use bevy::ui::Node;

    fn build_ui_tree(
        world: &World,
        entity: Entity,
        depth: u32,
        max_depth: u32,
    ) -> serde_json::Value {
        if depth >= max_depth {
            return json!({
                "handle": entity_to_uri(world, entity),
                "id": entity.index().index(),
                "children": [],
                "truncated": true,
            });
        }

        let mut node_info = json!({
            "handle": entity_to_uri(world, entity),
            "id": entity.index().index(),
        });

        if world.get::<Node>(entity).is_some() {
            node_info["node"] = json!({ "has_node": true });
        }

        if let Some(text) = world.get::<bevy::prelude::Text>(entity) {
            node_info["text"] = json!(text.to_string());
        }

        let children: Vec<serde_json::Value> =
            if let Some(children) = world.get::<bevy::ecs::hierarchy::Children>(entity) {
                children
                    .iter()
                    .map(|child| build_ui_tree(world, child, depth + 1, max_depth))
                    .collect()
            } else {
                vec![]
            };
        node_info["children"] = json!(children);
        node_info
    }

    if let Some(root_handle) = root {
        let entity = match resolve_entity(world, root_handle) {
            Some(e) => e,
            None => {
                return McpResult::error(
                    "ENTITY_NOT_FOUND",
                    format!("Entity {root_handle} not found"),
                );
            }
        };
        let tree = build_ui_tree(world, entity, 0, max_depth);
        McpResult::success(json!({ "ui": tree }))
    } else {
        let mut roots = Vec::new();
        for entity_ref in world.iter_entities() {
            let entity = entity_ref.id();
            if world.get::<Node>(entity).is_some()
                && world.get::<bevy::ecs::hierarchy::ChildOf>(entity).is_none()
            {
                roots.push(build_ui_tree(world, entity, 0, max_depth));
            }
        }
        McpResult::success(json!({
            "ui": roots,
            "root_count": roots.len(),
        }))
    }
}

pub(crate) fn ui_inspect(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    let mut info = json!({
        "handle": entity_to_uri(world, entity),
        "id": entity.index().index(),
    });

    if world.get::<bevy::ui::Node>(entity).is_some() {
        info["has_node"] = json!(true);
    }
    if let Some(text) = world.get::<bevy::prelude::Text>(entity) {
        info["text"] = json!(text.to_string());
    }
    if world.get::<bevy::prelude::Button>(entity).is_some() {
        info["is_button"] = json!(true);
    }

    McpResult::success(info)
}
