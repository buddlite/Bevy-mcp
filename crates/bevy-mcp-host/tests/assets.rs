use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::Value;

fn response_for(results: &McpResultQueue, request_id: u64) -> McpResult {
    results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response")
        .result
}

fn success_for(results: &McpResultQueue, request_id: u64) -> Value {
    match response_for(results, request_id) {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected {code}: {message}"),
    }
}

fn asset_app(permissions: McpPermissions) -> (App, McpIngressQueue, McpResultQueue) {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(permissions),
        );
    (app, ingress, results)
}

#[test]
fn asset_capabilities_reflect_asset_server_and_permissions() {
    let (mut app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(1, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 1);

    assert_eq!(capabilities["assets"]["list"]["implemented"], false);
    for key in ["inspect", "status"] {
        assert_eq!(capabilities["assets"][key]["implemented"], true);
        assert_eq!(capabilities["assets"][key]["available"], true);
        assert_eq!(capabilities["assets"][key]["allowed"], true);
        assert_eq!(capabilities["assets"][key]["operational"], true);
    }
    assert_eq!(capabilities["assets"]["reload"]["implemented"], true);
    assert_eq!(capabilities["assets"]["reload"]["available"], true);
    assert_eq!(capabilities["assets"]["reload"]["allowed"], false);
    assert_eq!(capabilities["assets"]["reload"]["operational"], false);
}

#[test]
fn asset_status_reports_unknown_path_as_not_loaded() {
    let (mut app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(
        2,
        McpCommand::AssetStatus {
            path: "textures/not-loaded.png".into(),
        },
    );
    app.update();
    let status = success_for(&results, 2);
    assert_eq!(status["path"], "textures/not-loaded.png");
    assert_eq!(status["active"], false);
    assert_eq!(status["status"], "not_loaded");
    assert_eq!(status["assets"].as_array().unwrap().len(), 0);
}

#[test]
fn asset_reload_requires_runtime_permission_and_an_active_loaded_path() {
    let (mut read_app, ingress, results) = asset_app(McpPermissions::read_only());
    ingress.push(
        3,
        McpCommand::AssetReload {
            path: "textures/not-loaded.png".into(),
        },
    );
    read_app.update();
    match response_for(&results, 3) {
        McpResult::Error { code, .. } => assert_eq!(code, "PERMISSION_DENIED"),
        result => panic!("expected permission denial, got {result:?}"),
    }

    let (mut full_app, ingress, results) = asset_app(McpPermissions::full());
    ingress.push(
        4,
        McpCommand::AssetReload {
            path: "textures/not-loaded.png".into(),
        },
    );
    full_app.update();
    match response_for(&results, 4) {
        McpResult::Error { code, .. } => assert_eq!(code, "ASSET_NOT_ACTIVE"),
        result => panic!("expected inactive-path error, got {result:?}"),
    }
}
