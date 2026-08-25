use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions, PermissionLevel};
use serde_json::Value;

fn success_for(results: &McpResultQueue, request_id: u64) -> Value {
    let response = results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response");
    match response.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected MCP error {code}: {message}"),
    }
}

#[test]
fn capabilities_are_live_and_permission_aware() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::read_only()),
    );

    ingress.push(1, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 1);

    assert_eq!(capabilities["schema_version"], 2);
    assert_eq!(capabilities["permissions"]["level"], "read");
    assert_eq!(capabilities["runtime"]["pause"]["implemented"], true);
    assert_eq!(capabilities["runtime"]["pause"]["allowed"], false);

    for capability in [
        &capabilities["ui"]["click"],
        &capabilities["input"]["mouse_move"],
    ] {
        assert_eq!(capability["implemented"], true);
        assert_eq!(capability["available"], false);
        assert_eq!(capability["allowed"], false);
        assert_eq!(capability["operational"], false);
    }

    assert_eq!(capabilities["capture"]["viewport"]["implemented"], true);
    assert_eq!(capabilities["capture"]["viewport"]["available"], false);
    assert_eq!(capabilities["debugger"]["watchpoints"]["implemented"], true);
    assert!(
        capabilities["deprecations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["tool"] == "playtest_run" && entry["replacement"] == "playtest_start"
            })
    );
}

#[test]
fn capabilities_remain_discoverable_with_no_operational_permissions() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions {
                level: PermissionLevel::None,
                allow_input: false,
                allow_runtime_control: false,
                allow_build: false,
            }),
    );

    ingress.push(41, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 41);
    assert_eq!(capabilities["connected"], true);
    assert_eq!(capabilities["permissions"]["level"], "none");
    assert_eq!(capabilities["ecs"]["inspect"]["allowed"], false);
    assert_eq!(capabilities["ecs"]["inspect"]["operational"], false);
}

#[test]
fn full_permissions_expose_installed_raw_input_and_gamepad_command_works() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(ButtonInput::<MouseButton>::default());
    let gamepad = app.world_mut().spawn(Gamepad::default()).id();

    ingress.push(2, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 2);
    for key in ["key", "mouse_button", "gamepad_button"] {
        assert_eq!(capabilities["input"][key]["implemented"], true);
        assert_eq!(capabilities["input"][key]["available"], true);
        assert_eq!(capabilities["input"][key]["allowed"], true);
        assert_eq!(capabilities["input"][key]["operational"], true);
    }

    ingress.push(
        3,
        McpCommand::InputGamepad {
            button: "south".into(),
            pressed: true,
        },
    );
    app.update();
    let result = success_for(&results, 3);
    assert_eq!(result["button"], "south");
    assert_eq!(result["pressed"], true);
    assert!(
        app.world()
            .get::<Gamepad>(gamepad)
            .expect("mock gamepad should remain connected")
            .pressed(GamepadButton::South)
    );
}
