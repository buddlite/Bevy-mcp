use super::*;

pub(crate) fn world_summary(world: &World) -> McpResult {
    let mut entity_count = 0usize;
    let mut component_ids: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for entity_ref in world.iter_entities() {
        entity_count += 1;
        for component_id in entity_ref.archetype().components() {
            component_ids.insert(component_id.index());
        }
    }

    McpResult::success(json!({
        "entities": entity_count,
        "archetypes": world.archetypes().len(),
        "component_types": component_ids.len(),
    }))
}

pub(crate) fn world_context_scan(world: &World, registry: &McpRegistry) -> McpResult {
    use bevy::ecs::hierarchy::{ChildOf, Children};

    let mut total_entity_count: usize = 0;
    let mut archetype_map: std::collections::HashMap<usize, (Vec<usize>, usize)> =
        std::collections::HashMap::new();

    for entity_ref in world.iter_entities() {
        total_entity_count += 1;
        let arch_id = entity_ref.archetype().id().index();
        let comp_ids: Vec<usize> = entity_ref
            .archetype()
            .components()
            .iter()
            .map(|cid| cid.index())
            .collect();
        archetype_map
            .entry(arch_id)
            .and_modify(|(_, count)| *count += 1)
            .or_insert_with(|| (comp_ids, 1));
    }

    let mut archetypes_json = Vec::new();
    let mut arch_keys: Vec<usize> = archetype_map.keys().copied().collect();
    arch_keys.sort();
    for arch_id in &arch_keys {
        let (comp_ids, count) = &archetype_map[arch_id];
        let component_names: Vec<String> = comp_ids
            .iter()
            .filter_map(|cid| {
                world
                    .components()
                    .get_info(bevy::ecs::component::ComponentId::new(*cid))
                    .map(|info| info.name().to_string())
            })
            .collect();
        archetypes_json.push(json!({
            "id": arch_id,
            "entities": count,
            "components": component_names,
        }));
    }

    let app_registry = world.resource::<AppTypeRegistry>();
    let type_registry = app_registry.read();

    let mut component_entity_counts: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (comp_ids, count) in archetype_map.values() {
        for cid in comp_ids {
            *component_entity_counts.entry(*cid).or_insert(0) += count;
        }
    }

    let mut component_types_json = Vec::new();
    for registration in type_registry.iter() {
        let is_component = registration
            .data::<bevy::ecs::reflect::ReflectComponent>()
            .is_some();
        if !is_component {
            continue;
        }
        let type_info = registration.type_info();
        let short_path = type_info.type_path_table().short_path().to_string();
        let type_path = type_info.type_path_table().path().to_string();
        let entity_count = world
            .components()
            .get_id(registration.type_id())
            .and_then(|cid| component_entity_counts.get(&cid.index()).copied())
            .unwrap_or(0);

        component_types_json.push(json!({
            "name": short_path,
            "type_path": type_path,
            "is_component": true,
            "entity_count": entity_count,
        }));
    }

    let mut resource_types_json = Vec::new();
    for registration in type_registry.iter() {
        if registration
            .data::<bevy::ecs::reflect::ReflectResource>()
            .is_some()
        {
            let type_info = registration.type_info();
            resource_types_json.push(json!({
                "name": type_info.type_path_table().short_path(),
                "type_path": type_info.type_path_table().path(),
            }));
        }
    }

    fn build_context_tree(
        world: &World,
        entity: Entity,
        depth: u32,
        max_depth: u32,
    ) -> serde_json::Value {
        if depth >= max_depth {
            return json!({
                "entity_id": entity.index().index(),
                "name": "",
                "components": [],
                "children": [],
                "truncated": true,
            });
        }

        let entity_ref = world.get_entity(entity).unwrap();
        let component_names: Vec<String> = entity_ref
            .archetype()
            .components()
            .iter()
            .filter_map(|cid| {
                world
                    .components()
                    .get_info(*cid)
                    .map(|info| info.name().to_string())
            })
            .collect();

        let name = world
            .get::<bevy::prelude::Name>(entity)
            .map(|n| n.to_string())
            .unwrap_or_default();

        let children_json: Vec<serde_json::Value> =
            if let Some(children) = world.get::<Children>(entity) {
                children
                    .iter()
                    .map(|child| build_context_tree(world, child, depth + 1, max_depth))
                    .collect()
            } else {
                vec![]
            };

        json!({
            "entity_id": entity.index().index(),
            "name": name,
            "components": component_names,
            "children": children_json,
        })
    }

    let mut roots_json = Vec::new();
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        if world.get::<ChildOf>(entity).is_none() {
            roots_json.push(build_context_tree(world, entity, 0, 10));
        }
    }

    let runtime_json = json!({
        "frame": registry.frame,
        "paused": registry.paused,
        "time_scale": registry.time_scale,
    });

    McpResult::success(json!({
        "entity_count": total_entity_count,
        "archetype_count": archetype_map.len(),
        "archetypes": archetypes_json,
        "component_types": component_types_json,
        "resource_types": resource_types_json,
        "hierarchy": { "roots": roots_json },
        "runtime": runtime_json,
    }))
}

