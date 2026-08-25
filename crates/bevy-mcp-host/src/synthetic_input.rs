use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::input::mouse::MouseButton;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerInput};
use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};

use crate::entity_handle::entity_to_uri;
use crate::interaction::mcp_pointer_id;
use crate::permissions::McpPermissions;
use crate::queue::{McpIngressQueue, McpResultQueue};

#[derive(Debug, Clone)]
enum SyntheticInputAction {
    Key { key: String, pressed: bool },
    Gamepad { button: String, pressed: bool },
}

#[derive(Debug, Clone)]
struct PendingSyntheticInput {
    request_id: Option<u64>,
    action: SyntheticInputAction,
}

#[derive(Resource, Default)]
pub struct McpSyntheticInputQueue {
    pending: Vec<PendingSyntheticInput>,
}

fn parse_keycode(key: &str) -> Option<KeyCode> {
    match key.to_ascii_lowercase().as_str() {
        "a" | "keya" => Some(KeyCode::KeyA),
        "b" | "keyb" => Some(KeyCode::KeyB),
        "c" | "keyc" => Some(KeyCode::KeyC),
        "d" | "keyd" => Some(KeyCode::KeyD),
        "e" | "keye" => Some(KeyCode::KeyE),
        "f" | "keyf" => Some(KeyCode::KeyF),
        "g" | "keyg" => Some(KeyCode::KeyG),
        "h" | "keyh" => Some(KeyCode::KeyH),
        "i" | "keyi" => Some(KeyCode::KeyI),
        "j" | "keyj" => Some(KeyCode::KeyJ),
        "k" | "keyk" => Some(KeyCode::KeyK),
        "l" | "keyl" => Some(KeyCode::KeyL),
        "m" | "keym" => Some(KeyCode::KeyM),
        "n" | "keyn" => Some(KeyCode::KeyN),
        "o" | "keyo" => Some(KeyCode::KeyO),
        "p" | "keyp" => Some(KeyCode::KeyP),
        "q" | "keyq" => Some(KeyCode::KeyQ),
        "r" | "keyr" => Some(KeyCode::KeyR),
        "s" | "keys" => Some(KeyCode::KeyS),
        "t" | "keyt" => Some(KeyCode::KeyT),
        "u" | "keyu" => Some(KeyCode::KeyU),
        "v" | "keyv" => Some(KeyCode::KeyV),
        "w" | "keyw" => Some(KeyCode::KeyW),
        "x" | "keyx" => Some(KeyCode::KeyX),
        "y" | "keyy" => Some(KeyCode::KeyY),
        "z" | "keyz" => Some(KeyCode::KeyZ),
        "0" | "digit0" => Some(KeyCode::Digit0),
        "1" | "digit1" => Some(KeyCode::Digit1),
        "2" | "digit2" => Some(KeyCode::Digit2),
        "3" | "digit3" => Some(KeyCode::Digit3),
        "4" | "digit4" => Some(KeyCode::Digit4),
        "5" | "digit5" => Some(KeyCode::Digit5),
        "6" | "digit6" => Some(KeyCode::Digit6),
        "7" | "digit7" => Some(KeyCode::Digit7),
        "8" | "digit8" => Some(KeyCode::Digit8),
        "9" | "digit9" => Some(KeyCode::Digit9),
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
        "ctrl" | "control" | "leftctrl" => Some(KeyCode::ControlLeft),
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

fn parse_gamepad_button(button: &str) -> Option<GamepadButton> {
    match button.to_ascii_lowercase().as_str() {
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

pub(crate) fn queue_key(
    world: &mut World,
    key: &str,
    pressed: bool,
) -> Result<(), String> {
    if parse_keycode(key).is_none() {
        return Err(format!("Unknown key '{key}'"));
    }
    let Some(mut queue) = world.get_resource_mut::<McpSyntheticInputQueue>() else {
        return Err("McpSyntheticInputQueue is not available; add BevyMcpPlugin".to_string());
    };
    queue.pending.push(PendingSyntheticInput {
        request_id: None,
        action: SyntheticInputAction::Key {
            key: key.to_string(),
            pressed,
        },
    });
    Ok(())
}

pub fn synthetic_input_ingress_system(world: &mut World) {
    let entries = world.resource::<McpIngressQueue>().drain();
    let can_input = world.resource::<McpPermissions>().can_inject_input();

    for entry in entries {
        let action = match &entry.command {
            McpCommand::InputKey { key, pressed } => Some(SyntheticInputAction::Key {
                key: key.clone(),
                pressed: *pressed,
            }),
            McpCommand::InputGamepad { button, pressed } => Some(SyntheticInputAction::Gamepad {
                button: button.clone(),
                pressed: *pressed,
            }),
            _ => None,
        };

        let Some(action) = action else {
            world
                .resource::<McpIngressQueue>()
                .push(entry.request_id, entry.command);
            continue;
        };

        if !can_input {
            world.resource::<McpResultQueue>().push(McpResponse {
                request_id: entry.request_id,
                result: McpResult::error(
                    "PERMISSION_DENIED",
                    "The configured MCP permissions do not allow input injection",
                ),
            });
            continue;
        }

        world
            .resource_mut::<McpSyntheticInputQueue>()
            .pending
            .push(PendingSyntheticInput {
                request_id: Some(entry.request_id),
                action,
            });
    }
}

pub fn synthetic_input_apply_system(world: &mut World) {
    let pending = {
        let mut queue = world.resource_mut::<McpSyntheticInputQueue>();
        std::mem::take(&mut queue.pending)
    };

    for pending in pending {
        let result = match pending.action {
            SyntheticInputAction::Key { key, pressed } => apply_key(world, &key, pressed),
            SyntheticInputAction::Gamepad { button, pressed } => {
                apply_gamepad(world, &button, pressed)
            }
        };

        if let Some(request_id) = pending.request_id {
            world.resource::<McpResultQueue>().push(McpResponse {
                request_id,
                result,
            });
        }
    }
}

fn apply_key(world: &mut World, key: &str, pressed: bool) -> McpResult {
    let Some(keycode) = parse_keycode(key) else {
        return McpResult::error("INVALID_KEY", format!("Unknown key: {key}"));
    };
    let Some(mut input) = world.get_resource_mut::<ButtonInput<KeyCode>>() else {
        return McpResult::error(
            "INPUT_NOT_AVAILABLE",
            "ButtonInput<KeyCode> resource not found. Add InputPlugin to your app.",
        );
    };
    if pressed {
        input.press(keycode);
    } else {
        input.release(keycode);
    }
    McpResult::success(serde_json::json!({ "key": key, "pressed": pressed }))
}

fn apply_gamepad(world: &mut World, button: &str, pressed: bool) -> McpResult {
    let Some(button_type) = parse_gamepad_button(button) else {
        return McpResult::error(
            "INVALID_BUTTON",
            format!("Unknown gamepad button: {button}"),
        );
    };
    let Some(entity) = world
        .iter_entities()
        .find(|entity| entity.contains::<Gamepad>())
        .map(|entity| entity.id())
    else {
        return McpResult::error(
            "GAMEPAD_NOT_AVAILABLE",
            "No connected Bevy 0.19 Gamepad entity is available",
        );
    };
    let Some(mut gamepad) = world.get_mut::<Gamepad>(entity) else {
        return McpResult::error("GAMEPAD_NOT_AVAILABLE", "Gamepad disappeared before input apply");
    };
    if pressed {
        gamepad.digital_mut().press(button_type);
    } else {
        gamepad.digital_mut().release(button_type);
    }
    McpResult::success(serde_json::json!({
        "gamepad": entity_to_uri(world, entity),
        "button": button,
        "pressed": pressed,
    }))
}

pub fn synthetic_pointer_button_system(
    mut events: MessageReader<PointerInput>,
    input: Option<ResMut<ButtonInput<MouseButton>>>,
) {
    let Some(mut input) = input else {
        return;
    };
    for event in events.read() {
        if event.pointer_id != mcp_pointer_id() {
            continue;
        }
        let (button, pressed) = match event.action {
            PointerAction::Press(button) => (button, true),
            PointerAction::Release(button) => (button, false),
            _ => continue,
        };
        let button = match button {
            PointerButton::Primary => MouseButton::Left,
            PointerButton::Secondary => MouseButton::Right,
            PointerButton::Middle => MouseButton::Middle,
        };
        if pressed {
            input.press(button);
        } else {
            input.release(button);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::InputPlugin;

    #[derive(Resource, Default)]
    struct SeenKeyEdge {
        just_pressed: bool,
        just_released: bool,
        pressed: bool,
    }

    fn observe_key(input: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenKeyEdge>) {
        seen.just_pressed = input.just_pressed(KeyCode::KeyA);
        seen.just_released = input.just_released(KeyCode::KeyA);
        seen.pressed = input.pressed(KeyCode::KeyA);
    }

    #[test]
    fn key_edges_survive_bevy_input_clear_for_gameplay_update() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin));
        app.init_resource::<McpSyntheticInputQueue>();
        app.init_resource::<SeenKeyEdge>();
        app.add_systems(
            PreUpdate,
            synthetic_input_apply_system.after(bevy::input::InputSystems),
        );
        app.add_systems(Update, observe_key);

        queue_key(app.world_mut(), "a", true).unwrap();
        app.update();
        let seen = app.world().resource::<SeenKeyEdge>();
        assert!(seen.just_pressed);
        assert!(seen.pressed);

        app.update();
        let seen = app.world().resource::<SeenKeyEdge>();
        assert!(!seen.just_pressed);
        assert!(seen.pressed);

        queue_key(app.world_mut(), "a", false).unwrap();
        app.update();
        let seen = app.world().resource::<SeenKeyEdge>();
        assert!(seen.just_released);
        assert!(!seen.pressed);
    }

    #[test]
    fn gamepad_uses_bevy_019_entity_state() {
        let mut world = World::new();
        world.init_resource::<McpSyntheticInputQueue>();
        let entity = world.spawn(Gamepad::default()).id();
        let result = apply_gamepad(&mut world, "south", true);
        assert!(matches!(result, McpResult::Success(_)));
        assert!(world.get::<Gamepad>(entity).unwrap().just_pressed(GamepadButton::South));
    }
}
