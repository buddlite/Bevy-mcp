use super::ecs_inspect::component_schema;
use super::*;

pub(crate) fn resource_list(world: &World) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();

    let mut resources = Vec::new();
    for registration in registry.iter() {
        if registration
            .data::<bevy::ecs::reflect::ReflectResource>()
            .is_some()
        {
            let type_info = registration.type_info();
            resources.push(json!({
                "name": type_info.type_path_table().short_path(),
                "type_path": type_info.type_path_table().path(),
            }));
        }
    }

    McpResult::success(json!({ "resources": resources }))
}

pub(crate) fn resource_get(world: &World, resource: &str) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();

    let registration = match registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == resource)
    {
        Some(r) => r,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REGISTERED",
                format!("Resource '{resource}' is not registered in the type registry"),
            );
        }
    };

    let type_id = registration.type_id();
    let reflect_from_ptr = match registration.data::<bevy::reflect::ReflectFromPtr>() {
        Some(rfp) => rfp,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REFLECTED",
                format!("Resource '{resource}' does not have ReflectFromPtr data"),
            );
        }
    };

    for (info, ptr) in world.iter_resources() {
        if info.type_id() == Some(type_id) {
            let reflected = unsafe { reflect_from_ptr.as_reflect(ptr) };
            let serializer = bevy::reflect::serde::ReflectSerializer::new(reflected, &registry);
            return match serde_json::to_value(&serializer) {
                Ok(value) => McpResult::success(json!({
                    "resource": resource,
                    "value": value,
                })),
                Err(e) => McpResult::error(
                    "SERIALIZATION_ERROR",
                    format!("Failed to serialize resource '{resource}': {e}"),
                ),
            };
        }
    }

    McpResult::error(
        "RESOURCE_NOT_PRESENT",
        format!("Resource '{resource}' is not present in the world"),
    )
}

pub(crate) fn resource_schema(world: &World, resource: &str) -> McpResult {
    component_schema(world, resource)
}

pub(crate) fn resource_update(world: &mut World, resource: &str, value: &Value) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == resource)
    {
        Some(r) => r,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REGISTERED",
                format!("Resource '{resource}' is not registered in the type registry"),
            );
        }
    };

    let component_id = match world.components().get_id(registration.type_id()) {
        Some(id) => id,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_PRESENT",
                format!("Resource '{resource}' is not registered as a component"),
            );
        }
    };

    let reflect_from_ptr = match registration.data::<bevy::reflect::ReflectFromPtr>() {
        Some(reflect_from_ptr) => reflect_from_ptr,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REFLECTED",
                format!("Resource '{resource}' does not have ReflectFromPtr data"),
            );
        }
    };

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json = wrapped.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    let reflected = match reflect_deserializer.deserialize(&mut deserializer) {
        Ok(value) => value,
        Err(error) => {
            return McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize resource '{resource}': {error}"),
            );
        }
    };

    let mut target = match world.get_resource_mut_by_id(component_id) {
        Some(target) => target,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_PRESENT",
                format!("Resource '{resource}' is not present in the world"),
            );
        }
    };
    let target = unsafe { reflect_from_ptr.as_reflect_mut(target.as_mut()) };
    target.apply(reflected.as_ref());

    McpResult::success(json!({
        "resource": resource,
        "status": "updated"
    }))
}

pub(crate) fn resource_remove(world: &mut World, resource: &str) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == resource)
    {
        Some(r) => r,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REGISTERED",
                format!("Resource '{resource}' is not registered in the type registry"),
            );
        }
    };

    if registration
        .data::<bevy::ecs::reflect::ReflectResource>()
        .is_none()
    {
        return McpResult::error(
            "RESOURCE_NOT_REFLECTED",
            format!("Resource '{resource}' does not have ReflectResource data"),
        );
    }

    let Some(component_id) = world.components().get_id(registration.type_id()) else {
        return McpResult::error(
            "RESOURCE_NOT_REGISTERED",
            format!("Resource '{resource}' has no world component id"),
        );
    };
    if !world.remove_resource_by_id(component_id) {
        return McpResult::error(
            "RESOURCE_NOT_PRESENT",
            format!("Resource '{resource}' is not present in the world"),
        );
    }

    McpResult::success(json!({
        "resource": resource,
        "status": "removed"
    }))
}
