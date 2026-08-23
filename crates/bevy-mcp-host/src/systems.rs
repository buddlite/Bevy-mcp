use bevy::prelude::*;
use serde::de::DeserializeSeed;
use serde_json::{Value, json};

use crate::deferred::{DeferredCommand, DeferredMcpCommands};
use crate::entity_handle::{entity_to_uri, resolve_entity, resolve_entity_by_index};
use crate::permissions::{McpPermissions, PermissionLevel};
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult, MutationOperation};

fn command_allowed(command: &McpCommand, permissions: &McpPermissions) -> bool {
    match command {
        McpCommand::Capabilities => true,
        McpCommand::EntitySpawn { .. }
        | McpCommand::EntityDespawn { .. }
        | McpCommand::ComponentInsert { .. }
        | McpCommand::ComponentUpdate { .. }
        | McpCommand::ComponentRemove { .. }
        | McpCommand::AtomicMutationBatch { .. }
        | McpCommand::ResourceUpdate { .. }
        | McpCommand::ResourceInsert { .. }
        | McpCommand::ResourceRemove { .. }
        | McpCommand::EntityReparent { .. }
        | McpCommand::EntityDuplicate { .. }
        | McpCommand::MeshSpawn { .. }
        | McpCommand::TemplateLoad { .. } => permissions.can_mutate(),
        McpCommand::TemplateSave { .. } => {
            permissions.level != crate::permissions::PermissionLevel::None
        }
        McpCommand::InputKey { .. }
        | McpCommand::InputMouseButton { .. }
        | McpCommand::InputMouseMove { .. }
        | McpCommand::InputAction { .. }
        | McpCommand::InputGamepad { .. }
        | McpCommand::PickAt { .. }
        | McpCommand::PointerClick { .. }
        | McpCommand::PointerDrag { .. }
        | McpCommand::PointerScroll { .. }
        | McpCommand::UiClick { .. }
        | McpCommand::UiType { .. } => permissions.can_inject_input(),
        McpCommand::RuntimeLaunch
        | McpCommand::RuntimeStop
        | McpCommand::RuntimeRestart
        | McpCommand::RuntimePause
        | McpCommand::RuntimeResume
        | McpCommand::RuntimeStep { .. }
        | McpCommand::RuntimeTimeScale { .. }
        | McpCommand::CameraFrameEntity { .. }
        | McpCommand::CameraSetTransform { .. }
        | McpCommand::CameraLookAt { .. }
        | McpCommand::AssetReload { .. } => permissions.can_control_runtime(),
        _ => permissions.level != crate::permissions::PermissionLevel::None,
    }
}