pub(crate) fn entity_query(
    world: &World,
    with_components: &[String],
    without_components: &[String],
    include: &[String],
    limit: u32,
) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();
    let component_id = |name: &str| {
        registry
            .iter()
            .find(|registration| {
                let path = registration.type_info().type_path_table();
                path.short_path() == name || path.path() == name
            })
            .and_then(|registration| world.components().get_id(registration.type_id()))
    };

    let resolve_components = |names: &[String]| -> Result<Vec<_>, McpResult> {
        names
            .iter()
            .map(|name| {
                component_id(name).ok_or_else(|| {
                    McpResult::error(
                        "COMPONENT_NOT_REGISTERED",
                        format!("Component '{name}' is not registered"),
                    )
                })
            })
            .collect()
    };
    let with_ids = match resolve_components(with_components) {
        Ok(ids) => ids,
        Err(error) => return error,
    };
    let without_ids = match resolve_components(without_components) {
        Ok(ids) => ids,
        Err(error) => return error,
    };
    let include_ids = match resolve_components(include) {
        Ok(ids) => ids,
        Err(error) => return error,
    };
    drop(registry);

    let mut entities = Vec::new();
    let mut count = 0u32;
    for entity_ref in world.iter_entities() {
        if !with_ids.iter().all(|id| entity_ref.contains_id(*id))
            || without_ids.iter().any(|id| entity_ref.contains_id(*id))
        {
            continue;
        }
        if limit > 0 && count >= limit {
            break;
        }
        let entity = entity_ref.id();
        let included_components: Vec<_> = include_ids
            .iter()
            .filter_map(|id| {
                world
                    .components()
                    .get_info(*id)
                    .map(|info| info.name().to_string())
            })
            .collect();
        entities.push(json!({
            "handle": entity_to_uri(entity),
            "id": entity.index().index(),
            "included_components": included_components,
        }));
        count += 1;
    }

    McpResult::success(json!({
        "entities": entities,
        "count": entities.len(),
    }))
}

pub(crate) fn entity_get(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    let entity_ref = world.get_entity(entity).unwrap();
    let mut components = Vec::new();
    for component_id in entity_ref.archetype().components() {
        if let Some(info) = world.components().get_info(*component_id) {
            components.push(json!({
                "name": info.name().to_string(),
                "id": component_id.index(),
            }));
        }
    }

    McpResult::success(json!({
        "handle": entity_to_uri(entity),
        "id": entity.index().index(),
        "components": components,
    }))
}

pub(crate) fn component_get(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    component: &str,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    let entity_ref = world.get_entity(entity).unwrap();
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();

    let registration = registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == component);

    let registration = match registration {
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

    let reflected = match reflect_component.reflect(entity_ref) {
        Some(r) => r,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_PRESENT",
                format!("Entity {handle} does not have component '{component}'"),
            );
        }
    };

    let serializer =
        bevy::reflect::serde::ReflectSerializer::new(reflected.as_reflect(), &registry);
    match serde_json::to_value(&serializer) {
        Ok(value) => McpResult::success(json!({
            "component": component,
            "entity_id": entity.index().index(),
            "value": value,
        })),
        Err(e) => McpResult::error(
            "SERIALIZATION_ERROR",
            format!("Failed to serialize component '{component}': {e}"),
        ),
    }
}

