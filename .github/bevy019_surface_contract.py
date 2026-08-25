from pathlib import Path

path = Path("crates/bevy-mcp-host/tests/surface_contract.rs")
text = path.read_text()

old = '''    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(ButtonInput::<MouseButton>::default());
    app.insert_resource(ButtonInput::<GamepadButton>::default());

    ingress.push(2, McpCommand::Capabilities);'''
new = '''    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(ButtonInput::<MouseButton>::default());
    let gamepad = app.world_mut().spawn(Gamepad::default()).id();

    ingress.push(2, McpCommand::Capabilities);'''
if old not in text:
    raise SystemExit("legacy gamepad fixture block not found")
text = text.replace(old, new, 1)

old = '''    assert!(
        app.world()
            .resource::<ButtonInput<GamepadButton>>()
            .pressed(GamepadButton::South)
    );
}'''
new = '''    assert!(
        app.world()
            .get::<Gamepad>(gamepad)
            .expect("mock gamepad should remain connected")
            .pressed(GamepadButton::South)
    );
}'''
if old not in text:
    raise SystemExit("legacy gamepad assertion block not found")
text = text.replace(old, new, 1)

path.write_text(text)
print("Aligned surface contract gamepad fixture with Bevy 0.19 entity model")
