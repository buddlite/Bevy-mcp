use bevy::prelude::*;
use serde::de::DeserializeSeed;
use serde_json::{Value, json};

use crate::deferred::{DeferredCommand, DeferredMcpCommands};
use crate::entity_handle::{entity_to_uri, resolve_entity, resolve_entity_by_index};
use crate::permissions::McpPermissions;
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};

fn command_allowed(command: &McpCommand, permissions: &McpPermissions) -> bool {
    match command {
        McpCommand::EntitySpawn { .. }
        | McpCommand::EntityDespawn { .. }
        | McpCommand::ComponentInsert { .. }
        | McpCommand::ComponentUpdate { .. }
        | McpCommand::ComponentRemove { .. }
        | McpCommand::ResourceUpdate { .. }
        | McpCommand::ResourceInsert { .. }
        | McpCommand::ResourceRemove { .. }
        | McpCommand::EntityReparent { .. }
        | McpCommand::EntityDuplicate { .. }
        | McpCommand::MeshSpawn { .. }
        | McpCommand::TemplateLoad { .. } => permissions.can_mutate(),
        McpCommand::TemplateSave { .. } => permissions.level != crate::permissions::PermissionLevel::None,
        McpCommand::InputKey { .. }
        | McpCommand::InputMouseButton { .. }
        | McpCommand::InputMouseMove { .. }
        | McpCommand::InputAction { .. }
        | McpCommand::InputGamepad { .. }
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
        | McpCommand::CameraLookAt { .. } => permissions.can_control_runtime(),
        _ => permissions.level != crate::permissions::PermissionLevel::None,
    }
}

