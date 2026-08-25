use super::ecs_mutate::insert_component_by_reflect;
use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn mesh_spawn_apply(
    world: &mut World,
    shape: &str,
    size: f64,
    radius: f64,
    color: (f32, f32, f32, f32),
    metallic: f32,
    roughness: f32,
    position: (f32, f32, f32),
    parent: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
) -> McpResult {
    use bevy::pbr::{MeshMaterial3d, StandardMaterial};

    let size_f32 = size as f32;
    let radius_f32 = radius as f32;
    let mesh_handle = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        match shape {
            "cube" => meshes.add(Cuboid::new(size_f32, size_f32, size_f32)),
            "sphere" => meshes.add(Sphere::new(radius_f32)),
            "plane" => meshes.add(Plane3d::default().mesh().size(size_f32, size_f32)),
            "cylinder" => meshes.add(Cylinder::new(radius_f32, size_f32)),
            "torus" => meshes.add(Torus::new(radius_f32, size_f32 * 0.4)),
            _ => {
                return McpResult::error(
                    "INVALID_SHAPE",
                    format!("Unknown shape '{shape}'. Valid: cube, sphere, plane, cylinder, torus"),
                );
            }
        }
    };

    let material_handle = {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        materials.add(StandardMaterial {
            base_color: Color::srgba(color.0, color.1, color.2, color.3),
            metallic,
            perceptual_roughness: roughness,
            ..default()
        })
    };

    let entity = world
        .spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            Transform::from_xyz(position.0, position.1, position.2),
        ))
        .id();

    if let Some(parent_handle) = parent {
        if let Some(parent_entity) = resolve_entity(world, parent_handle) {
            if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
                use bevy::ecs::hierarchy::ChildOf;
                entity_ref.insert(ChildOf(parent_entity));
            }
        } else {
            return McpResult::error(
                "PARENT_NOT_FOUND",
                format!("Parent entity {parent_handle} not found"),
            );
        }
    }

    McpResult::success(json!({
        "handle": entity_to_uri(world, entity),
        "id": entity.index().index(),
        "shape": shape,
        "size": size,
        "radius": radius,
    }))
}

pub(crate) fn template_save(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    name: &str,
    path: Option<&str>,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let entity_ref = world.get_entity(entity).unwrap();
    let mut components_json = serde_json::Map::new();

    for component_id in entity_ref.archetype().components() {
        let Some(info) = world.components().get_info(*component_id) else {
            continue;
        };
        let component_name = info.name().to_string();
        let short_name = component_name
            .rsplit("::")
            .next()
            .unwrap_or(&component_name)
            .to_string();

        let registration = find_type_registration(&registry, &component_name)
            .or_else(|| find_type_registration(&registry, &short_name));

        if let Some(registration) = registration
            && let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            && let Some(reflected) = reflect_component.reflect(entity_ref)
        {
            let serializer =
                bevy::reflect::serde::ReflectSerializer::new(reflected.as_reflect(), &registry);
            if let Ok(value) = serde_json::to_value(&serializer) {
                if let Some(obj) = value.as_object()
                    && let Some(inner) = obj.values().next()
                {
                    components_json.insert(short_name, inner.clone());
                    continue;
                }
                components_json.insert(short_name, value);
            }
        }
    }

    let template = json!({
        "name": name,
        "components": components_json,
    });

    let file_path = match path {
        Some(p) => p.to_string(),
        None => format!("templates/{name}.json"),
    };
    let json_string = match serde_json::to_string_pretty(&template) {
        Ok(s) => s,
        Err(e) => {
            return McpResult::error(
                "SERIALIZATION_ERROR",
                format!("Failed to serialize template: {e}"),
            );
        }
    };

    if let Some(parent) = std::path::Path::new(&file_path).parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return McpResult::error(
            "IO_ERROR",
            format!("Failed to create directory {}: {e}", parent.display()),
        );
    }
    if let Err(e) = std::fs::write(&file_path, &json_string) {
        return McpResult::error("IO_ERROR", format!("Failed to write template file: {e}"));
    }

    McpResult::success(json!({
        "saved": true,
        "name": name,
        "path": file_path,
        "component_count": components_json.len(),
    }))
}

pub(crate) fn template_load_apply(
    world: &mut World,
    name: &str,
    path: Option<&str>,
    parent: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
    position: Option<(f32, f32, f32)>,
) -> McpResult {
    let file_path = match path {
        Some(p) => p.to_string(),
        None => format!("templates/{name}.json"),
    };

    let json_string = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            return McpResult::error(
                "IO_ERROR",
                format!("Failed to read template file '{file_path}': {e}"),
            );
        }
    };

    let template: serde_json::Value = match serde_json::from_str(&json_string) {
        Ok(v) => v,
        Err(e) => {
            return McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to parse template JSON: {e}"),
            );
        }
    };

    let template_name = template
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(name);

    let components = match template.get("components").and_then(|v| v.as_object()) {
        Some(c) => c,
        None => {
            return McpResult::error(
                "INVALID_TEMPLATE",
                "Template JSON missing 'components' object",
            );
        }
    };

    let entity = world.spawn_empty().id();
    let mut inserted = Vec::new();
    for (component_name, value) in components {
        match insert_component_by_reflect(world, entity, component_name, value) {
            McpResult::Success(_) => inserted.push(component_name.clone()),
            McpResult::Error { code, message } => {
                tracing::warn!(
                    component = component_name.as_str(),
                    code = code.as_str(),
                    message = message.as_str(),
                    "Skipping component during template load"
                );
            }
        }
    }

    if let Some((x, y, z)) = position
        && let Ok(mut entity_ref) = world.get_entity_mut(entity)
    {
        entity_ref.insert(Transform::from_xyz(x, y, z));
    }

    if let Some(parent_handle) = parent {
        if let Some(parent_entity) = resolve_entity(world, parent_handle) {
            if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
                use bevy::ecs::hierarchy::ChildOf;
                entity_ref.insert(ChildOf(parent_entity));
            }
        } else {
            return McpResult::error(
                "PARENT_NOT_FOUND",
                format!("Parent entity {parent_handle} not found"),
            );
        }
    }

    McpResult::success(json!({
        "handle": entity_to_uri(world, entity),
        "id": entity.index().index(),
        "template_name": template_name,
        "components_inserted": inserted,
    }))
}
