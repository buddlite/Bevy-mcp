from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: str, old: str, new: str, label: str) -> None:
    full = ROOT / path
    text = full.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 exact match, found {count}")
    full.write_text(text.replace(old, new, 1))


path = "crates/bevy-mcp-host/src/systems/runtime.rs"
replace_once(
    path,
    '''pub fn runtime_system(mut registry: ResMut<McpRegistry>, mut time: ResMut<Time<Virtual>>) {
    if registry.paused {
        if registry.step_remaining > 0 {
            registry.step_remaining -= 1;
            time.unpause();
        } else {
            time.pause();
        }
    } else {
        time.unpause();
        time.set_relative_speed_f64(registry.time_scale);
    }
}''',
    '''pub fn runtime_system(mut registry: ResMut<McpRegistry>, mut time: ResMut<Time<Virtual>>) {
    // Keep Bevy's relative-speed state synchronized even while paused so the next
    // stepped frame uses the scale currently reported by the MCP registry.
    time.set_relative_speed_f64(registry.time_scale);
    if registry.paused {
        if registry.step_remaining > 0 {
            registry.step_remaining -= 1;
            time.unpause();
        } else {
            time.pause();
        }
    } else {
        time.unpause();
    }
}''',
    "runtime configured speed during pause",
)

replace_once(
    path,
    '''    #[test]
    fn finite_nonnegative_time_scales_are_valid() {''',
    '''    #[test]
    fn paused_step_uses_configured_time_scale() {
        let mut app = App::new();
        app.insert_resource(McpRegistry {
            paused: true,
            time_scale: 2.0,
            step_remaining: 1,
            ..Default::default()
        });
        app.insert_resource(Time::<Virtual>::default());
        app.add_systems(Update, runtime_system);

        app.update();
        let time = app.world().resource::<Time<Virtual>>();
        assert_eq!(time.relative_speed_f64(), 2.0);
        assert!(!time.is_paused());
        assert_eq!(app.world().resource::<McpRegistry>().step_remaining, 0);

        app.update();
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
    }

    #[test]
    fn finite_nonnegative_time_scales_are_valid() {''',
    "paused step time-scale regression test",
)

print("Applied paused step time-scale fix")