/// System that drains the ingress queue and defers all commands.
///
/// All commands (both reads and mutations) are deferred so that reads
/// see the result of mutations queued in the same frame.
/// Runs in PreUpdate::McpIngress.
pub fn ingress_system(world: &mut World) {
    // Drain the ingress queue first (read + clear).
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
            // TemplateSave goes through the read path (read-only access to world + file I/O).
            // All other commands (reads, runtime control) are deferred too.
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

/// System that applies runtime state (pause, time_scale, step).
///
/// Runs in PreUpdate after ingress.
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

/// System that increments the frame counter.
pub fn diagnostics_system(mut registry: ResMut<McpRegistry>) {
    registry.frame += 1;
}

/// System that applies deferred mutation commands.
///
/// Runs in Update as an exclusive system (has &mut World access).
pub fn deferred_apply_system(world: &mut World) {
    // Drain pending commands first.
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
            DeferredCommand::InputKey {
                key,
                pressed,
                result_id,
            } => {
                // Parse the key name to a KeyCode.
                if let Some(keycode) = parse_keycode(&key) {
                    // Check if ButtonInput<KeyCode> resource exists.
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
                    // Check if ButtonInput<MouseButton> resource exists.
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
                    world, &shape, size, radius, color, metallic, roughness, position, parent.as_ref(),
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
                let result = template_load_apply(world, &name, path.as_deref(), parent.as_ref(), position);
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

    // Wrap the value in a type annotation for ReflectDeserializer.
    // Format: {"type_path": inner_value}
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

    // Insert or apply the reflected value to the entity's component.
    let mut entity_ref = match world.get_entity_mut(entity) {
        Ok(e) => e,
        Err(_) => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity not found")),
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
        Err(_) => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity not found")),
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

    // Remove existing parent.
    if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
        entity_ref.remove::<ChildOf>();
    }

    // Set new parent if provided.
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
        McpResult::success(
            json!({ "reparented": entity_to_uri(entity), "new_parent": entity_to_uri(parent) }),
        )
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
        McpCommand::Logs { level, limit } => logs(world, level, *limit),
        McpCommand::Diagnostics => diagnostics(world, registry),
        McpCommand::Hierarchy { root, max_depth } => hierarchy(world, root.as_ref(), *max_depth),
        McpCommand::ObserveEvents { event_type, limit } => {
            observe_events(world, event_type, *limit)
        }
        McpCommand::UiQuery { root, max_depth } => ui_query(world, root.as_ref(), *max_depth),
        McpCommand::ListPlugins => list_plugins(world),
        McpCommand::CaptureGame => capture_game(world),
        McpCommand::CameraFrameEntity { entity } => camera_frame_entity(world, entity),
        McpCommand::CameraInspect => camera_inspect(world),
        McpCommand::CameraSetTransform { .. } => McpResult::error(
            "NOT_IMPLEMENTED",
            "Camera transform control is not implemented",
        ),
        McpCommand::CameraLookAt { .. } => {
            McpResult::error("NOT_IMPLEMENTED", "Camera look-at is not implemented")
        }
        McpCommand::CaptureCamera => capture_game(world),
        McpCommand::UiInspect { entity } => ui_inspect(world, entity),
        McpCommand::UiClick { entity } => ui_click(world, entity),
        McpCommand::UiType { entity, text } => ui_type(world, entity, text),
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
        // Procedural asset commands
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
        // These commands require &mut World and are handled in the deferred system.
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

// ---------------------------------------------------------------------------
// ECS inspection
// ---------------------------------------------------------------------------

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

    // ---- (a) Iterate all entities, count, group by archetype ----
    let mut total_entity_count: usize = 0;
    // Map from archetype index -> (component_id set, entity count)
    let mut archetype_map: std::collections::HashMap<
        usize,
        (Vec<usize>, usize),
    > = std::collections::HashMap::new();

    for entity_ref in world.iter_entities() {
        total_entity_count += 1;
        let arch_id = entity_ref.archetype().id().index();
        let comp_ids: Vec<usize> = entity_ref
            .archetype()
            .components()
            .map(|cid| cid.index())
            .collect();
        archetype_map
            .entry(arch_id)
            .and_modify(|(_, count)| *count += 1)
            .or_insert_with(|| (comp_ids, 1));
    }

    // Build archetype list for JSON
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

    // ---- (b) Registered component types with entity counts ----
    let app_registry = world.resource::<AppTypeRegistry>();
    let type_registry = app_registry.read();

    // Build a map from ComponentId -> entity count by summing across archetypes
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

        // Look up entity count via ComponentId
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

    // ---- (c) Resource types ----
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

    // ---- (d) Hierarchy tree ----
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

        // Gather component names for this entity
        let entity_ref = world.get_entity(entity).unwrap();
        let component_names: Vec<String> = entity_ref
            .archetype()
            .components()
            .filter_map(|cid| {
                world
                    .components()
                    .get_info(cid)
                    .map(|info| info.name().to_string())
            })
            .collect();

        // Get name if Name component exists
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

    // Collect root entities (those without ChildOf)
    let mut roots_json = Vec::new();
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        if world.get::<ChildOf>(entity).is_none() {
            roots_json.push(build_context_tree(world, entity, 0, 10));
        }
    }

    // ---- (e) Runtime state ----
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
        "hierarchy": {
            "roots": roots_json,
        },
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

    // Get the type registry.
    let app_registry = world.resource::<AppTypeRegistry>();
    let registry = app_registry.read();

    // Look up the component type by short name.
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

    // Get ReflectComponent type data to read the component from the entity.
    let reflect_component = match registration.data::<bevy::ecs::reflect::ReflectComponent>() {
        Some(rc) => rc,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_REFLECTED",
                format!("Component '{component}' does not have ReflectComponent data"),
            );
        }
    };

    // Get the component as a reflected value.
    let reflected = match reflect_component.reflect(entity_ref) {
        Some(r) => r,
        None => {
            return McpResult::error(
                "COMPONENT_NOT_PRESENT",
                format!("Entity {handle} does not have component '{component}'"),
            );
        }
    };

    // Serialize the reflected value to JSON.
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
                .map(|field| {
                    json!({
                        "name": field.name(),
                        "type": field.type_path(),
                    })
                })
                .collect();
            schema["kind"] = json!("struct");
            schema["fields"] = json!(fields);
            schema["field_count"] = json!(s.field_len());
        }
        bevy::reflect::TypeInfo::TupleStruct(ts) => {
            let fields: Vec<Value> = ts
                .iter()
                .map(|field| {
                    json!({
                        "type": field.type_path(),
                    })
                })
                .collect();
            schema["kind"] = json!("tuple_struct");
            schema["fields"] = json!(fields);
            schema["field_count"] = json!(ts.field_len());
        }
        bevy::reflect::TypeInfo::Tuple(t) => {
            let fields: Vec<Value> = t
                .iter()
                .map(|field| {
                    json!({
                        "type": field.type_path(),
                    })
                })
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
                    let mut v = json!({
                        "name": variant.name(),
                    });
                    match variant {
                        bevy::reflect::enums::VariantInfo::Struct(s) => {
                            let fields: Vec<Value> = s
                                .iter()
                                .map(|f| {
                                    json!({
                                        "name": f.name(),
                                        "type": f.type_path(),
                                    })
                                })
                                .collect();
                            v["kind"] = json!("struct");
                            v["fields"] = json!(fields);
                        }
                        bevy::reflect::enums::VariantInfo::Tuple(t) => {
                            let fields: Vec<Value> = t
                                .iter()
                                .map(|f| {
                                    json!({
                                        "type": f.type_path(),
                                    })
                                })
                                .collect();
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

    // Get ReflectFromPtr type data to convert the raw pointer to &dyn Reflect.
    let reflect_from_ptr = match registration.data::<bevy::reflect::ReflectFromPtr>() {
        Some(rfp) => rfp,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REFLECTED",
                format!("Resource '{resource}' does not have ReflectFromPtr data"),
            );
        }
    };

    // Find the resource by iterating over all resources.
    for (info, ptr) in world.iter_resources() {
        if info.type_id() == Some(type_id) {
            // Cast to &dyn Reflect using ReflectFromPtr.
            // SAFETY: ptr points to an object of the correct type_id, and reflect_from_ptr
            // was constructed for the same type_id.
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
    // Reuse the same logic as component_schema since both use TypeInfo.
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
    // SAFETY: `component_id`, its type registration, and the mutable resource
    // pointer come from the same world while `target` holds the exclusive borrow.
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

    let reflect_resource = match registration.data::<bevy::ecs::reflect::ReflectResource>() {
        Some(rr) => rr,
        None => {
            return McpResult::error(
                "RESOURCE_NOT_REFLECTED",
                format!("Resource '{resource}' does not have ReflectResource data"),
            );
        }
    };

    reflect_resource.remove(world);

    McpResult::success(json!({
        "resource": resource,
        "status": "removed"
    }))
}

// ---------------------------------------------------------------------------
// ECS mutation
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Runtime control — modifies McpRegistry, runtime_system applies to Bevy
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Logs & diagnostics
// ---------------------------------------------------------------------------

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
    let mut entity_count = 0usize;
    for _ in world.iter_entities() {
        entity_count += 1;
    }
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
        // Return hierarchy starting from a specific entity.
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
        // Return all root entities (entities without a parent).
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

        // Add Node info if present (UI layout).
        if let Some(_node) = world.get::<Node>(entity) {
            node_info["node"] = json!({
                "has_node": true,
            });
        }

        // Add Text info if present.
        if let Some(text) = world.get::<bevy::prelude::Text>(entity) {
            node_info["text"] = json!(text.to_string());
        }

        // Recurse into children.
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
        // Query all UI root entities (entities with Node but no parent).
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

    // Add Node info if present.
    if let Some(_node) = world.get::<bevy::ui::Node>(entity) {
        info["has_node"] = json!(true);
    }

    // Add Text info if present.
    if let Some(text) = world.get::<bevy::prelude::Text>(entity) {
        info["text"] = json!(text.to_string());
    }

    // Add Button info if present.
    if world.get::<bevy::prelude::Button>(entity).is_some() {
        info["is_button"] = json!(true);
    }

    McpResult::success(info)
}

fn ui_click(world: &World, handle: &bevy_mcp_core::entity_handle::EntityHandle) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    // Check if the entity is a button.
    if world.get::<bevy::prelude::Button>(entity).is_none() {
        return McpResult::error("NOT_A_BUTTON", format!("Entity {handle} is not a Button"));
    }

    McpResult::error(
        "NOT_IMPLEMENTED",
        "UI interaction injection is not implemented",
    )
}

