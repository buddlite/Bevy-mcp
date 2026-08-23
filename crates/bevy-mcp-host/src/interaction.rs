use std::collections::VecDeque;

use bevy::asset::uuid::{Uuid, uuid};
use bevy::camera::RenderTarget;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input::touch::TouchPhase;
use bevy::picking::PickingSettings;
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerInteraction,
};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};
use serde_json::{Value, json};

use crate::entity_handle::{entity_to_uri, resolve_entity};
use crate::queue::McpResultQueue;

const MCP_POINTER_UUID: Uuid = uuid!("5c700a9a-6a31-4b7c-a5f5-2f75f8ef3dc4");

pub fn mcp_pointer_id() -> PointerId {
    PointerId::Custom(MCP_POINTER_UUID)
}

#[derive(Debug, Clone)]
enum InteractionKind {
    Pick,
    Move,
    RawButton {
        button: PointerButton,
        pressed: bool,
    },
    Click {
        button: PointerButton,
        expected_target: Option<Entity>,
    },
    Drag {
        button: PointerButton,
        from: Location,
        to: Location,
        steps: u32,
        step: u32,
    },
    Scroll {
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Debug, Clone)]
struct PendingInteraction {
    request_id: u64,
    location: Location,
    kind: InteractionKind,
    phase: u32,
}

#[derive(Resource, Default)]
pub struct McpInteractionState {
    pointer_entity: Option<Entity>,
    pending: VecDeque<PendingInteraction>,
    active: Option<PendingInteraction>,
    last_location: Option<Location>,
}

impl McpInteractionState {
    pub fn set_pointer_entity(&mut self, entity: Entity) {
        self.pointer_entity = Some(entity);
    }

    pub fn pointer_entity(&self) -> Option<Entity> {
        self.pointer_entity
    }
}

fn push_result(world: &World, request_id: u64, result: McpResult) {
    world
        .resource::<McpResultQueue>()
        .push(McpResponse { request_id, result });
}

fn parse_button(button: &str) -> Result<PointerButton, String> {
    match button.to_ascii_lowercase().as_str() {
        "left" | "primary" => Ok(PointerButton::Primary),
        "right" | "secondary" => Ok(PointerButton::Secondary),
        "middle" => Ok(PointerButton::Middle),
        other => Err(format!(
            "Unknown pointer button '{other}'. Valid buttons: left/primary, right/secondary, middle"
        )),
    }
}

fn picking_available(world: &World) -> Result<(), McpResult> {
    if world.get_resource::<PickingSettings>().is_none()
        || world.get_resource::<Messages<PointerInput>>().is_none()
    {
        return Err(McpResult::error(
            "PICKING_NOT_AVAILABLE",
            "Bevy PickingPlugin is not installed; add DefaultPlugins or PickingPlugin",
        ));
    }
    let settings = world.resource::<PickingSettings>();
    if !settings.is_enabled {
        return Err(McpResult::error(
            "PICKING_DISABLED",
            "Bevy picking is currently disabled",
        ));
    }
    if !settings.is_input_enabled {
        return Err(McpResult::error(
            "PICKING_INPUT_DISABLED",
            "Bevy picking input processing is currently disabled",
        ));
    }
    Ok(())
}

fn primary_location(world: &World, x: f64, y: f64) -> Result<Location, McpResult> {
    picking_available(world)?;
    let primary = world
        .iter_entities()
        .find(|entity| entity.contains::<PrimaryWindow>())
        .map(|entity| entity.id())
        .ok_or_else(|| {
            McpResult::error(
                "PRIMARY_WINDOW_NOT_AVAILABLE",
                "Pointer interaction currently requires a PrimaryWindow",
            )
        })?;
    let target = RenderTarget::Window(WindowRef::Entity(primary))
        .normalize(Some(primary))
        .ok_or_else(|| {
            McpResult::error(
                "POINTER_TARGET_NOT_AVAILABLE",
                "Could not normalize the primary window render target",
            )
        })?;
    Ok(Location {
        target,
        position: Vec2::new(x as f32, y as f32),
    })
}