/// System that drains the ingress queue and defers all commands.
///
/// All commands (both reads and mutations) are deferred so that reads
/// see the result of mutations queued in the same frame.
/// Runs in PreUpdate::McpIngress.
pub fn ingress_system(world: &mut World) {
    let entries = {
        let ingress = world.resource::<McpIngressQueue>();
        ingress.drain()
    };

    for entry in entries {
        let allowed = {
            let permissions = world.resource::<McpPermissions>();
            command_allowed(&entry.command, permissions)
        };
        if !allowed {
            world.resource::<McpResultQueue>().push(McpResponse {
                request_id: entry.request_id,
                result: McpResult::error(
                    "PERMISSION_DENIED",
                    "The configured MCP permissions do not allow this operation",
                ),
            });
            continue;
        }

        if crate::interaction::is_interaction_command(&entry.command) {
            crate::interaction::enqueue_command(world, entry.request_id, &entry.command);
            continue;
        }

        match &entry.command {
            McpCommand::EntitySpawn { components } => {
                world
                    .resource_mut::<DeferredMcpCommands>()
                    .pending
                    .push(DeferredCommand::Spawn {
                        components: components.clone(),
                        result_id: entry.request_id,
                    });
            }
            McpCommand::EntityDespawn { entity: handle } => {
                if let Some(entity) = resolve_entity(world, handle) {
                    world.resource_mut::<DeferredMcpCommands>().pending.push(
                        DeferredCommand::Despawn {
                            entity,
                            result_id: entry.request_id,
                        },
                    );
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: entry.request_id,
                        result: McpResult::error(
                            "ENTITY_NOT_FOUND",
                            format!("Entity {handle} not found"),
                        ),
                    });
                }
            }
            McpCommand::ComponentInsert {
                entity: handle,
                component,
                value,
            } => {
                if let Some(entity) = resolve_entity(world, handle) {
                    world.resource_mut::<DeferredMcpCommands>().pending.push(
                        DeferredCommand::InsertComponent {
                            entity,
                            component: component.clone(),
                            value: value.clone(),
                            result_id: entry.request_id,
                        },
                    );
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: entry.request_id,
                        result: McpResult::error(
                            "ENTITY_NOT_FOUND",
                            format!("Entity {handle} not found"),
                        ),
                    });
                }
            }
            McpCommand::ComponentUpdate {
                entity: handle,
                component,
                value,
            } => {
                if let Some(entity) = resolve_entity(world, handle) {
                    world.resource_mut::<DeferredMcpCommands>().pending.push(
                        DeferredCommand::InsertComponent {
                            entity,
                            component: component.clone(),
                            value: value.clone(),
                            result_id: entry.request_id,
                        },
                    );
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: entry.request_id,
                        result: McpResult::error(
                            "ENTITY_NOT_FOUND",
                            format!("Entity {handle} not found"),
                        ),
                    });
                }
            }
            McpCommand::ComponentRemove {
                entity: handle,
                component,
            } => {
                if let Some(entity) = resolve_entity(world, handle) {
                    world.resource_mut::<DeferredMcpCommands>().pending.push(
                        DeferredCommand::RemoveComponent {
                            entity,
                            component: component.clone(),
                            result_id: entry.request_id,
                        },
                    );
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: entry.request_id,
                        result: McpResult::error(
                            "ENTITY_NOT_FOUND",
                            format!("Entity {handle} not found"),
                        ),
                    });
                }
            }
            McpCommand::AtomicMutationBatch {
                operations,
                dry_run,
            } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::AtomicMutationBatch {
                        operations: operations.clone(),
                        dry_run: *dry_run,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::InputKey { key, pressed } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::InputKey {
                        key: key.clone(),
                        pressed: *pressed,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::InputMouseButton {
                button, pressed, ..
            } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::InputMouseButton {
                        button: button.clone(),
                        pressed: *pressed,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::InputMouseMove { x, y } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::InputMouseMove {
                        x: *x,
                        y: *y,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::InputGamepad { button, pressed } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::InputGamepad {
                        button: button.clone(),
                        pressed: *pressed,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::UiType { entity, text } => {
                world
                    .resource_mut::<DeferredMcpCommands>()
                    .pending
                    .push(DeferredCommand::UiType {
                        entity: entity.clone(),
                        text: text.clone(),
                        result_id: entry.request_id,
                    });
            }
            McpCommand::CameraFrameEntity { entity, margin } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::CameraFrameEntity {
                        entity: entity.clone(),
                        margin: *margin,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::CameraSetTransform { x, y, z } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::CameraSetTransform {
                        x: *x,
                        y: *y,
                        z: *z,
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::CameraLookAt { entity } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::CameraLookAt {
                        entity: entity.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::ResourceUpdate { resource, value } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::ResourceUpdate {
                        resource: resource.clone(),
                        value: value.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::ResourceInsert { resource, value } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::ResourceUpdate {
                        resource: resource.clone(),
                        value: value.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::ResourceRemove { resource } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::ResourceRemove {
                        resource: resource.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::EntityReparent { entity, parent } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::EntityReparent {
                        entity: entity.clone(),
                        parent: parent.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::EntityDuplicate { entity } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::EntityDuplicate {
                        entity: entity.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::MeshSpawn {
                shape,
                size,
                radius,
                color,
                metallic,
                roughness,
                position,
                parent,
            } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::MeshSpawn {
                        shape: shape.clone(),
                        size: *size,
                        radius: *radius,
                        color: *color,
                        metallic: *metallic,
                        roughness: *roughness,
                        position: *position,
                        parent: parent.clone(),
                        result_id: entry.request_id,
                    },
                );
            }
            McpCommand::TemplateLoad {
                name,
                path,
                parent,
                position,
            } => {
                world.resource_mut::<DeferredMcpCommands>().pending.push(
                    DeferredCommand::TemplateLoad {
                        name: name.clone(),
                        path: path.clone(),
                        parent: parent.clone(),
                        position: *position,
                        result_id: entry.request_id,
                    },
                );
            }
            _ => {
                world
                    .resource_mut::<DeferredMcpCommands>()
                    .pending
                    .push(DeferredCommand::Read {
                        command: entry.command.clone(),
                        result_id: entry.request_id,
                    });
            }
        }
    }
}

pub fn runtime_system(mut registry: ResMut<McpRegistry>, mut time: ResMut<Time<Virtual>>) {
    if registry.paused {
        if registry.step_remaining > 0 {
            registry.step_remaining -= 1;
            time.unpause();
        } else {
            time.pause();
        }
    } else {
        time.unpause();
        time.set_relative_speed(registry.time_scale as f32);
    }
}

pub fn diagnostics_system(mut registry: ResMut<McpRegistry>) {
    registry.frame += 1;
}

pub fn deferred_apply_system(world: &mut World) {
    let pending = {
        let mut deferred = world.resource_mut::<DeferredMcpCommands>();
        deferred.pending.drain(..).collect::<Vec<_>>()
    };

    'commands: for cmd in pending {
        match cmd {
            DeferredCommand::Spawn {
                components,
                result_id,
            } => {
                let entity = world.spawn_empty().id();
                let mut inserted = Vec::with_capacity(components.len());
                for (component, value) in components {
                    match insert_component_by_reflect(world, entity, &component, &value) {
                        McpResult::Success(_) => inserted.push(component),
                        McpResult::Error { code, message } => {
                            let _ = world.despawn(entity);
                            world.resource::<McpResultQueue>().push(McpResponse {
                                request_id: result_id,
                                result: McpResult::error(
                                    "SPAWN_FAILED",
                                    format!("Could not insert component '{component}' ({code}): {message}"),
                                ),
                            });
                            continue 'commands;
                        }
                    }
                }
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result: McpResult::success(json!({
                        "handle": entity_to_uri(entity),
                        "id": entity.index().index(),
                        "components": inserted,
                    })),
                });
            }
            DeferredCommand::Despawn { entity, result_id } => {
                world.despawn(entity);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result: McpResult::success(json!({ "despawned": true })),
                });
            }
            DeferredCommand::InsertComponent {
                entity,
                component,
                value,
                result_id,
            } => {
                let result = insert_component_by_reflect(world, entity, &component, &value);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::RemoveComponent {
                entity,
                component,
                result_id,
            } => {
                let result = remove_component_by_reflect(world, entity, &component);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::AtomicMutationBatch {
                operations,
                dry_run,
                result_id,
            } => {
                let result = apply_atomic_mutation_batch(world, &operations, dry_run);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::InputKey {
                key,
                pressed,
                result_id,
            } => {
                if let Some(keycode) = parse_keycode(&key) {
                    if world.get_resource::<ButtonInput<KeyCode>>().is_some() {
                        if pressed {
                            world.resource_mut::<ButtonInput<KeyCode>>().press(keycode);
                        } else {
                            world
                                .resource_mut::<ButtonInput<KeyCode>>()
                                .release(keycode);
                        }
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::success(json!({ "key": key, "pressed": pressed })),
                        });
                    } else {
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::error(
                                "INPUT_NOT_AVAILABLE",
                                "ButtonInput<KeyCode> resource not found. Add InputPlugin to your app.",
                            ),
                        });
                    }
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: result_id,
                        result: McpResult::error("INVALID_KEY", format!("Unknown key: {key}")),
                    });
                }
            }
            DeferredCommand::InputMouseButton {
                button,
                pressed,
                result_id,
            } => {
                if let Some(mouse_button) = parse_mouse_button(&button) {
                    if world.get_resource::<ButtonInput<MouseButton>>().is_some() {
                        if pressed {
                            world
                                .resource_mut::<ButtonInput<MouseButton>>()
                                .press(mouse_button);
                        } else {
                            world
                                .resource_mut::<ButtonInput<MouseButton>>()
                                .release(mouse_button);
                        }
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::success(
                                json!({ "button": button, "pressed": pressed }),
                            ),
                        });
                    } else {
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::error(
                                "INPUT_NOT_AVAILABLE",
                                "ButtonInput<MouseButton> resource not found. Add InputPlugin to your app.",
                            ),
                        });
                    }
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: result_id,
                        result: McpResult::error(
                            "INVALID_BUTTON",
                            format!("Unknown mouse button: {button}"),
                        ),
                    });
                }
            }
            DeferredCommand::InputMouseMove { x, y, result_id } => {
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result: McpResult::error(
                        "NOT_IMPLEMENTED",
                        format!("Mouse motion injection is not implemented ({x}, {y})"),
                    ),
                });
            }
            DeferredCommand::InputGamepad {
                button,
                pressed,
                result_id,
            } => {
                if let Some(gamepad_button) = parse_gamepad_button(&button) {
                    if world.get_resource::<ButtonInput<GamepadButton>>().is_some() {
                        if pressed {
                            world
                                .resource_mut::<ButtonInput<GamepadButton>>()
                                .press(gamepad_button);
                        } else {
                            world
                                .resource_mut::<ButtonInput<GamepadButton>>()
                                .release(gamepad_button);
                        }
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::success(
                                json!({ "button": button, "pressed": pressed }),
                            ),
                        });
                    } else {
                        world.resource::<McpResultQueue>().push(McpResponse {
                            request_id: result_id,
                            result: McpResult::error(
                                "INPUT_NOT_AVAILABLE",
                                "ButtonInput<GamepadButton> resource not found. Add InputPlugin to your app.",
                            ),
                        });
                    }
                } else {
                    world.resource::<McpResultQueue>().push(McpResponse {
                        request_id: result_id,
                        result: McpResult::error(
                            "INVALID_BUTTON",
                            format!("Unknown gamepad button: {button}"),
                        ),
                    });
                }
            }
            DeferredCommand::UiType {
                entity,
                text,
                result_id,
            } => {
                let result = ui_type_apply(world, &entity, &text);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::CameraFrameEntity {
                entity,
                margin,
                result_id,
            } => {
                let result = camera_frame_entity_apply(world, &entity, margin);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::CameraSetTransform { x, y, z, result_id } => {
                let result = camera_set_transform_apply(world, x, y, z);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::CameraLookAt { entity, result_id } => {
                let result = camera_look_at_apply(world, &entity);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::ResourceUpdate {
                resource,
                value,
                result_id,
            } => {
                let result = resource_update(world, &resource, &value);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::ResourceRemove {
                resource,
                result_id,
            } => {
                let result = resource_remove(world, &resource);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::EntityReparent {
                entity,
                parent,
                result_id,
            } => {
                let result = entity_reparent(world, &entity, parent.as_ref());
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::EntityDuplicate { entity, result_id } => {
                let result = entity_duplicate(world, &entity);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::MeshSpawn {
                shape,
                size,
                radius,
                color,
                metallic,
                roughness,
                position,
                parent,
                result_id,
            } => {
                let result = mesh_spawn_apply(
                    world,
                    &shape,
                    size,
                    radius,
                    color,
                    metallic,
                    roughness,
                    position,
                    parent.as_ref(),
                );
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::TemplateLoad {
                name,
                path,
                parent,
                position,
                result_id,
            } => {
                let result =
                    template_load_apply(world, &name, path.as_deref(), parent.as_ref(), position);
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
            DeferredCommand::Read { command, result_id } => {
                let result = world.resource_scope(|world, mut registry: Mut<McpRegistry>| {
                    execute_command(world, &command, &mut registry)
                });
                world.resource::<McpResultQueue>().push(McpResponse {
                    request_id: result_id,
                    result,
                });
            }
        }
    }
}

fn mutation_operation_name(operation: &MutationOperation) -> &'static str {
    match operation {
        MutationOperation::ComponentInsert { .. } => "component_insert",
        MutationOperation::ComponentUpdate { .. } => "component_update",
        MutationOperation::ComponentRemove { .. } => "component_remove",
        MutationOperation::ResourceUpdate { .. } => "resource_update",
    }
}

fn transaction_validation_error(
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

fn validate_component_write(
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

fn validate_component_remove(
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

fn validate_resource_write(world: &World, resource: &str, value: &Value) -> Result<(), McpResult> {
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

fn apply_atomic_mutation_batch(
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

fn insert_component_by_reflect(
    world: &mut World,
    entity: Entity,
    component: &str,
    value: &Value,
) -> McpResult {
    tracing::debug!(component, ?value, "insert_component_by_reflect");

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == component)
    {
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

fn remove_component_by_reflect(world: &mut World, entity: Entity, component: &str) -> McpResult {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();

    let registration = match registry
        .iter()
        .find(|r| r.type_info().type_path_table().short_path() == component)
    {
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

fn entity_reparent(
    world: &mut World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
    parent_handle: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
) -> McpResult {
    use bevy::ecs::hierarchy::ChildOf;

    let entity = match resolve_entity(world, entity_handle) {
        Some(e) => e,
        None => {
            return McpResult::error(
                "ENTITY_NOT_FOUND",
                format!("Entity {entity_handle} not found"),
            );
        }
    };

    if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
        entity_ref.remove::<ChildOf>();
    }

    if let Some(parent_handle) = parent_handle {
        let parent = match resolve_entity(world, parent_handle) {
            Some(e) => e,
            None => {
                return McpResult::error(
                    "ENTITY_NOT_FOUND",
                    format!("Parent entity {parent_handle} not found"),
                );
            }
        };
        if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(ChildOf(parent));
        }
        McpResult::success(json!({
            "reparented": entity_to_uri(entity),
            "new_parent": entity_to_uri(parent)
        }))
    } else {
        McpResult::success(json!({ "reparented": entity_to_uri(entity), "new_parent": null }))
    }
}

fn entity_duplicate(
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

fn execute_command(world: &World, command: &McpCommand, registry: &mut McpRegistry) -> McpResult {
    match command {
        McpCommand::WorldSummary => world_summary(world),
        McpCommand::Capabilities => capabilities(world),
        McpCommand::WorldContextScan => world_context_scan(world, registry),
        McpCommand::EntityQuery {
            with_components,
            without_components,
            include,
            limit,
        } => entity_query(world, with_components, without_components, include, *limit),
        McpCommand::EntityGet { entity } => entity_get(world, entity),
        McpCommand::ComponentGet { entity, component } => component_get(world, entity, component),
        McpCommand::ComponentSchema { component } => component_schema(world, component),
        McpCommand::ResourceList => resource_list(world),
        McpCommand::ResourceGet { resource } => resource_get(world, resource),
        McpCommand::ResourceSchema { resource } => resource_schema(world, resource),
        McpCommand::EntitySpawn { components } => entity_spawn(components),
        McpCommand::EntityDespawn { entity } => entity_despawn(world, entity),
        McpCommand::ComponentInsert {
            entity,
            component,
            value,
        } => component_insert(world, entity, component, value),
        McpCommand::ComponentUpdate {
            entity,
            component,
            value,
        } => component_update(world, entity, component, value),
        McpCommand::ComponentRemove { entity, component } => {
            component_remove(world, entity, component)
        }
        McpCommand::AtomicMutationBatch { .. } => {
            McpResult::error("INTERNAL", "Atomic mutation batches should be deferred")
        }
        McpCommand::RuntimePause => runtime_pause(registry),
        McpCommand::RuntimeResume => runtime_resume(registry),
        McpCommand::RuntimeStep { frames } => runtime_step(registry, *frames),
        McpCommand::RuntimeTimeScale { scale } => runtime_time_scale(registry, *scale),
        McpCommand::InputKey { key, pressed } => McpResult::error(
            "INTERNAL",
            format!(
                "InputKey should be deferred, not executed directly (key={key}, pressed={pressed})"
            ),
        ),
        McpCommand::InputMouseButton {
            button, pressed, ..
        } => McpResult::error(
            "INTERNAL",
            format!("InputMouseButton should be deferred (button={button}, pressed={pressed})"),
        ),
        McpCommand::InputMouseMove { x, y } => McpResult::error(
            "INTERNAL",
            format!("InputMouseMove should be deferred ({x}, {y})"),
        ),
        McpCommand::InputAction { action, strength } => McpResult::error(
            "INTERNAL",
            format!("InputAction should be deferred (action={action}, strength={strength})"),
        ),
        McpCommand::InputGamepad { button, pressed } => McpResult::error(
            "INTERNAL",
            format!("InputGamepad should be deferred (button={button}, pressed={pressed})"),
        ),
        McpCommand::PickAt { .. }
        | McpCommand::PointerClick { .. }
        | McpCommand::PointerDrag { .. }
        | McpCommand::PointerScroll { .. } => McpResult::error(
            "INTERNAL",
            "Pointer interaction commands must be handled by the interaction state machine",
        ),
        McpCommand::Logs { level, limit } => logs(world, level, *limit),
        McpCommand::Diagnostics => diagnostics(world, registry),
        McpCommand::Hierarchy { root, max_depth } => hierarchy(world, root.as_ref(), *max_depth),
        McpCommand::ObserveEvents { event_type, limit } => {
            observe_events(world, event_type, *limit)
        }
        McpCommand::UiQuery { root, max_depth } => ui_query(world, root.as_ref(), *max_depth),
        McpCommand::ListPlugins => list_plugins(world),
        McpCommand::CaptureGame => capture_game(world),
        McpCommand::CameraFrameEntity { .. } => {
            McpResult::error("INTERNAL", "Camera framing should be deferred")
        }
        McpCommand::CameraInspect => camera_inspect(world),
        McpCommand::CameraSetTransform { .. } | McpCommand::CameraLookAt { .. } => {
            McpResult::error("INTERNAL", "Camera mutation should be deferred")
        }
        McpCommand::CaptureCamera => capture_game(world),
        McpCommand::UiInspect { entity } => ui_inspect(world, entity),
        McpCommand::UiClick { .. } => McpResult::error(
            "INTERNAL",
            "UI click should be handled by the interaction state machine",
        ),
        McpCommand::UiType { .. } => McpResult::error("INTERNAL", "UI type should be deferred"),
        McpCommand::PlaytestRun { steps } => playtest_run(world, steps),
        McpCommand::Assert { assertion } => assert_condition(world, assertion),
        McpCommand::RuntimeLaunch | McpCommand::RuntimeStop | McpCommand::RuntimeRestart => {
            McpResult::error(
                "NOT_IMPLEMENTED",
                "Application lifecycle is managed by the embedding application",
            )
        }
        McpCommand::OperationStatus { operation_id } => {
            operation_status(world, operation_id.as_deref())
        }
        McpCommand::OperationCancel { operation_id } => operation_cancel(world, operation_id),
        McpCommand::AssetList { filter } => asset_list(world, filter.as_deref()),
        McpCommand::AssetGet { path } => asset_get(world, path),
        McpCommand::AssetStatus { path } => asset_status(world, path),
        McpCommand::AssetReload { path } => asset_reload(world, path),
        McpCommand::MeshSpawn { .. } => McpResult::error(
            "INTERNAL",
            "MeshSpawn should be deferred, not executed directly",
        ),
        McpCommand::TemplateSave { entity, name, path } => {
            template_save(world, entity, name, path.as_deref())
        }
        McpCommand::TemplateLoad { .. } => McpResult::error(
            "INTERNAL",
            "TemplateLoad should be deferred, not executed directly",
        ),
        McpCommand::ResourceUpdate { resource, value: _ } => McpResult::error(
            "INTERNAL",
            format!("ResourceUpdate should be deferred (resource={resource})"),
        ),
        McpCommand::ResourceInsert { resource, value: _ } => McpResult::error(
            "INTERNAL",
            format!("ResourceInsert should be deferred (resource={resource})"),
        ),
        McpCommand::ResourceRemove { resource } => McpResult::error(
            "INTERNAL",
            format!("ResourceRemove should be deferred (resource={resource})"),
        ),
        McpCommand::EntityReparent { entity, .. } => McpResult::error(
            "INTERNAL",
            format!("EntityReparent should be deferred (entity={entity})"),
        ),
        McpCommand::EntityDuplicate { entity } => McpResult::error(
            "INTERNAL",
            format!("EntityDuplicate should be deferred (entity={entity})"),
        ),
    }
}

fn capability(implemented: bool, available: bool, allowed: bool) -> Value {
    json!({
        "implemented": implemented,
        "available": available,
        "allowed": allowed,
        "operational": implemented && available && allowed,
    })
}

fn capabilities(world: &World) -> McpResult {
    let permissions = world.resource::<McpPermissions>();
    let can_read = permissions.level != PermissionLevel::None;
    let can_mutate = permissions.can_mutate();
    let can_input = permissions.can_inject_input();
    let can_runtime = permissions.can_control_runtime();
    let can_build = permissions.can_build();
    let permission_level = match permissions.level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
        PermissionLevel::Full => "full",
    };

    let key_input_available = world.contains_resource::<ButtonInput<KeyCode>>();
    let mouse_button_available = world.contains_resource::<ButtonInput<MouseButton>>();
    let gamepad_button_available = world.contains_resource::<ButtonInput<GamepadButton>>();
    let pointer_available = crate::interaction::pointer_available(world);
    let camera_available = active_camera_entity(world).is_some();
    let camera_frame_available = active_camera_entity(world).is_some_and(|camera| {
        matches!(
            world.get::<bevy::camera::Projection>(camera),
            Some(
                bevy::camera::Projection::Perspective(_)
                    | bevy::camera::Projection::Orthographic(_)
            )
        )
    });
    let renderer_available = world
        .get_resource::<bevy::render::renderer::RenderDevice>()
        .is_some();
    let primary_window_available = renderer_available
        && world
            .iter_entities()
            .any(|entity| entity.contains::<bevy::window::PrimaryWindow>());
    let camera_target_available = renderer_available
        && world
            .iter_entities()
            .any(|entity| entity.contains::<bevy::camera::RenderTarget>());
    let ui_capture_available = renderer_available
        && world
            .get_resource::<crate::agent_api::McpCaptureTargets>()
            .and_then(|targets| targets.ui_target())
            .is_some();
    let mesh_spawn_available = world.contains_resource::<Assets<Mesh>>()
        && world.contains_resource::<Assets<bevy::pbr::StandardMaterial>>();
    let reflected_types_available = world.contains_resource::<AppTypeRegistry>();
    let asset_server_available = world.contains_resource::<bevy::asset::AssetServer>();
    let tracker_available = world.contains_resource::<crate::change_tracking::WorldChangeTracker>();
    let system_access_available =
        world.contains_resource::<crate::agent_api::McpSystemAccessRegistry>();
    let timings_available = world.contains_resource::<crate::agent_api::McpSystemTimings>();
    let debugger_available = world.contains_resource::<crate::debugger::McpDebugger>();
    let checkpoints_available = world
        .contains_resource::<crate::checkpoint::McpCheckpointRegistry>()
        && world.contains_resource::<crate::checkpoint::McpCheckpointStore>();
    let recorder_available = world.contains_resource::<crate::checkpoint::McpRecorder>();

    McpResult::success(json!({
        "schema_version": 2,
        "connected": true,
        "permissions": {
            "level": permission_level,
            "ecs_mutation": can_mutate,
            "input": can_input,
            "runtime_control": can_runtime,
            "build": can_build,
        },
        "transport": {
            "concurrent_correlated_requests": capability(true, true, can_read),
        },
        "ecs": {
            "inspect": capability(true, true, can_read),
            "query": capability(true, true, can_read),
            "hierarchy": capability(true, true, can_read),
            "reflection": capability(true, reflected_types_available, can_read),
            "mutate": capability(true, reflected_types_available, can_mutate),
            "atomic_mutation_batch": capability(true, reflected_types_available, can_mutate),
            "entity_duplicate": capability(false, false, false),
        },
        "runtime": {
            "pause": capability(true, true, can_runtime),
            "resume": capability(true, true, can_runtime),
            "step": capability(true, true, can_runtime),
            "time_scale": capability(true, true, can_runtime),
            "launch": capability(false, false, false),
            "stop": capability(false, false, false),
            "restart": capability(false, false, false),
        },
        "input": {
            "key": capability(true, key_input_available, can_input),
            "mouse_button": capability(true, mouse_button_available, can_input),
            "mouse_move": capability(true, pointer_available, can_input),
            "action": capability(false, false, false),
            "gamepad_button": capability(true, gamepad_button_available, can_input),
        },
        "interaction": {
            "pick_at": capability(true, pointer_available, can_input),
            "pointer_move": capability(true, pointer_available, can_input),
            "pointer_click": capability(true, pointer_available, can_input),
            "pointer_drag": capability(true, pointer_available, can_input),
            "pointer_scroll": capability(true, pointer_available, can_input),
        },
        "capture": {
            "viewport": capability(true, primary_window_available, can_read),
            "camera_target": capability(true, camera_target_available, can_read),
            "ui_only": capability(true, ui_capture_available, can_read),
        },
        "diagnostics": {
            "logs": capability(true, true, can_read),
            "events": capability(true, true, can_read),
            "change_tracking": capability(true, tracker_available, can_read),
            "system_access": capability(true, system_access_available, can_read),
            "system_timings": capability(true, timings_available, can_read),
        },
        "debugger": {
            "watchpoints": capability(true, debugger_available, can_read),
            "playtests": capability(true, debugger_available, can_runtime && can_input),
            "checkpoint_create": capability(true, checkpoints_available, can_read),
            "checkpoint_restore": capability(true, checkpoints_available, can_mutate),
            "recording": capability(true, recorder_available, can_read),
            "replay": capability(true, recorder_available, can_runtime && can_input),
        },
        "ui": {
            "query": capability(true, true, can_read),
            "inspect": capability(true, true, can_read),
            "click": capability(true, pointer_available, can_input),
            "type_text": capability(true, true, can_input),
        },
        "camera": {
            "list": capability(true, true, can_read),
            "inspect": capability(true, true, can_read),
            "frame_entity": capability(true, camera_frame_available, can_runtime),
            "set_transform": capability(true, camera_available, can_runtime),
            "look_at": capability(true, camera_available, can_runtime),
        },
        "assets": {
            "list": capability(false, false, false),
            "inspect": capability(true, asset_server_available, can_read),
            "status": capability(true, asset_server_available, can_read),
            "reload": capability(true, asset_server_available, can_runtime),
        },
        "procedural": {
            "mesh_spawn": capability(true, mesh_spawn_available, can_mutate),
            "template_save": capability(true, reflected_types_available, can_read),
            "template_load": capability(true, reflected_types_available, can_mutate),
        },
        "build": {
            "check": capability(false, false, can_build),
            "build": capability(false, false, can_build),
            "test": capability(false, false, can_build),
        },
        "deprecations": [
            {
                "tool": "capture_game",
                "status": "deprecated_alias",
                "functional": true,
                "replacement": "capture_viewport"
            },
            {
                "tool": "capture_camera",
                "status": "deprecated_alias",
                "functional": true,
                "replacement": "capture_viewport"
            },
            {
                "tool": "playtest_run",
                "status": "deprecated_unavailable",
                "functional": false,
                "replacement": "playtest_start"
            }
        ]
    }))
}

fn world_summary(world: &World) -> McpResult {
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

fn world_context_scan(world: &World, registry: &McpRegistry) -> McpResult {
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

fn entity_query(
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

fn entity_get(world: &World, handle: &bevy_mcp_core::entity_handle::EntityHandle) -> McpResult {
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

fn component_get(
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

fn component_schema(world: &World, component: &str) -> McpResult {
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

fn resource_list(world: &World) -> McpResult {
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

fn resource_get(world: &World, resource: &str) -> McpResult {
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

fn resource_schema(world: &World, resource: &str) -> McpResult {
    component_schema(world, resource)
}

fn resource_update(world: &mut World, resource: &str, value: &Value) -> McpResult {
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

fn resource_remove(world: &mut World, resource: &str) -> McpResult {
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

fn entity_spawn(_components: &[(String, Value)]) -> McpResult {
    McpResult::error("INTERNAL", "Entity spawn not yet wired to deferred queue")
}

fn entity_despawn(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    McpResult::error("INTERNAL", "Entity despawn not yet wired to deferred queue")
}

fn component_insert(
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

fn component_update(
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

fn component_remove(
    _world: &World,
    _entity: &bevy_mcp_core::entity_handle::EntityHandle,
    _component: &str,
) -> McpResult {
    McpResult::error(
        "INTERNAL",
        "Component remove not yet wired to deferred queue",
    )
}

fn runtime_pause(registry: &mut McpRegistry) -> McpResult {
    registry.paused = true;
    McpResult::success(json!({ "paused": true }))
}

fn runtime_resume(registry: &mut McpRegistry) -> McpResult {
    registry.paused = false;
    registry.step_remaining = 0;
    McpResult::success(json!({ "paused": false }))
}

fn runtime_step(registry: &mut McpRegistry, frames: u32) -> McpResult {
    registry.paused = true;
    registry.step_remaining = frames;
    McpResult::success(json!({ "paused": true, "step_frames": frames }))
}

fn runtime_time_scale(registry: &mut McpRegistry, scale: f64) -> McpResult {
    registry.time_scale = scale;
    McpResult::success(json!({ "time_scale": scale }))
}

fn logs(world: &World, level: &Option<String>, limit: u32) -> McpResult {
    let log_capture = match world.get_resource::<crate::log_capture::LogCapture>() {
        Some(lc) => lc,
        None => {
            return McpResult::error(
                "LOG_CAPTURE_NOT_AVAILABLE",
                "LogCapture resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    let entries = log_capture.get_entries(level.as_deref(), limit as usize);
    let log_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "level": entry.level,
                "message": entry.message,
                "target": entry.target,
                "timestamp": entry.timestamp,
            })
        })
        .collect();

    McpResult::success(json!({
        "logs": log_entries,
        "count": log_entries.len(),
    }))
}

fn diagnostics(world: &World, registry: &McpRegistry) -> McpResult {
    let entity_count = world.iter_entities().count();
    McpResult::success(json!({
        "frame": registry.frame,
        "entity_count": entity_count,
        "paused": registry.paused,
        "time_scale": registry.time_scale,
    }))
}

fn hierarchy(
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

fn observe_events(world: &World, event_type: &Option<String>, limit: u32) -> McpResult {
    let event_capture = match world.get_resource::<crate::event_capture::EventCapture>() {
        Some(ec) => ec,
        None => {
            return McpResult::error(
                "EVENT_CAPTURE_NOT_AVAILABLE",
                "EventCapture resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    let entries = event_capture.get_events(event_type.as_deref(), limit as usize);
    let event_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "event_type": entry.event_type,
                "data": entry.data,
                "timestamp": entry.timestamp,
            })
        })
        .collect();

    McpResult::success(json!({
        "events": event_entries,
        "count": event_entries.len(),
    }))
}

fn ui_query(
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
                "handle": entity_to_uri(entity),
                "id": entity.index().index(),
                "children": [],
                "truncated": true,
            });
        }

        let mut node_info = json!({
            "handle": entity_to_uri(entity),
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

fn ui_inspect(world: &World, handle: &bevy_mcp_core::entity_handle::EntityHandle) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    let mut info = json!({
        "handle": entity_to_uri(entity),
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

fn active_camera_entity(world: &World) -> Option<Entity> {
    world
        .iter_entities()
        .find(|entity| {
            entity
                .get::<bevy::prelude::Camera>()
                .is_some_and(|camera| camera.is_active)
                && entity.get::<Transform>().is_some()
        })
        .map(|entity| entity.id())
        .or_else(|| {
            world
                .iter_entities()
                .find(|entity| {
                    entity.get::<bevy::prelude::Camera>().is_some()
                        && entity.get::<Transform>().is_some()
                })
                .map(|entity| entity.id())
        })
}

fn ui_type_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    text: &str,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let Some(mut editable) = world.get_mut::<bevy::text::EditableText>(entity) else {
        return McpResult::error(
            "NOT_EDITABLE_TEXT",
            format!("Entity {handle} does not have an EditableText component"),
        );
    };
    editable.queue_edit(bevy::text::TextEdit::Insert(text.into()));
    McpResult::success(json!({
        "entity": entity_to_uri(entity),
        "status": "queued",
        "text": text,
    }))
}

fn target_position(world: &World, entity: Entity) -> Option<Vec3> {
    current_global_transform(world, entity).map(|transform| transform.translation())
}

fn current_global_transform(world: &World, entity: Entity) -> Option<GlobalTransform> {
    fn resolve(world: &World, entity: Entity, depth: u32) -> Option<GlobalTransform> {
        if depth > 128 {
            return None;
        }
        if let Some(local) = world.get::<Transform>(entity).copied() {
            if let Some(parent) = world.get::<bevy::ecs::hierarchy::ChildOf>(entity) {
                let parent_global = resolve(world, parent.parent(), depth + 1)?;
                Some(parent_global.mul_transform(local))
            } else {
                Some(GlobalTransform::from(local))
            }
        } else {
            world.get::<GlobalTransform>(entity).copied()
        }
    }

    resolve(world, entity, 0)
}

fn set_world_transform(
    world: &mut World,
    entity: Entity,
    desired_world: Transform,
) -> Result<(), McpResult> {
    let parent = world
        .get::<bevy::ecs::hierarchy::ChildOf>(entity)
        .map(|parent| parent.parent());
    let local = if let Some(parent) = parent {
        let Some(parent_global) = current_global_transform(world, parent) else {
            return Err(McpResult::error(
                "PARENT_TRANSFORM_NOT_READY",
                "Camera parent transform could not be resolved",
            ));
        };
        GlobalTransform::from(desired_world).reparented_to(&parent_global)
    } else {
        desired_world
    };

    let Some(mut transform) = world.get_mut::<Transform>(entity) else {
        return Err(McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera has no Transform",
        ));
    };
    *transform = local;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AggregateBounds {
    min: Vec3,
    max: Vec3,
    bounded_entities: usize,
}

impl AggregateBounds {
    fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    fn corners(self) -> [Vec3; 8] {
        let min = self.min;
        let max = self.max;
        [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(max.x, max.y, max.z),
        ]
    }
}

fn aggregate_world_bounds(world: &World, root: Entity) -> Result<AggregateBounds, McpResult> {
    use bevy::camera::primitives::Aabb;

    let mut stack = vec![root];
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut bounded_entities = 0usize;

    while let Some(entity) = stack.pop() {
        if let Some(children) = world.get::<bevy::ecs::hierarchy::Children>(entity) {
            stack.extend(children.iter());
        }

        let Some(aabb) = world.get::<Aabb>(entity) else {
            continue;
        };
        let Some(global) = current_global_transform(world, entity) else {
            return Err(McpResult::error(
                "BOUNDS_NOT_READY",
                format!(
                    "Entity {} has an Aabb but its world transform is not available",
                    entity_to_uri(entity)
                ),
            ));
        };

        let center: Vec3 = aabb.center.into();
        let half_extents: Vec3 = aabb.half_extents.into();
        for x in [-half_extents.x, half_extents.x] {
            for y in [-half_extents.y, half_extents.y] {
                for z in [-half_extents.z, half_extents.z] {
                    let point = global.transform_point(center + Vec3::new(x, y, z));
                    if !point.is_finite() {
                        return Err(McpResult::error(
                            "INVALID_BOUNDS",
                            "Encountered a non-finite transformed Aabb corner",
                        ));
                    }
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                }
            }
        }
        bounded_entities += 1;
    }

    if bounded_entities == 0 {
        return Err(McpResult::error(
            "NO_BOUNDS",
            "Target entity and its descendants do not contain any Aabb components",
        ));
    }

    Ok(AggregateBounds {
        min: minimum,
        max: maximum,
        bounded_entities,
    })
}

fn framing_basis(camera_world: GlobalTransform, center: Vec3) -> (Vec3, Vec3, Vec3, f32) {
    let camera_position = camera_world.translation();
    let offset = camera_position - center;
    let current_distance = offset.length();
    let direction = offset.try_normalize().unwrap_or_else(|| {
        (camera_world.rotation() * Vec3::Z)
            .try_normalize()
            .unwrap_or(Vec3::Z)
    });
    let forward = -direction;
    let preferred_up = camera_world.rotation() * Vec3::Y;
    let fallback_up = if forward.dot(Vec3::Y).abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = forward
        .cross(preferred_up)
        .try_normalize()
        .or_else(|| forward.cross(fallback_up).try_normalize())
        .unwrap_or(Vec3::X);
    let up = right.cross(forward).try_normalize().unwrap_or(fallback_up);
    (direction, right, up, current_distance)
}

fn camera_set_transform_apply(world: &mut World, x: f64, y: f64, z: f64) -> McpResult {
    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(global) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    let mut desired = global.compute_transform();
    desired.translation = Vec3::new(x as f32, y as f32, z as f32);
    if let Err(error) = set_world_transform(world, camera, desired) {
        return error;
    }
    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "position": {"x": x, "y": y, "z": z},
        "space": "world",
    }))
}

fn camera_look_at_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    let target = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let Some(target_position) = target_position(world, target) else {
        return McpResult::error(
            "NO_TRANSFORM",
            format!("Entity {handle} has no resolvable world transform"),
        );
    };
    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(global) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    if global.translation().distance_squared(target_position) <= f32::EPSILON {
        return McpResult::error(
            "CAMERA_AT_TARGET",
            "Camera and target occupy the same position",
        );
    }
    let mut desired = global.compute_transform();
    desired.look_at(target_position, Vec3::Y);
    if let Err(error) = set_world_transform(world, camera, desired) {
        return error;
    }
    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "target": entity_to_uri(target),
    }))
}

fn camera_frame_entity_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    margin: f64,
) -> McpResult {
    if !margin.is_finite() || !(0.0..=2.0).contains(&margin) {
        return McpResult::error(
            "INVALID_MARGIN",
            "margin must be a finite value between 0.0 and 2.0",
        );
    }

    let target = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let bounds = match aggregate_world_bounds(world, target) {
        Ok(bounds) => bounds,
        Err(error) => return error,
    };
    let center = bounds.center();

    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(camera_world) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    let Some(projection) = world.get::<bevy::camera::Projection>(camera) else {
        return McpResult::error(
            "CAMERA_PROJECTION_NOT_AVAILABLE",
            "Active camera does not have a Projection component",
        );
    };

    #[derive(Clone, Copy)]
    enum ProjectionData {
        Perspective {
            fov: f32,
            aspect_ratio: f32,
            near: f32,
            far: f32,
        },
        Orthographic {
            near: f32,
            far: f32,
            scale: f32,
            area: Rect,
        },
        Custom,
    }

    let projection = match projection {
        bevy::camera::Projection::Perspective(value) => ProjectionData::Perspective {
            fov: value.fov,
            aspect_ratio: value.aspect_ratio,
            near: value.near,
            far: value.far,
        },
        bevy::camera::Projection::Orthographic(value) => ProjectionData::Orthographic {
            near: value.near,
            far: value.far,
            scale: value.scale,
            area: value.area,
        },
        bevy::camera::Projection::Custom(_) => ProjectionData::Custom,
    };

    if matches!(projection, ProjectionData::Custom) {
        return McpResult::error(
            "UNSUPPORTED_PROJECTION",
            "camera_frame_entity does not support custom camera projections",
        );
    }

    let (direction, right, up, current_distance) = framing_basis(camera_world, center);
    let padding = 1.0 + margin as f32;
    let corners = bounds.corners();

    let response = match projection {
        ProjectionData::Perspective {
            fov,
            aspect_ratio,
            near,
            far,
        } => {
            if !fov.is_finite()
                || fov <= 0.0
                || fov >= std::f32::consts::PI
                || !aspect_ratio.is_finite()
                || aspect_ratio <= f32::EPSILON
                || !near.is_finite()
                || near < 0.0
                || !far.is_finite()
                || far <= near
            {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Perspective projection has invalid FOV, aspect ratio, or clip planes",
                );
            }
            let tan_vertical = (fov * 0.5).tan();
            let tan_horizontal = tan_vertical * aspect_ratio;
            if tan_vertical <= f32::EPSILON || tan_horizontal <= f32::EPSILON {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Perspective projection produces a degenerate field of view",
                );
            }

            let mut distance = near.max(0.001);
            for corner in corners {
                let relative = corner - center;
                let z_offset = relative.dot(direction);
                distance =
                    distance.max(relative.dot(right).abs() * padding / tan_horizontal + z_offset);
                distance = distance.max(relative.dot(up).abs() * padding / tan_vertical + z_offset);
                distance = distance.max(near + z_offset + 0.001);
            }
            let farthest_depth = corners
                .iter()
                .map(|corner| distance - (*corner - center).dot(direction))
                .fold(0.0_f32, f32::max);
            if farthest_depth > far {
                return McpResult::error(
                    "BOUNDS_OUTSIDE_CLIP_RANGE",
                    format!(
                        "Framed bounds require depth {farthest_depth:.3}, beyond camera far plane {far:.3}"
                    ),
                );
            }

            let mut desired = camera_world.compute_transform();
            desired.translation = center + direction * distance;
            desired.look_at(center, up);
            if let Err(error) = set_world_transform(world, camera, desired) {
                return error;
            }

            json!({
                "projection": "perspective",
                "distance": distance,
                "fov": fov,
                "aspect_ratio": aspect_ratio,
            })
        }
        ProjectionData::Orthographic {
            near,
            far,
            scale,
            area,
        } => {
            let area_size = area.max - area.min;
            if !near.is_finite()
                || !far.is_finite()
                || far <= near
                || !scale.is_finite()
                || scale <= 0.0
                || !area_size.is_finite()
                || area_size.x.abs() <= f32::EPSILON
                || area_size.y.abs() <= f32::EPSILON
            {
                return McpResult::error(
                    "CAMERA_PROJECTION_NOT_READY",
                    "Orthographic projection area/scale or clip planes are not ready for framing",
                );
            }

            let mut extent_x = 0.0_f32;
            let mut extent_y = 0.0_f32;
            let mut max_z = f32::NEG_INFINITY;
            let mut min_z = f32::INFINITY;
            for corner in corners {
                let relative = corner - center;
                extent_x = extent_x.max(relative.dot(right).abs());
                extent_y = extent_y.max(relative.dot(up).abs());
                let z = relative.dot(direction);
                min_z = min_z.min(z);
                max_z = max_z.max(z);
            }

            let required_width = (extent_x * 2.0 * padding).max(0.001);
            let required_height = (extent_y * 2.0 * padding).max(0.001);
            let ratio = (required_width / area_size.x.abs())
                .max(required_height / area_size.y.abs())
                .max(0.000001);
            let new_scale = scale * ratio;
            if !new_scale.is_finite() || new_scale <= 0.0 {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Calculated orthographic scale is invalid",
                );
            }

            let minimum_distance = near + max_z + 0.001;
            let maximum_distance = far + min_z - 0.001;
            if minimum_distance > maximum_distance {
                return McpResult::error(
                    "BOUNDS_OUTSIDE_CLIP_RANGE",
                    "Aggregate bounds are deeper than the orthographic camera clip range",
                );
            }
            let distance = current_distance
                .max(0.001)
                .clamp(minimum_distance, maximum_distance);
            let scaled_area_center = (area.min + area.max) * 0.5 * ratio;

            let mut desired = camera_world.compute_transform();
            desired.translation = center - right * scaled_area_center.x - up * scaled_area_center.y
                + direction * distance;
            desired.look_at(
                center - right * scaled_area_center.x - up * scaled_area_center.y,
                up,
            );
            if let Err(error) = set_world_transform(world, camera, desired) {
                return error;
            }

            let Some(mut projection) = world.get_mut::<bevy::camera::Projection>(camera) else {
                return McpResult::error(
                    "CAMERA_PROJECTION_NOT_AVAILABLE",
                    "Active camera Projection disappeared during framing",
                );
            };
            let bevy::camera::Projection::Orthographic(value) = &mut *projection else {
                return McpResult::error(
                    "CAMERA_PROJECTION_CHANGED",
                    "Active camera projection changed during framing",
                );
            };
            value.scale = new_scale;

            json!({
                "projection": "orthographic",
                "distance": distance,
                "scale": new_scale,
                "previous_scale": scale,
            })
        }
        ProjectionData::Custom => unreachable!(),
    };

    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "target": entity_to_uri(target),
        "margin": margin,
        "bounded_entities": bounds.bounded_entities,
        "bounds": {
            "min": {"x": bounds.min.x, "y": bounds.min.y, "z": bounds.min.z},
            "max": {"x": bounds.max.x, "y": bounds.max.y, "z": bounds.max.z},
            "center": {"x": center.x, "y": center.y, "z": center.z},
        },
        "framing": response,
    }))
}

fn playtest_run(_world: &World, _steps: &[bevy_mcp_core::command::PlaytestStep]) -> McpResult {
    McpResult::error("NOT_IMPLEMENTED", "Playtest execution is not implemented")
}

fn reflect_serialized_root(value: &Value) -> &Value {
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

fn serialize_assert_component(
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

fn serialize_assert_resource(world: &World, resource: &str) -> Result<Value, String> {
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

fn equality_assertion(
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

fn list_plugins(world: &World) -> McpResult {
    let checks: Vec<(&str, &str, bool)> = vec![
        (
            "InputPlugin",
            "bevy_input",
            world
                .get_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>()
                .is_some(),
        ),
        (
            "PickingPlugin",
            "bevy_picking",
            world
                .get_resource::<bevy::picking::PickingSettings>()
                .is_some(),
        ),
        (
            "GizmoPlugin",
            "bevy_gizmos",
            world
                .get_resource::<bevy::gizmos::config::GizmoConfigStore>()
                .is_some(),
        ),
        (
            "UiPlugin",
            "bevy_ui",
            world.get_resource::<bevy::ui::UiScale>().is_some(),
        ),
        (
            "TimePlugin",
            "bevy_time",
            world.get_resource::<bevy::time::Time<()>>().is_some(),
        ),
    ];

    let plugins: Vec<_> = checks
        .into_iter()
        .map(|(name, crate_name, installed)| {
            json!({
                "name": name,
                "crate": crate_name,
                "installed": installed,
            })
        })
        .collect();

    McpResult::success(json!({
        "plugins": plugins,
        "count": plugins.len(),
    }))
}

fn operation_status(world: &World, operation_id: Option<&str>) -> McpResult {
    let tracker = match world.get_resource::<crate::operations::OperationTracker>() {
        Some(t) => t,
        None => {
            return McpResult::error(
                "OPERATION_TRACKER_NOT_AVAILABLE",
                "OperationTracker resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };
    McpResult::success(tracker.get_status(operation_id))
}

fn operation_cancel(world: &World, operation_id: &str) -> McpResult {
    let tracker = match world.get_resource::<crate::operations::OperationTracker>() {
        Some(t) => t,
        None => {
            return McpResult::error(
                "OPERATION_TRACKER_NOT_AVAILABLE",
                "OperationTracker resource not found. Add BevyMcpPlugin to your app.",
            );
        }
    };

    if tracker.cancel(operation_id) {
        McpResult::success(json!({ "cancelled": operation_id }))
    } else {
        McpResult::error("NOT_FOUND", format!("Operation '{operation_id}' not found"))
    }
}

fn asset_list(world: &World, filter: Option<&str>) -> McpResult {
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };
    let _ = filter;
    McpResult::error(
        "NOT_IMPLEMENTED",
        "Global asset enumeration is not exposed by Bevy's public AssetServer API; inspect known paths with asset_get/asset_status",
    )
}

fn load_state_json(state: &bevy::asset::LoadState) -> Value {
    match state {
        bevy::asset::LoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::LoadState::Loading => json!({"state": "loading"}),
        bevy::asset::LoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::LoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn dependency_load_state_json(state: &bevy::asset::DependencyLoadState) -> Value {
    match state {
        bevy::asset::DependencyLoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::DependencyLoadState::Loading => json!({"state": "loading"}),
        bevy::asset::DependencyLoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::DependencyLoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn recursive_dependency_load_state_json(
    state: &bevy::asset::RecursiveDependencyLoadState,
) -> Value {
    match state {
        bevy::asset::RecursiveDependencyLoadState::NotLoaded => json!({"state": "not_loaded"}),
        bevy::asset::RecursiveDependencyLoadState::Loading => json!({"state": "loading"}),
        bevy::asset::RecursiveDependencyLoadState::Loaded => json!({"state": "loaded"}),
        bevy::asset::RecursiveDependencyLoadState::Failed(error) => {
            json!({"state": "failed", "error": error.to_string()})
        }
    }
}

fn asset_type_name(world: &World, type_id: std::any::TypeId) -> Option<String> {
    let registry = world.get_resource::<AppTypeRegistry>()?.read();
    registry
        .iter()
        .find(|registration| registration.type_id() == type_id)
        .map(|registration| {
            registration
                .type_info()
                .type_path_table()
                .path()
                .to_string()
        })
}

fn asset_path_snapshot(world: &World, path: &str) -> McpResult {
    let Some(asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    let ids = asset_server.get_path_ids(path.to_owned());
    if ids.is_empty() {
        return McpResult::success(json!({
            "path": path,
            "active": false,
            "status": "not_loaded",
            "assets": [],
        }));
    }

    let mut any_failed = false;
    let mut all_ready = true;
    let mut rows = Vec::with_capacity(ids.len());

    for id in ids {
        let type_id = id.type_id();
        let type_name = asset_type_name(world, type_id);
        let id_debug = format!("{id:?}");
        match asset_server.get_load_states(id) {
            Some((root, dependencies, recursive_dependencies)) => {
                let ready = root.is_loaded() && recursive_dependencies.is_loaded();
                any_failed |= root.is_failed()
                    || dependencies.is_failed()
                    || recursive_dependencies.is_failed();
                all_ready &= ready;
                rows.push(json!({
                    "id": id_debug,
                    "type_id": format!("{type_id:?}"),
                    "type_name": type_name,
                    "ready": ready,
                    "load": load_state_json(&root),
                    "dependencies": dependency_load_state_json(&dependencies),
                    "recursive_dependencies": recursive_dependency_load_state_json(&recursive_dependencies),
                }));
            }
            None => {
                all_ready = false;
                rows.push(json!({
                    "id": id_debug,
                    "type_id": format!("{type_id:?}"),
                    "type_name": type_name,
                    "ready": false,
                    "load": {"state": "unknown"},
                    "dependencies": {"state": "unknown"},
                    "recursive_dependencies": {"state": "unknown"},
                }));
            }
        }
    }

    let status = if any_failed {
        "failed"
    } else if all_ready {
        "loaded"
    } else {
        "loading"
    };

    McpResult::success(json!({
        "path": path,
        "active": true,
        "status": status,
        "assets": rows,
    }))
}

fn asset_get(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

fn asset_status(world: &World, path: &str) -> McpResult {
    asset_path_snapshot(world, path)
}

fn asset_reload(world: &World, path: &str) -> McpResult {
    let Some(asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    let ids = asset_server.get_path_ids(path.to_owned());
    if ids.is_empty() {
        return McpResult::error(
            "ASSET_NOT_ACTIVE",
            format!("Asset path '{path}' has no active AssetServer handle"),
        );
    }
    let loaded = ids.iter().any(|id| {
        asset_server
            .get_load_state(*id)
            .is_some_and(|state| state.is_loaded())
    });
    if !loaded {
        return McpResult::error(
            "ASSET_NOT_LOADED",
            format!("Asset path '{path}' is active but is not currently loaded"),
        );
    }

    asset_server.reload(path.to_owned());
    McpResult::success(json!({
        "path": path,
        "reload_queued": true,
        "active_asset_count": ids.len(),
    }))
}

fn capture_game(world: &World) -> McpResult {
    if world
        .get_resource::<bevy::render::renderer::RenderDevice>()
        .is_none()
    {
        return McpResult::error(
            "RENDER_NOT_AVAILABLE",
            "Screenshot capture requires RenderPlugin. Add DefaultPlugins to your app.",
        );
    }
    McpResult::error("NOT_IMPLEMENTED", "Screenshot capture is not implemented")
}

fn camera_inspect(world: &World) -> McpResult {
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        if let Some(camera) = world.get::<bevy::prelude::Camera>(entity) {
            let mut info = json!({
                "handle": entity_to_uri(entity),
                "id": entity.index().index(),
                "is_active": camera.is_active,
            });
            if let Some(transform) = world.get::<Transform>(entity) {
                info["position"] = json!({
                    "x": transform.translation.x,
                    "y": transform.translation.y,
                    "z": transform.translation.z,
                });
            }
            return McpResult::success(info);
        }
    }
    McpResult::error("NO_CAMERA", "No camera found in the scene")
}

fn parse_keycode(key: &str) -> Option<KeyCode> {
    match key.to_lowercase().as_str() {
        "a" => Some(KeyCode::KeyA),
        "b" => Some(KeyCode::KeyB),
        "c" => Some(KeyCode::KeyC),
        "d" => Some(KeyCode::KeyD),
        "e" => Some(KeyCode::KeyE),
        "f" => Some(KeyCode::KeyF),
        "g" => Some(KeyCode::KeyG),
        "h" => Some(KeyCode::KeyH),
        "i" => Some(KeyCode::KeyI),
        "j" => Some(KeyCode::KeyJ),
        "k" => Some(KeyCode::KeyK),
        "l" => Some(KeyCode::KeyL),
        "m" => Some(KeyCode::KeyM),
        "n" => Some(KeyCode::KeyN),
        "o" => Some(KeyCode::KeyO),
        "p" => Some(KeyCode::KeyP),
        "q" => Some(KeyCode::KeyQ),
        "r" => Some(KeyCode::KeyR),
        "s" => Some(KeyCode::KeyS),
        "t" => Some(KeyCode::KeyT),
        "u" => Some(KeyCode::KeyU),
        "v" => Some(KeyCode::KeyV),
        "w" => Some(KeyCode::KeyW),
        "x" => Some(KeyCode::KeyX),
        "y" => Some(KeyCode::KeyY),
        "z" => Some(KeyCode::KeyZ),
        "0" => Some(KeyCode::Digit0),
        "1" => Some(KeyCode::Digit1),
        "2" => Some(KeyCode::Digit2),
        "3" => Some(KeyCode::Digit3),
        "4" => Some(KeyCode::Digit4),
        "5" => Some(KeyCode::Digit5),
        "6" => Some(KeyCode::Digit6),
        "7" => Some(KeyCode::Digit7),
        "8" => Some(KeyCode::Digit8),
        "9" => Some(KeyCode::Digit9),
        "space" => Some(KeyCode::Space),
        "enter" | "return" => Some(KeyCode::Enter),
        "escape" | "esc" => Some(KeyCode::Escape),
        "tab" => Some(KeyCode::Tab),
        "backspace" => Some(KeyCode::Backspace),
        "left" | "arrowleft" => Some(KeyCode::ArrowLeft),
        "right" | "arrowright" => Some(KeyCode::ArrowRight),
        "up" | "arrowup" => Some(KeyCode::ArrowUp),
        "down" | "arrowdown" => Some(KeyCode::ArrowDown),
        "shift" | "leftshift" => Some(KeyCode::ShiftLeft),
        "ctrl" | "leftctrl" => Some(KeyCode::ControlLeft),
        "alt" | "leftalt" => Some(KeyCode::AltLeft),
        "f1" => Some(KeyCode::F1),
        "f2" => Some(KeyCode::F2),
        "f3" => Some(KeyCode::F3),
        "f4" => Some(KeyCode::F4),
        "f5" => Some(KeyCode::F5),
        "f6" => Some(KeyCode::F6),
        "f7" => Some(KeyCode::F7),
        "f8" => Some(KeyCode::F8),
        "f9" => Some(KeyCode::F9),
        "f10" => Some(KeyCode::F10),
        "f11" => Some(KeyCode::F11),
        "f12" => Some(KeyCode::F12),
        _ => None,
    }
}

fn parse_mouse_button(button: &str) -> Option<MouseButton> {
    match button.to_lowercase().as_str() {
        "left" => Some(MouseButton::Left),
        "right" => Some(MouseButton::Right),
        "middle" => Some(MouseButton::Middle),
        _ => None,
    }
}

fn parse_gamepad_button(button: &str) -> Option<GamepadButton> {
    match button.to_lowercase().as_str() {
        "south" | "a" | "cross" => Some(GamepadButton::South),
        "north" | "y" | "triangle" => Some(GamepadButton::North),
        "east" | "b" | "circle" => Some(GamepadButton::East),
        "west" | "x" | "square" => Some(GamepadButton::West),
        "left_trigger" | "lt" => Some(GamepadButton::LeftTrigger),
        "right_trigger" | "rt" => Some(GamepadButton::RightTrigger),
        "left_trigger2" | "lt2" => Some(GamepadButton::LeftTrigger2),
        "right_trigger2" | "rt2" => Some(GamepadButton::RightTrigger2),
        "select" | "back" => Some(GamepadButton::Select),
        "start" | "menu" => Some(GamepadButton::Start),
        "left_stick" | "ls" => Some(GamepadButton::LeftThumb),
        "right_stick" | "rs" => Some(GamepadButton::RightThumb),
        "dpad_up" => Some(GamepadButton::DPadUp),
        "dpad_down" => Some(GamepadButton::DPadDown),
        "dpad_left" => Some(GamepadButton::DPadLeft),
        "dpad_right" => Some(GamepadButton::DPadRight),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn mesh_spawn_apply(
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
        "handle": entity_to_uri(entity),
        "id": entity.index().index(),
        "shape": shape,
        "size": size,
        "radius": radius,
    }))
}

fn template_save(
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

        let registration = registry.iter().find(|r| {
            let tp = r.type_info().type_path_table();
            tp.short_path() == short_name || tp.path() == component_name
        });

        if let Some(registration) = registration {
            if let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            {
                if let Some(reflected) = reflect_component.reflect(entity_ref) {
                    let serializer = bevy::reflect::serde::ReflectSerializer::new(
                        reflected.as_reflect(),
                        &registry,
                    );
                    if let Ok(value) = serde_json::to_value(&serializer) {
                        if let Some(obj) = value.as_object() {
                            if let Some(inner) = obj.values().next() {
                                components_json.insert(short_name, inner.clone());
                                continue;
                            }
                        }
                        components_json.insert(short_name, value);
                    }
                }
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

    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return McpResult::error(
                "IO_ERROR",
                format!("Failed to create directory {}: {e}", parent.display()),
            );
        }
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

fn template_load_apply(
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

    if let Some((x, y, z)) = position {
        if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(Transform::from_xyz(x, y, z));
        }
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
        "handle": entity_to_uri(entity),
        "id": entity.index().index(),
        "template_name": template_name,
        "components_inserted": inserted,
    }))
}