fn ui_type(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    _text: &str,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    // Check if the entity has Text.
    if world.get::<bevy::prelude::Text>(entity).is_none() {
        return McpResult::error(
            "NOT_A_TEXT_FIELD",
            format!("Entity {handle} does not have a Text component"),
        );
    }

    McpResult::error("NOT_IMPLEMENTED", "UI text input is not implemented")
}

fn playtest_run(_world: &World, _steps: &[bevy_mcp_core::command::PlaytestStep]) -> McpResult {
    McpResult::error("NOT_IMPLEMENTED", "Playtest execution is not implemented")
}

fn assert_condition(world: &World, assertion: &bevy_mcp_core::command::Assertion) -> McpResult {
    use bevy_mcp_core::command::Assertion;

    match assertion {
        Assertion::EntityExists { entity_id } => {
            let exists = resolve_entity_by_index(world, *entity_id).is_some();
            if exists {
                McpResult::success(
                    json!({ "passed": true, "assertion": "entity_exists", "entity_id": entity_id }),
                )
            } else {
                McpResult::success(
                    json!({ "passed": false, "assertion": "entity_exists", "entity_id": entity_id, "error": "Entity not found" }),
                )
            }
        }
        Assertion::ComponentExists {
            entity_id,
            component,
        } => {
            let entity = match resolve_entity_by_index(world, *entity_id) {
                Some(e) => e,
                None => {
                    return McpResult::success(
                        json!({ "passed": false, "assertion": "component_exists", "error": "Entity not found" }),
                    );
                }
            };

            let app_registry = world.resource::<AppTypeRegistry>();
            let registry = app_registry.read();

            let has_component = registry.iter().any(|r| {
                r.type_info().type_path_table().short_path() == component
                    && r.data::<bevy::ecs::reflect::ReflectComponent>()
                        .and_then(|rc| rc.reflect(world.get_entity(entity).ok()?))
                        .is_some()
            });

            McpResult::success(json!({
                "passed": has_component,
                "assertion": "component_exists",
                "entity_id": entity_id,
                "component": component,
            }))
        }
        Assertion::EntityCount { expected } => {
            let count = world.iter_entities().count() as u32;
            McpResult::success(json!({
                "passed": count == *expected,
                "assertion": "entity_count",
                "expected": expected,
                "actual": count,
            }))
        }
        _ => McpResult::error(
            "NOT_IMPLEMENTED",
            "This assertion type is not yet implemented",
        ),
    }
}