fn ui_location(world: &World, entity: Entity) -> Result<Location, McpResult> {
    picking_available(world)?;
    let transform = world
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .ok_or_else(|| {
            McpResult::error(
                "UI_LAYOUT_NOT_READY",
                "UI entity has no UiGlobalTransform; wait for UI layout to run",
            )
        })?;
    let position = transform.transform_point2(Vec2::ZERO);

    let primary = world
        .iter_entities()
        .find(|candidate| candidate.contains::<PrimaryWindow>())
        .map(|candidate| candidate.id());

    let render_target = world
        .get::<bevy::ui::ComputedUiTargetCamera>(entity)
        .and_then(|target| target.get())
        .and_then(|camera| world.get::<RenderTarget>(camera).cloned())
        .unwrap_or(RenderTarget::Window(WindowRef::Primary));

    let target = render_target.normalize(primary).ok_or_else(|| {
        McpResult::error(
            "UI_TARGET_NOT_AVAILABLE",
            "Could not normalize the UI entity's render target",
        )
    })?;

    Ok(Location { target, position })
}

fn queue(world: &mut World, pending: PendingInteraction) {
    world
        .resource_mut::<McpInteractionState>()
        .pending
        .push_back(pending);
}

pub fn is_interaction_command(command: &McpCommand) -> bool {
    matches!(
        command,
        McpCommand::InputMouseMove { .. }
            | McpCommand::InputMouseButton { .. }
            | McpCommand::PickAt { .. }
            | McpCommand::PointerClick { .. }
            | McpCommand::PointerDrag { .. }
            | McpCommand::PointerScroll { .. }
            | McpCommand::UiClick { .. }
    )
}

pub fn enqueue_command(world: &mut World, request_id: u64, command: &McpCommand) {
    let result = match command {
        McpCommand::InputMouseMove { x, y } => primary_location(world, *x, *y).map(|location| {
            queue(
                world,
                PendingInteraction {
                    request_id,
                    location,
                    kind: InteractionKind::Move,
                    phase: 0,
                },
            )
        }),
        McpCommand::InputMouseButton {
            button,
            pressed,
            x,
            y,
        } => {
            let button = match parse_button(button) {
                Ok(button) => button,
                Err(message) => {
                    push_result(
                        world,
                        request_id,
                        McpResult::error("INVALID_BUTTON", message),
                    );
                    return;
                }
            };
            let location = match (x, y) {
                (Some(x), Some(y)) => primary_location(world, *x, *y),
                (None, None) => world
                    .resource::<McpInteractionState>()
                    .last_location
                    .clone()
                    .ok_or_else(|| {
                        McpResult::error(
                            "POINTER_LOCATION_REQUIRED",
                            "Move the MCP pointer first or provide both x and y",
                        )
                    }),
                _ => Err(McpResult::error(
                    "INVALID_PARAMS",
                    "x and y must either both be supplied or both be omitted",
                )),
            };
            location.map(|location| {
                queue(
                    world,
                    PendingInteraction {
                        request_id,
                        location,
                        kind: InteractionKind::RawButton {
                            button,
                            pressed: *pressed,
                        },
                        phase: 0,
                    },
                )
            })
        }
        McpCommand::PickAt { x, y } => primary_location(world, *x, *y).map(|location| {
            queue(
                world,
                PendingInteraction {
                    request_id,
                    location,
                    kind: InteractionKind::Pick,
                    phase: 0,
                },
            )
        }),
        McpCommand::PointerClick { x, y, button } => {
            let button = match parse_button(button) {
                Ok(button) => button,
                Err(message) => {
                    push_result(
                        world,
                        request_id,
                        McpResult::error("INVALID_BUTTON", message),
                    );
                    return;
                }
            };
            primary_location(world, *x, *y).map(|location| {
                queue(
                    world,
                    PendingInteraction {
                        request_id,
                        location,
                        kind: InteractionKind::Click {
                            button,
                            expected_target: None,
                        },
                        phase: 0,
                    },
                )
            })
        }
        McpCommand::PointerDrag {
            from_x,
            from_y,
            to_x,
            to_y,
            button,
            steps,
        } => {
            let button = match parse_button(button) {
                Ok(button) => button,
                Err(message) => {
                    push_result(
                        world,
                        request_id,
                        McpResult::error("INVALID_BUTTON", message),
                    );
                    return;
                }
            };
            let from = match primary_location(world, *from_x, *from_y) {
                Ok(location) => location,
                Err(error) => {
                    push_result(world, request_id, error);
                    return;
                }
            };
            let to = match primary_location(world, *to_x, *to_y) {
                Ok(location) => location,
                Err(error) => {
                    push_result(world, request_id, error);
                    return;
                }
            };
            let steps = (*steps).clamp(1, 120);
            Ok(queue(
                world,
                PendingInteraction {
                    request_id,
                    location: from.clone(),
                    kind: InteractionKind::Drag {
                        button,
                        from,
                        to,
                        steps,
                        step: 0,
                    },
                    phase: 0,
                },
            ))
        }
        McpCommand::PointerScroll {
            x,
            y,
            delta_x,
            delta_y,
        } => primary_location(world, *x, *y).map(|location| {
            queue(
                world,
                PendingInteraction {
                    request_id,
                    location,
                    kind: InteractionKind::Scroll {
                        delta_x: *delta_x as f32,
                        delta_y: *delta_y as f32,
                    },
                    phase: 0,
                },
            )
        }),
        McpCommand::UiClick { entity } => {
            let entity = match resolve_entity(world, entity) {
                Some(entity) => entity,
                None => {
                    push_result(
                        world,
                        request_id,
                        McpResult::error("ENTITY_NOT_FOUND", format!("Entity {entity} not found")),
                    );
                    return;
                }
            };
            if world.get::<bevy::ui::Node>(entity).is_none() {
                push_result(
                    world,
                    request_id,
                    McpResult::error("NOT_UI_NODE", "Target entity does not have a UI Node"),
                );
                return;
            }
            ui_location(world, entity).map(|location| {
                queue(
                    world,
                    PendingInteraction {
                        request_id,
                        location,
                        kind: InteractionKind::Click {
                            button: PointerButton::Primary,
                            expected_target: Some(entity),
                        },
                        phase: 0,
                    },
                )
            })
        }
        _ => return,
    };

    if let Err(error) = result {
        push_result(world, request_id, error);
    }
}

