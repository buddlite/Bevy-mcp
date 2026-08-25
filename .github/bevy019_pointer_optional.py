from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: str, old: str, new: str, label: str) -> None:
    full = ROOT / path
    text = full.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 exact match, found {count}")
    full.write_text(text.replace(old, new, 1))


path = "crates/bevy-mcp-host/src/synthetic_input.rs"
replace_once(
    path,
    '''pub fn synthetic_pointer_button_system(
    mut events: MessageReader<PointerInput>,
    input: Option<ResMut<ButtonInput<MouseButton>>>,
) {
    let Some(mut input) = input else {
        return;
    };
    for event in events.read() {''',
    '''pub fn synthetic_pointer_button_system(
    events: Option<MessageReader<PointerInput>>,
    input: Option<ResMut<ButtonInput<MouseButton>>>,
) {
    let Some(mut events) = events else {
        return;
    };
    let Some(mut input) = input else {
        return;
    };
    for event in events.read() {''',
    "optional PointerInput messages",
)

replace_once(
    path,
    '''    #[test]
    fn gamepad_uses_bevy_019_entity_state() {''',
    '''    #[test]
    fn pointer_bridge_is_safe_without_picking_plugin() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin));
        app.add_systems(
            PreUpdate,
            synthetic_pointer_button_system.after(bevy::input::InputSystems),
        );

        app.update();

        let input = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(!input.pressed(MouseButton::Left));
        assert!(!input.just_pressed(MouseButton::Left));
        assert!(!input.just_released(MouseButton::Left));
    }

    #[test]
    fn gamepad_uses_bevy_019_entity_state() {''',
    "missing PickingPlugin regression test",
)

print("Applied optional PickingPlugin compatibility fix")