fn list_plugins(world: &World) -> McpResult {
    // Detect installed plugins by checking for their characteristic resources.
    let mut plugins = Vec::new();

    // Check for common Bevy plugins by looking for their resources.
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

    for (name, crate_name, installed) in checks {
        plugins.push(json!({
            "name": name,
            "crate": crate_name,
            "installed": installed,
        }));
    }

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

    let status = tracker.get_status(operation_id);
    McpResult::success(status)
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
    // Check if AssetServer is available.
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    let _ = filter;
    McpResult::error("NOT_IMPLEMENTED", "Asset listing is not implemented")
}

fn asset_get(world: &World, path: &str) -> McpResult {
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    McpResult::error(
        "NOT_IMPLEMENTED",
        format!("Asset inspection is not implemented ({path})"),
    )
}

fn asset_status(world: &World, path: &str) -> McpResult {
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    McpResult::error(
        "NOT_IMPLEMENTED",
        format!("Asset status is not implemented ({path})"),
    )
}

fn asset_reload(world: &World, path: &str) -> McpResult {
    let Some(_asset_server) = world.get_resource::<bevy::asset::AssetServer>() else {
        return McpResult::error(
            "ASSET_SERVER_NOT_AVAILABLE",
            "AssetServer not found. Add AssetPlugin to your app.",
        );
    };

    McpResult::error(
        "NOT_IMPLEMENTED",
        format!("Asset reloading is not implemented ({path})"),
    )
}