fn pointer_delta(previous: Option<&Location>, next: &Location) -> Vec2 {
    previous
        .filter(|previous| previous.target == next.target)
        .map(|previous| next.position - previous.position)
        .unwrap_or(Vec2::ZERO)
}

fn emit(world: &mut World, location: Location, action: PointerAction) -> Result<(), McpResult> {
    if world.get_resource::<Messages<PointerInput>>().is_none() {
        return Err(McpResult::error(
            "PICKING_NOT_AVAILABLE",
            "PointerInput message resource is not installed",
        ));
    }
    world.write_message(PointerInput::new(mcp_pointer_id(), location, action));
    Ok(())
}

pub fn interaction_input_system(world: &mut World) {
    let mut state = world
        .remove_resource::<McpInteractionState>()
        .unwrap_or_default();
    let Some(mut active) = state.active.take().or_else(|| state.pending.pop_front()) else {
        world.insert_resource(state);
        return;
    };

    let previous = state.last_location.as_ref();
    let (location, action) = match &mut active.kind {
        InteractionKind::Pick | InteractionKind::Move => {
            let delta = pointer_delta(previous, &active.location);
            (active.location.clone(), PointerAction::Move { delta })
        }
        InteractionKind::RawButton { button, pressed } => {
            if active.phase == 0 {
                let delta = pointer_delta(previous, &active.location);
                (active.location.clone(), PointerAction::Move { delta })
            } else if *pressed {
                (active.location.clone(), PointerAction::Press(*button))
            } else {
                (active.location.clone(), PointerAction::Release(*button))
            }
        }
        InteractionKind::Click { button, .. } => match active.phase {
            0 => {
                let delta = pointer_delta(previous, &active.location);
                (active.location.clone(), PointerAction::Move { delta })
            }
            1 => (active.location.clone(), PointerAction::Press(*button)),
            _ => (active.location.clone(), PointerAction::Release(*button)),
        },
        InteractionKind::Drag {
            button,
            from,
            to,
            steps,
            step,
        } => match active.phase {
            0 => {
                active.location = from.clone();
                let delta = pointer_delta(previous, &active.location);
                (active.location.clone(), PointerAction::Move { delta })
            }
            1 => (active.location.clone(), PointerAction::Press(*button)),
            2 => {
                *step = (*step + 1).min(*steps);
                let t = *step as f32 / *steps as f32;
                active.location = Location {
                    target: to.target.clone(),
                    position: from.position.lerp(to.position, t),
                };
                let delta = pointer_delta(previous, &active.location);
                (active.location.clone(), PointerAction::Move { delta })
            }
            _ => (active.location.clone(), PointerAction::Release(*button)),
        },
        InteractionKind::Scroll { delta_x, delta_y } => {
            if active.phase == 0 {
                let delta = pointer_delta(previous, &active.location);
                (active.location.clone(), PointerAction::Move { delta })
            } else {
                (
                    active.location.clone(),
                    PointerAction::Scroll {
                        unit: MouseScrollUnit::Pixel,
                        x: *delta_x,
                        y: *delta_y,
                        phase: TouchPhase::Moved,
                    },
                )
            }
        }
    };

    match emit(world, location.clone(), action) {
        Ok(()) => {
            state.last_location = Some(location);
            state.active = Some(active);
        }
        Err(error) => push_result(world, active.request_id, error),
    }
    world.insert_resource(state);
}

