use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::debug::{
    DebugCondition, DebugPlaytestPlan, DebugPlaytestStep, DebugRequest, EvidenceOptions,
    WatchpointSpec, encode_debug_request,
};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::Value;

fn send_debug(ingress: &McpIngressQueue, request_id: u64, request: DebugRequest) {
    let operation_id = encode_debug_request(&request).expect("debug request should serialize");
    ingress.push(
        request_id,
        McpCommand::OperationStatus {
            operation_id: Some(operation_id),
        },
    );
}

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

fn no_screenshot_evidence() -> EvidenceOptions {
    EvidenceOptions {
        changes_frames: 10,
        logs_limit: 0,
        events_limit: 0,
        include_states: false,
        include_system_timings: false,
        screenshot: false,
    }
}

#[test]
fn watchpoint_triggers_on_rising_edge_and_disables_when_once() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::read_only()),
    );

    send_debug(
        &ingress,
        1,
        DebugRequest::WatchpointAdd {
            spec: WatchpointSpec {
                name: "frame-zero".into(),
                condition: DebugCondition::FrameAtLeast { frame: 0 },
                pause_on_trigger: false,
                once: true,
                evidence: no_screenshot_evidence(),
            },
        },
    );
    app.update();
    let add = success_for(&results, 1);
    assert_eq!(add["enabled"], true);

    send_debug(&ingress, 2, DebugRequest::WatchpointList);
    app.update();
    let list = success_for(&results, 2);
    let watchpoint = &list["watchpoints"][0];
    assert_eq!(watchpoint["name"], "frame-zero");
    assert_eq!(watchpoint["trigger_count"], 1);
    assert_eq!(watchpoint["enabled"], false);
    assert!(watchpoint["evidence"].is_object());
}

#[test]
fn frame_driven_playtest_completes_without_blocking_ingress() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    send_debug(
        &ingress,
        10,
        DebugRequest::PlaytestStart {
            plan: DebugPlaytestPlan {
                name: "frame-smoke-test".into(),
                steps: vec![
                    DebugPlaytestStep::StepFrames { frames: 1 },
                    DebugPlaytestStep::Assert {
                        condition: DebugCondition::FrameAtLeast { frame: 1 },
                        message: Some("frame should advance".into()),
                    },
                ],
                pause_on_failure: true,
                evidence: no_screenshot_evidence(),
            },
        },
    );

    app.update();
    let started = success_for(&results, 10);
    let id = started["id"].as_str().expect("playtest id").to_owned();
    assert_eq!(started["status"], "running");

    app.update();

    send_debug(
        &ingress,
        11,
        DebugRequest::PlaytestStatus { id: id.clone() },
    );
    app.update();
    let status = success_for(&results, 11);
    assert_eq!(status["id"], id);
    assert_eq!(status["status"], "passed");
    assert_eq!(status["step_index"], 2);
    assert_eq!(status["steps_total"], 2);
}
