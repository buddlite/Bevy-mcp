use super::assertions::*;
use super::assets::*;
use super::camera::*;
use super::capabilities::*;
use super::ecs_inspect::*;
use super::ecs_mutate::*;
use super::input::*;
use super::procedural::*;
use super::resources::*;
use super::runtime::*;
use super::ui::*;
use super::*;

pub(crate) fn command_allowed(command: &McpCommand, permissions: &McpPermissions) -> bool {
    match command {
        McpCommand::Capabilities | McpCommand::HostProbe { .. } => true,
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
        if let Some(result) = validate_command_entity_handles(world, &entry.command) {
            world.resource::<McpResultQueue>().push(McpResponse {
                request_id: entry.request_id,
                result,
            });
            continue;
        }

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
                        "handle": entity_to_uri(world, entity),
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

pub(crate) fn execute_command(
    world: &World,
    command: &McpCommand,
    registry: &mut McpRegistry,
) -> McpResult {
    match command {
        McpCommand::WorldSummary => world_summary(world),
        McpCommand::Capabilities => capabilities(world),
        McpCommand::HostProbe { probe_id } => McpResult::success(json!({
            "probe_id": probe_id,
            "instance_id": world.resource::<crate::instance::McpInstanceId>().as_str(),
            "frame": registry.frame,
        })),
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

#[cfg(test)]
mod supervisor_stage1_tests {
    use super::*;
    use crate::instance::McpInstanceId;

    #[test]
    fn host_probe_is_acknowledged_by_normal_command_execution() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-test"));
        let mut registry = McpRegistry::new("0.19.1");
        registry.frame = 41;
        let result = execute_command(
            &world,
            &McpCommand::HostProbe { probe_id: 7 },
            &mut registry,
        );
        match result {
            McpResult::Success(value) => {
                assert_eq!(value["probe_id"], 7);
                assert_eq!(value["instance_id"], "run-test");
                assert_eq!(value["frame"], 41);
            }
            other => panic!("expected successful probe, got {other:?}"),
        }
    }
}