pub(crate) fn component_schema(world: &World, component: &str) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();

    let registration = registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == component);

    let registration = match registration {
        Some(r) => r,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            );
        }
    };

    let type_info = registration.type_info();
    let type_path = type_info.type_path_table().path().to_string();
    let is_component = registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .is_some();

    let mut schema = json!({
        "name": component,
        "type_path": type_path,
        "is_component": is_component,
    });

    match type_info {
        bevy::reflect::TypeInfo::Struct(s) => {
            let fields: Vec<Value> = s
                .iter()
                .map(|field| json!({ "name": field.name(), "type": field.type_path() }))
                .collect();
            schema["kind"] = json!("struct");
            schema["fields"] = json!(fields);
            schema["field_count"] = json!(s.field_len());
        }
        bevy::reflect::TypeInfo::TupleStruct(ts) => {
            let fields: Vec<Value> = ts
                .iter()
                .map(|field| json!({ "type": field.type_path() }))
                .collect();
            schema["kind"] = json!("tuple_struct");
            schema["fields"] = json!(fields);
            schema["field_count"] = json!(ts.field_len());
        }
        bevy::reflect::TypeInfo::Tuple(t) => {
            let fields: Vec<Value> = t
                .iter()
                .map(|field| json!({ "type": field.type_path() }))
                .collect();
            schema["kind"] = json!("tuple");
            schema["fields"] = json!(fields);
            schema["field_count"] = json!(t.field_len());
        }
        bevy::reflect::TypeInfo::List(l) => {
            schema["kind"] = json!("list");
            schema["type_path"] = json!(l.type_path());
        }
        bevy::reflect::TypeInfo::Array(a) => {
            schema["kind"] = json!("array");
            schema["type_path"] = json!(a.type_path());
        }
        bevy::reflect::TypeInfo::Map(m) => {
            schema["kind"] = json!("map");
            schema["type_path"] = json!(m.type_path());
        }
        bevy::reflect::TypeInfo::Set(s) => {
            schema["kind"] = json!("set");
            schema["type_path"] = json!(s.type_path());
        }
        bevy::reflect::TypeInfo::Enum(e) => {
            let variants: Vec<Value> = e
                .iter()
                .map(|variant| {
                    let mut v = json!({ "name": variant.name() });
                    match variant {
                        bevy::reflect::enums::VariantInfo::Struct(s) => {
                            let fields: Vec<Value> = s
                                .iter()
                                .map(|f| json!({ "name": f.name(), "type": f.type_path() }))
                                .collect();
                            v["kind"] = json!("struct");
                            v["fields"] = json!(fields);
                        }
                        bevy::reflect::enums::VariantInfo::Tuple(t) => {
                            let fields: Vec<Value> =
                                t.iter().map(|f| json!({ "type": f.type_path() })).collect();
                            v["kind"] = json!("tuple");
                            v["fields"] = json!(fields);
                        }
                        bevy::reflect::enums::VariantInfo::Unit(_) => {
                            v["kind"] = json!("unit");
                        }
                    }
                    v
                })
                .collect();
            schema["kind"] = json!("enum");
            schema["variants"] = json!(variants);
            schema["variant_count"] = json!(e.variant_len());
        }
        bevy::reflect::TypeInfo::Opaque(_) => {
            schema["kind"] = json!("opaque");
        }
    }

    McpResult::success(schema)
}

pub(crate) fn hierarchy(
    world: &World,
    root: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
    max_depth: u32,
) -> McpResult {
    use bevy::ecs::hierarchy::{ChildOf, Children};

    fn build_tree(world: &World, entity: Entity, depth: u32, max_depth: u32) -> serde_json::Value {
        if depth >= max_depth {
            return json!({
                "handle": entity_to_uri(entity),
                "id": entity.index().index(),
                "children": [],
                "truncated": true,
            });
        }

        let children: Vec<serde_json::Value> = if let Some(children) = world.get::<Children>(entity)
        {
            children
                .iter()
                .map(|child| build_tree(world, child, depth + 1, max_depth))
                .collect()
        } else {
            vec![]
        };

        json!({
            "handle": entity_to_uri(entity),
            "id": entity.index().index(),
            "children": children,
        })
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
        let tree = build_tree(world, entity, 0, max_depth);
        McpResult::success(json!({ "hierarchy": tree }))
    } else {
        let mut roots = Vec::new();
        for entity_ref in world.iter_entities() {
            let entity = entity_ref.id();
            if world.get::<ChildOf>(entity).is_none() {
                roots.push(build_tree(world, entity, 0, max_depth));
            }
        }
        McpResult::success(json!({
            "hierarchy": roots,
            "root_count": roots.len(),
        }))
    }
}