fn hit_rows(world: &World, pointer_entity: Entity) -> Vec<Value> {
    let Some(interaction) = world.get::<PointerInteraction>(pointer_entity) else {
        return Vec::new();
    };
    interaction
        .iter()
        .map(|(entity, hit)| {
            json!({
                "entity": entity_to_uri(*entity),
                "id": entity.index().index(),
                "camera": entity_to_uri(hit.camera),
                "depth": hit.depth,
                "position": hit.position.map(|point| json!({"x": point.x, "y": point.y, "z": point.z})),
                "normal": hit.normal.map(|normal| json!({"x": normal.x, "y": normal.y, "z": normal.z})),
            })
        })
        .collect()
}

fn is_descendant_of(world: &World, mut entity: Entity, ancestor: Entity) -> bool {
    if entity == ancestor {
        return true;
    }
    while let Some(parent) = world.get::<bevy::ecs::hierarchy::ChildOf>(entity) {
        entity = parent.parent();
        if entity == ancestor {
            return true;
        }
    }
    false
}

fn target_is_hit(world: &World, pointer_entity: Entity, expected: Entity) -> bool {
    world
        .get::<PointerInteraction>(pointer_entity)
        .is_some_and(|interaction| {
            interaction
                .iter()
                .any(|(entity, _)| is_descendant_of(world, *entity, expected))
        })
}

pub fn interaction_result_system(world: &mut World) {
    let mut state = world
        .remove_resource::<McpInteractionState>()
        .unwrap_or_default();
    let Some(pointer_entity) = state.pointer_entity else {
        world.insert_resource(state);
        return;
    };
    let Some(mut active) = state.active.take() else {
        world.insert_resource(state);
        return;
    };

    let mut complete = false;
    let mut failure: Option<McpResult> = None;

    match &mut active.kind {
        InteractionKind::Pick | InteractionKind::Move => complete = true,
        InteractionKind::RawButton { .. } => {
            if active.phase == 0 {
                active.phase = 1;
            } else {
                complete = true;
            }
        }
        InteractionKind::Click {
            expected_target, ..
        } => match active.phase {
            0 => {
                if let Some(expected) = expected_target
                    && !target_is_hit(world, pointer_entity, *expected)
                {
                    failure = Some(McpResult::error(
                        "TARGET_NOT_PICKED",
                        format!(
                            "UI target {} was not under the MCP pointer after moving to its computed center",
                            entity_to_uri(*expected)
                        ),
                    ));
                } else {
                    active.phase = 1;
                }
            }
            1 => active.phase = 2,
            _ => complete = true,
        },
        InteractionKind::Drag { steps, step, .. } => match active.phase {
            0 => active.phase = 1,
            1 => active.phase = 2,
            2 if *step < *steps => {}
            2 => active.phase = 3,
            _ => complete = true,
        },
        InteractionKind::Scroll { .. } => {
            if active.phase == 0 {
                active.phase = 1;
            } else {
                complete = true;
            }
        }
    }

    if failure.is_some() || complete {
        let hits = hit_rows(world, pointer_entity);
        let nearest = hits.first().cloned();
        let result = failure.unwrap_or_else(|| {
            McpResult::success(json!({
                "pointer": "mcp",
                "position": {"x": active.location.position.x, "y": active.location.position.y},
                "hits": hits,
                "nearest": nearest,
            }))
        });
        push_result(world, active.request_id, result);
    } else {
        state.active = Some(active);
    }

    world.insert_resource(state);
}

pub fn pointer_available(world: &World) -> bool {
    let picking_ready = world
        .get_resource::<PickingSettings>()
        .is_some_and(|settings| settings.is_enabled && settings.is_input_enabled);
    let pointer_registered = world
        .get_resource::<McpInteractionState>()
        .and_then(McpInteractionState::pointer_entity)
        .is_some_and(|entity| world.get::<PointerId>(entity) == Some(&mcp_pointer_id()));
    picking_ready
        && world.get_resource::<Messages<PointerInput>>().is_some()
        && pointer_registered
        && world
            .iter_entities()
            .any(|entity| entity.contains::<PrimaryWindow>())
}