fn capture_game(world: &World) -> McpResult {
    // Check if the render plugin is installed by looking for RenderDevice.
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

fn camera_frame_entity(
    world: &World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(e) => e,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };

    // Check if the entity has a Transform component.
    let Some(transform) = world.get::<Transform>(entity) else {
        return McpResult::error(
            "NO_TRANSFORM",
            format!("Entity {handle} does not have a Transform component"),
        );
    };

    let position = transform.translation;

    McpResult::error(
        "NOT_IMPLEMENTED",
        format!("Camera framing is not implemented (target position: {position:?})"),
    )
}

fn camera_inspect(world: &World) -> McpResult {
    // Find the first camera entity.
    let mut camera_info = None;
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

            camera_info = Some(info);
            break;
        }
    }

    match camera_info {
        Some(info) => McpResult::success(info),
        None => McpResult::error("NO_CAMERA", "No camera found in the scene"),
    }
}

// ---------------------------------------------------------------------------
// Input parsing helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Procedural asset generation
// ---------------------------------------------------------------------------

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
    use bevy::pbr::{Mesh3d, MeshMaterial3d, StandardMaterial};

    let size_f32 = size as f32;
    let radius_f32 = radius as f32;

    // Create the mesh based on shape.
    let mesh_handle = {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        match shape {
            "cube" => meshes.add(Cuboid::new(size_f32, size_f32, size_f32)),
            "sphere" => meshes.add(Sphere::new(radius_f32)),
            "plane" => meshes.add(Plane3d::default().mesh().size(size_f32, size_f32)),
            "cylinder" => meshes.add(Cylinder::new(radius_f32, size_f32)),
            "torus" => {
                // Use radius as major radius, derive minor radius from size.
                meshes.add(Torus::new(radius_f32, size_f32 * 0.4))
            }
            _ => {
                return McpResult::error(
                    "INVALID_SHAPE",
                    format!("Unknown shape '{shape}'. Valid: cube, sphere, plane, cylinder, torus"),
                );
            }
        }
    };

    // Create PBR material.
    let material_handle = {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        materials.add(StandardMaterial {
            base_color: Color::srgba(color.0, color.1, color.2, color.3),
            metallic,
            roughness,
            ..default()
        })
    };

    // Spawn entity with mesh, material, and transform.
    let entity = world
        .spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            Transform::from_xyz(position.0, position.1, position.2),
        ))
        .id();

    // Parent if specified.
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

// ---------------------------------------------------------------------------
// Template save / load
// ---------------------------------------------------------------------------

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

    // Collect all reflected components from the entity.
    let entity_ref = world.get_entity(entity).unwrap();
    let mut components_json = serde_json::Map::new();

    for component_id in entity_ref.archetype().components() {
        let Some(info) = world.components().get_info(*component_id) else {
            continue;
        };
        let component_name = info.name().to_string();

        // Try to find the type registration and serialize via reflection.
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
                        // Extract inner value: ReflectSerializer wraps in {"TypePath": value}.
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

    // Write to file.
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

    // Create parent directories if needed.
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

    // Spawn empty entity.
    let entity = world.spawn_empty().id();

    // Insert each component via reflection.
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

    // Apply position override (update Transform if present).
    if let Some((x, y, z)) = position {
        if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(Transform::from_xyz(x, y, z));
        }
    }

    // Parent if specified.
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
