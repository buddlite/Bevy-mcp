use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::prelude::*;
use bevy::reflect::serde::ReflectSerializer;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy_mcp_core::advanced::{AdvancedEntityQuery, QueryCondition};
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};
use bevy_mcp_core::debug::{
    DebugCondition, DebugPlaytestPlan, DebugPlaytestStep, DebugRequest, EvidenceOptions,
    WatchpointSpec, decode_debug_request,
};
use serde_json::{Value, json};

use crate::agent_api::{McpActionRegistry, McpStateRegistry, McpSystemTimings};
use crate::change_tracking::WorldChangeTracker;
use crate::checkpoint::{
    McpCheckpointRegistry, McpCheckpointStore, McpRecorder, RecordedAction, ReplayStatus,
    StoredCheckpoint,
};
use crate::entity_handle::resolve_entity;
use crate::event_capture::EventCapture;
use crate::log_capture::LogCapture;
use crate::permissions::{McpPermissions, PermissionLevel};
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaytestStatus {
    Running,
    Passed,
    Failed,
    Cancelled,
}

impl PlaytestStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
struct WatchpointRuntime {
    id: String,
    spec: WatchpointSpec,
    enabled: bool,
    trigger_count: u64,
    was_matched: bool,
    last_trigger_frame: Option<u64>,
    last_evaluation: Option<Value>,
    last_error: Option<String>,
    evidence: Option<Value>,
}

#[derive(Debug, Clone)]
struct PlaytestRuntime {
    id: String,
    plan: DebugPlaytestPlan,
    status: PlaytestStatus,
    step_index: usize,
    step_started_frame: Option<u64>,
    started_frame: u64,
    finished_frame: Option<u64>,
    step_results: Vec<Value>,
    failure: Option<Value>,
    evidence: Option<Value>,
    captures: Vec<String>,
}

#[derive(Resource, Default)]
pub struct McpDebugger {
    next_id: u64,
    watchpoints: HashMap<String, WatchpointRuntime>,
    playtests: HashMap<String, PlaytestRuntime>,
    captures: HashMap<String, Value>,
}

impl McpDebugger {
    fn allocate_id(&mut self, prefix: &str) -> String {
        self.next_id = self.next_id.saturating_add(1);
        format!("{prefix}-{}", self.next_id)
    }
}

#[derive(Debug)]
struct ConditionEvaluation {
    matched: bool,
    actual: Value,
}

/// Intercepts debugger protocol requests before the advanced and legacy ingress systems.
pub fn debug_ingress_system(world: &mut World) {
    let entries = world.resource::<McpIngressQueue>().drain();
    for entry in entries {
        let request_id = entry.request_id;
        match entry.command {
            McpCommand::OperationStatus {
                operation_id: Some(operation_id),
            } => match decode_debug_request(&operation_id) {
                Some(Ok(request)) => handle_debug_request(world, request_id, request),
                Some(Err(error)) => push_error(
                    world,
                    request_id,
                    "INVALID_DEBUG_REQUEST",
                    error.to_string(),
                ),
                None => world.resource::<McpIngressQueue>().push(
                    request_id,
                    McpCommand::OperationStatus {
                        operation_id: Some(operation_id),
                    },
                ),
            },
            command => world
                .resource::<McpIngressQueue>()
                .push(request_id, command),
        }
    }
}

fn handle_debug_request(world: &mut World, request_id: u64, request: DebugRequest) {
    let permissions = world.resource::<McpPermissions>().clone();
    if !debug_request_allowed(&request, &permissions) {
        push_error(
            world,
            request_id,
            "PERMISSION_DENIED",
            "The configured MCP permissions do not allow this debugger operation",
        );
        return;
    }

    match request {
        DebugRequest::WatchpointAdd { spec } => {
            let mut debugger = world.resource_mut::<McpDebugger>();
            let id = debugger.allocate_id("watch");
            debugger.watchpoints.insert(
                id.clone(),
                WatchpointRuntime {
                    id: id.clone(),
                    spec: spec.clone(),
                    enabled: true,
                    trigger_count: 0,
                    was_matched: false,
                    last_trigger_frame: None,
                    last_evaluation: None,
                    last_error: None,
                    evidence: None,
                },
            );
            drop(debugger);
            register_condition_tracking_interests(world, &spec.condition);
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "id": id, "name": spec.name, "enabled": true })),
            );
        }
        DebugRequest::WatchpointList => {
            let debugger = world.resource::<McpDebugger>();
            let mut rows: Vec<Value> = debugger
                .watchpoints
                .values()
                .map(|watchpoint| watchpoint_json(watchpoint, &debugger))
                .collect();
            rows.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "watchpoints": rows })),
            );
        }
        DebugRequest::WatchpointRemove { id } => {
            let removed = world
                .resource_mut::<McpDebugger>()
                .watchpoints
                .remove(&id)
                .is_some();
            if removed {
                push_result(
                    world,
                    request_id,
                    McpResult::success(json!({ "removed": id })),
                );
            } else {
                push_error(
                    world,
                    request_id,
                    "WATCHPOINT_NOT_FOUND",
                    format!("Watchpoint '{id}' not found"),
                );
            }
        }
        DebugRequest::WatchpointClear => {
            let count = world.resource::<McpDebugger>().watchpoints.len();
            world.resource_mut::<McpDebugger>().watchpoints.clear();
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "cleared": count })),
            );
        }
        DebugRequest::PlaytestStart { plan } => start_playtest(world, request_id, plan),
        DebugRequest::PlaytestStatus { id } => {
            let debugger = world.resource::<McpDebugger>();
            match debugger.playtests.get(&id) {
                Some(session) => push_result(
                    world,
                    request_id,
                    McpResult::success(playtest_json(session, &debugger)),
                ),
                None => push_error(
                    world,
                    request_id,
                    "PLAYTEST_NOT_FOUND",
                    format!("Playtest '{id}' not found"),
                ),
            }
        }
        DebugRequest::PlaytestList => {
            let debugger = world.resource::<McpDebugger>();
            let mut rows: Vec<Value> = debugger
                .playtests
                .values()
                .map(|session| playtest_json(session, &debugger))
                .collect();
            rows.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "playtests": rows })),
            );
        }
        DebugRequest::PlaytestCancel { id } => {
            let frame = current_frame(world);
            let mut debugger = world.resource_mut::<McpDebugger>();
            match debugger.playtests.get_mut(&id) {
                Some(session) if session.status == PlaytestStatus::Running => {
                    session.status = PlaytestStatus::Cancelled;
                    session.finished_frame = Some(frame);
                    push_result(
                        world,
                        request_id,
                        McpResult::success(json!({ "cancelled": id })),
                    );
                }
                Some(_) => push_error(
                    world,
                    request_id,
                    "PLAYTEST_NOT_RUNNING",
                    format!("Playtest '{id}' is not running"),
                ),
                None => push_error(
                    world,
                    request_id,
                    "PLAYTEST_NOT_FOUND",
                    format!("Playtest '{id}' not found"),
                ),
            }
        }
        DebugRequest::CheckpointCreate { name } => {
            let frame = current_frame(world);
            let captured = world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| {
                registry.capture(world)
            });
            match captured {
                Ok(values) => {
                    let mut store = world.resource_mut::<McpCheckpointStore>();
                    let id = store.next_id();
                    store.insert(StoredCheckpoint {
                        id: id.clone(),
                        name: name.clone(),
                        frame,
                        values,
                    });
                    drop(store);
                    let coverage = world.resource::<McpCheckpointRegistry>().coverage();
                    push_result(
                        world,
                        request_id,
                        McpResult::success(
                            json!({ "id": id, "name": name, "frame": frame, "coverage": coverage }),
                        ),
                    );
                }
                Err(error) => push_error(world, request_id, "CHECKPOINT_CAPTURE_FAILED", error),
            }
        }
        DebugRequest::CheckpointList => {
            let checkpoints = world.resource::<McpCheckpointStore>().list();
            let coverage = world.resource::<McpCheckpointRegistry>().coverage();
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "checkpoints": checkpoints, "coverage": coverage })),
            );
        }
        DebugRequest::CheckpointRestore { id } => {
            let checkpoint = world.resource::<McpCheckpointStore>().get(&id).cloned();
            match checkpoint {
                Some(checkpoint) => {
                    let restored =
                        world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| {
                            registry.restore(world, &checkpoint.values)
                        });
                    match restored {
                        Ok(()) => push_result(
                            world,
                            request_id,
                            McpResult::success(
                                json!({ "restored": id, "source_frame": checkpoint.frame, "frame": current_frame(world) }),
                            ),
                        ),
                        Err(error) => {
                            push_error(world, request_id, "CHECKPOINT_RESTORE_FAILED", error)
                        }
                    }
                }
                None => push_error(
                    world,
                    request_id,
                    "CHECKPOINT_NOT_FOUND",
                    format!("Checkpoint '{id}' not found"),
                ),
            }
        }
        DebugRequest::RecordingStart { name } => {
            let frame = current_frame(world);
            let result = world.resource_mut::<McpRecorder>().start(name, frame);
            match result {
                Ok(id) => push_result(
                    world,
                    request_id,
                    McpResult::success(json!({ "id": id, "start_frame": frame })),
                ),
                Err(error) => push_error(world, request_id, "RECORDING_START_FAILED", error),
            }
        }
        DebugRequest::RecordingStop => match world.resource_mut::<McpRecorder>().stop() {
            Ok(recording) => push_result(
                world,
                request_id,
                McpResult::success(
                    json!({ "id": recording.id, "name": recording.name, "events": recording.events.len() }),
                ),
            ),
            Err(error) => push_error(world, request_id, "RECORDING_STOP_FAILED", error),
        },
        DebugRequest::RecordingList => {
            let rows = world.resource::<McpRecorder>().list_recordings();
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "recordings": rows })),
            );
        }
        DebugRequest::ReplayStart {
            recording_id,
            checkpoint_id,
        } => {
            if let Err(error) = world
                .resource::<McpRecorder>()
                .validate_replay_start(&recording_id)
            {
                push_error(world, request_id, "REPLAY_START_FAILED", error);
                return;
            }

            if let Some(checkpoint_id) = checkpoint_id.as_ref() {
                let checkpoint = world
                    .resource::<McpCheckpointStore>()
                    .get(checkpoint_id)
                    .cloned();
                let Some(checkpoint) = checkpoint else {
                    push_error(
                        world,
                        request_id,
                        "CHECKPOINT_NOT_FOUND",
                        format!("Checkpoint '{checkpoint_id}' not found"),
                    );
                    return;
                };
                if let Err(error) =
                    world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| {
                        registry.restore(world, &checkpoint.values)
                    })
                {
                    push_error(world, request_id, "CHECKPOINT_RESTORE_FAILED", error);
                    return;
                }
            }
            let frame = current_frame(world);
            match world.resource_mut::<McpRecorder>().start_replay(
                recording_id,
                checkpoint_id,
                frame,
            ) {
                Ok(id) => push_result(
                    world,
                    request_id,
                    McpResult::success(
                        json!({ "id": id, "status": "running", "start_frame": frame }),
                    ),
                ),
                Err(error) => push_error(world, request_id, "REPLAY_START_FAILED", error),
            }
        }
        DebugRequest::ReplayStatus { id } => {
            match world.resource::<McpRecorder>().replay_json(&id) {
                Some(value) => push_result(world, request_id, McpResult::success(value)),
                None => push_error(
                    world,
                    request_id,
                    "REPLAY_NOT_FOUND",
                    format!("Replay '{id}' not found"),
                ),
            }
        }
        DebugRequest::ReplayCancel { id } => {
            let mut recorder = world.resource_mut::<McpRecorder>();
            match recorder.replays.get_mut(&id) {
                Some(replay) if replay.status == ReplayStatus::Running => {
                    replay.status = ReplayStatus::Cancelled;
                    push_result(
                        world,
                        request_id,
                        McpResult::success(json!({ "cancelled": id })),
                    );
                }
                Some(_) => push_error(
                    world,
                    request_id,
                    "REPLAY_NOT_RUNNING",
                    format!("Replay '{id}' is not running"),
                ),
                None => push_error(
                    world,
                    request_id,
                    "REPLAY_NOT_FOUND",
                    format!("Replay '{id}' not found"),
                ),
            }
        }
    }
}

fn debug_request_allowed(request: &DebugRequest, permissions: &McpPermissions) -> bool {
    match request {
        DebugRequest::PlaytestStart { .. }
        | DebugRequest::PlaytestCancel { .. }
        | DebugRequest::ReplayStart { .. }
        | DebugRequest::ReplayCancel { .. } => {
            permissions.can_control_runtime() && permissions.can_inject_input()
        }
        DebugRequest::CheckpointRestore { .. } => permissions.can_mutate(),
        DebugRequest::WatchpointAdd { spec } if spec.pause_on_trigger => {
            permissions.can_control_runtime()
        }
        _ => permissions.level != PermissionLevel::None,
    }
}

fn start_playtest(world: &mut World, request_id: u64, plan: DebugPlaytestPlan) {
    let frame = current_frame(world);
    let mut debugger = world.resource_mut::<McpDebugger>();
    if let Some(active) = debugger
        .playtests
        .values()
        .find(|session| session.status == PlaytestStatus::Running)
    {
        let active_id = active.id.clone();
        drop(debugger);
        push_error(
            world,
            request_id,
            "PLAYTEST_ALREADY_RUNNING",
            format!("Playtest '{active_id}' is already running"),
        );
        return;
    }

    let id = debugger.allocate_id("playtest");
    let step_count = plan.steps.len();
    debugger.playtests.insert(
        id.clone(),
        PlaytestRuntime {
            id: id.clone(),
            plan: plan.clone(),
            status: PlaytestStatus::Running,
            step_index: 0,
            step_started_frame: None,
            started_frame: frame,
            finished_frame: None,
            step_results: Vec::new(),
            failure: None,
            evidence: None,
            captures: Vec::new(),
        },
    );
    drop(debugger);

    for step in &plan.steps {
        match step {
            DebugPlaytestStep::Wait { condition, .. }
            | DebugPlaytestStep::Assert { condition, .. } => {
                register_condition_tracking_interests(world, condition)
            }
            _ => {}
        }
    }

    push_result(
        world,
        request_id,
        McpResult::success(json!({
            "id": id,
            "name": plan.name,
            "status": "running",
            "started_frame": frame,
            "steps": step_count,
        })),
    );
}

/// Evaluates watchpoints and advances the active playtest after the game has updated.
pub fn debug_tick_system(world: &mut World) {
    let frame = current_frame(world);
    let mut debugger = world.remove_resource::<McpDebugger>().unwrap_or_default();

    let watch_ids: Vec<String> = debugger.watchpoints.keys().cloned().collect();
    for id in watch_ids {
        let Some(mut watchpoint) = debugger.watchpoints.remove(&id) else {
            continue;
        };
        if watchpoint.enabled {
            match evaluate_condition(world, &watchpoint.spec.condition, frame) {
                Ok(evaluation) => {
                    watchpoint.last_error = None;
                    watchpoint.last_evaluation = Some(evaluation.actual.clone());
                    if evaluation.matched && !watchpoint.was_matched {
                        watchpoint.trigger_count = watchpoint.trigger_count.saturating_add(1);
                        watchpoint.last_trigger_frame = Some(frame);
                        if watchpoint.spec.pause_on_trigger {
                            world.resource_mut::<McpRegistry>().paused = true;
                        }
                        watchpoint.evidence = Some(collect_evidence(
                            world,
                            &mut debugger,
                            frame,
                            &watchpoint.spec.evidence,
                            &format!("watchpoint-{}", watchpoint.id),
                        ));
                        if watchpoint.spec.once {
                            watchpoint.enabled = false;
                        }
                    }
                    watchpoint.was_matched = evaluation.matched;
                }
                Err(error) => {
                    watchpoint.last_error = Some(error);
                    watchpoint.was_matched = false;
                }
            }
        }
        debugger.watchpoints.insert(id, watchpoint);
    }

    let running: Vec<String> = debugger
        .playtests
        .values()
        .filter(|session| session.status == PlaytestStatus::Running)
        .map(|session| session.id.clone())
        .collect();
    for id in running {
        let Some(mut session) = debugger.playtests.remove(&id) else {
            continue;
        };
        advance_playtest(world, &mut debugger, &mut session, frame);
        debugger.playtests.insert(id, session);
    }

    tick_replays(world, frame);
    world.insert_resource(debugger);
    reconcile_dynamic_tracking_interests(world);
}

fn tick_replays(world: &mut World, frame: u64) {
    let mut recorder = world.remove_resource::<McpRecorder>().unwrap_or_default();
    let ids: Vec<String> = recorder
        .replays
        .iter()
        .filter(|(_, r)| r.status == ReplayStatus::Running)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        let Some(mut replay) = recorder.replays.remove(&id) else {
            continue;
        };
        let Some(recording) = recorder.recordings.get(&replay.recording_id).cloned() else {
            replay.status = ReplayStatus::Failed;
            replay.failure = Some(format!("Recording '{}' disappeared", replay.recording_id));
            recorder.replays.insert(id, replay);
            continue;
        };
        while let Some(event) = recording.events.get(replay.next_event) {
            if frame.saturating_sub(replay.start_frame) < event.offset_frames {
                break;
            }
            let result = match &event.action {
                RecordedAction::SemanticAction { action, args } => world
                    .resource_scope(|world, actions: Mut<McpActionRegistry>| {
                        actions.invoke(action, world, args.clone())
                    })
                    .map(|_| ()),
                RecordedAction::StateTransition { state, value } => world
                    .resource_scope(|world, states: Mut<McpStateRegistry>| {
                        states.set(state, world, value.clone())
                    })
                    .map(|_| ()),
                RecordedAction::Key { key, pressed } => apply_key(world, key, *pressed),
            };
            if let Err(error) = result {
                replay.status = ReplayStatus::Failed;
                replay.failure = Some(error);
                break;
            }
            replay.next_event += 1;
        }
        if replay.status == ReplayStatus::Running && replay.next_event >= recording.events.len() {
            replay.status = ReplayStatus::Passed;
        }
        recorder.replays.insert(id, replay);
    }
    world.insert_resource(recorder);
}

fn advance_playtest(
    world: &mut World,
    debugger: &mut McpDebugger,
    session: &mut PlaytestRuntime,
    frame: u64,
) {
    // Allow chains of immediate actions/assertions in one frame without allowing a malformed
    // plan to monopolize the schedule.
    for _ in 0..64 {
        if session.step_index >= session.plan.steps.len() {
            session.status = PlaytestStatus::Passed;
            session.finished_frame = Some(frame);
            return;
        }

        let step = session.plan.steps[session.step_index].clone();
        match step {
            DebugPlaytestStep::SemanticAction { action, args } => {
                let requested_args = args.clone();
                let result = world.resource_scope(|world, actions: Mut<McpActionRegistry>| {
                    actions.invoke(&action, world, args)
                });
                match result {
                    Ok(value) => {
                        world.resource_mut::<McpRecorder>().record(
                            frame,
                            RecordedAction::SemanticAction {
                                action: action.clone(),
                                args: requested_args,
                            },
                        );
                        complete_step(
                            session,
                            frame,
                            json!({ "type": "semantic_action", "action": action, "result": value }),
                        );
                    }
                    Err(error) => {
                        fail_playtest(world, debugger, session, frame, "ACTION_FAILED", error);
                        return;
                    }
                }
            }
            DebugPlaytestStep::StateTransition { state, value } => {
                let requested_value = value.clone();
                let result = world.resource_scope(|world, states: Mut<McpStateRegistry>| {
                    states.set(&state, world, value)
                });
                match result {
                    Ok(result_value) => {
                        world.resource_mut::<McpRecorder>().record(
                            frame,
                            RecordedAction::StateTransition {
                                state: state.clone(),
                                value: requested_value,
                            },
                        );
                        complete_step(
                            session,
                            frame,
                            json!({ "type": "state_transition", "state": state, "result": result_value }),
                        );
                    }
                    Err(error) => {
                        fail_playtest(
                            world,
                            debugger,
                            session,
                            frame,
                            "STATE_TRANSITION_FAILED",
                            error,
                        );
                        return;
                    }
                }
            }
            DebugPlaytestStep::Key { key, pressed } => match apply_key(world, &key, pressed) {
                Ok(()) => {
                    world.resource_mut::<McpRecorder>().record(
                        frame,
                        RecordedAction::Key {
                            key: key.clone(),
                            pressed,
                        },
                    );
                    complete_step(
                        session,
                        frame,
                        json!({ "type": "key", "key": key, "pressed": pressed }),
                    );
                }
                Err(error) => {
                    fail_playtest(world, debugger, session, frame, "INPUT_FAILED", error);
                    return;
                }
            },
            DebugPlaytestStep::StepFrames { frames } => {
                let started = *session.step_started_frame.get_or_insert(frame);
                let elapsed = frame.saturating_sub(started);
                if elapsed >= frames as u64 {
                    complete_step(
                        session,
                        frame,
                        json!({
                            "type": "step_frames",
                            "frames": frames,
                            "elapsed_frames": elapsed,
                        }),
                    );
                } else {
                    return;
                }
            }
            DebugPlaytestStep::Wait {
                condition,
                timeout_frames,
            } => {
                let started = *session.step_started_frame.get_or_insert(frame);
                match evaluate_condition(world, &condition, frame) {
                    Ok(evaluation) if evaluation.matched => {
                        complete_step(
                            session,
                            frame,
                            json!({
                                "type": "wait",
                                "matched": true,
                                "actual": evaluation.actual,
                                "elapsed_frames": frame.saturating_sub(started),
                            }),
                        );
                    }
                    Ok(evaluation) => {
                        if frame.saturating_sub(started) >= timeout_frames as u64 {
                            fail_playtest(
                                world,
                                debugger,
                                session,
                                frame,
                                "WAIT_TIMEOUT",
                                format!(
                                    "Condition did not match within {timeout_frames} frames; actual={}",
                                    evaluation.actual
                                ),
                            );
                        }
                        return;
                    }
                    Err(error) => {
                        fail_playtest(
                            world,
                            debugger,
                            session,
                            frame,
                            "WAIT_EVALUATION_FAILED",
                            error,
                        );
                        return;
                    }
                }
            }
            DebugPlaytestStep::Assert { condition, message } => {
                match evaluate_condition(world, &condition, frame) {
                    Ok(evaluation) if evaluation.matched => complete_step(
                        session,
                        frame,
                        json!({
                            "type": "assert",
                            "matched": true,
                            "actual": evaluation.actual,
                        }),
                    ),
                    Ok(evaluation) => {
                        fail_playtest(
                            world,
                            debugger,
                            session,
                            frame,
                            "ASSERTION_FAILED",
                            message.unwrap_or_else(|| {
                                format!("Assertion did not match; actual={}", evaluation.actual)
                            }),
                        );
                        return;
                    }
                    Err(error) => {
                        fail_playtest(
                            world,
                            debugger,
                            session,
                            frame,
                            "ASSERTION_EVALUATION_FAILED",
                            error,
                        );
                        return;
                    }
                }
            }
            DebugPlaytestStep::Capture { name } => {
                let capture_id = debugger.allocate_id("capture");
                let capture = start_debug_capture(
                    world,
                    &capture_id,
                    name.as_deref()
                        .unwrap_or(&format!("{}-step-{}", session.id, session.step_index)),
                );
                debugger.captures.insert(capture_id.clone(), capture);
                session.captures.push(capture_id.clone());
                complete_step(
                    session,
                    frame,
                    json!({
                        "type": "capture",
                        "capture_id": capture_id,
                    }),
                );
            }
        }
    }

    fail_playtest(
        world,
        debugger,
        session,
        frame,
        "PLAYTEST_STEP_BUDGET_EXCEEDED",
        "More than 64 immediate playtest steps were attempted in one frame",
    );
}

fn complete_step(session: &mut PlaytestRuntime, frame: u64, result: Value) {
    session.step_results.push(json!({
        "step": session.step_index,
        "frame": frame,
        "result": result,
    }));
    session.step_index += 1;
    session.step_started_frame = None;
}

fn fail_playtest(
    world: &mut World,
    debugger: &mut McpDebugger,
    session: &mut PlaytestRuntime,
    frame: u64,
    code: &str,
    message: impl Into<String>,
) {
    let message = message.into();
    session.status = PlaytestStatus::Failed;
    session.finished_frame = Some(frame);
    session.failure = Some(json!({
        "code": code,
        "message": message,
        "step": session.step_index,
        "frame": frame,
    }));
    if session.plan.pause_on_failure {
        world.resource_mut::<McpRegistry>().paused = true;
    }
    session.evidence = Some(collect_evidence(
        world,
        debugger,
        frame,
        &session.plan.evidence,
        &format!("playtest-{}-failure", session.id),
    ));
}

fn evaluate_condition(
    world: &World,
    condition: &DebugCondition,
    frame: u64,
) -> Result<ConditionEvaluation, String> {
    match condition {
        DebugCondition::EntityExists { entity } => {
            let resolved = resolve_entity(world, entity);
            Ok(ConditionEvaluation {
                matched: resolved.is_some(),
                actual: json!({
                    "entity": entity.to_string(),
                    "exists": resolved.is_some(),
                }),
            })
        }
        DebugCondition::QueryCount { query, condition } => {
            let count = query_count(world, query)? as u64;
            Ok(ConditionEvaluation {
                matched: compare_json(&json!(count), &condition.op, &condition.value)?,
                actual: json!({ "count": count }),
            })
        }
        DebugCondition::EntityField {
            entity,
            component,
            field,
            condition,
        } => {
            let entity_id = resolve_entity(world, entity)
                .ok_or_else(|| format!("Entity {entity} not found"))?;
            let entity_ref = world
                .get_entity(entity_id)
                .map_err(|_| format!("Entity {entity} not found"))?;
            let value =
                reflected_component_json(world, &entity_ref, component)?.ok_or_else(|| {
                    format!("Entity {entity} does not expose reflected component '{component}'")
                })?;
            let actual = if field.is_empty() {
                &value
            } else {
                json_path(&value, field).ok_or_else(|| {
                    format!("Field '{field}' not found in component '{component}'")
                })?
            };
            Ok(ConditionEvaluation {
                matched: compare_json(actual, &condition.op, &condition.value)?,
                actual: json!({
                    "entity": entity.to_string(),
                    "component": component,
                    "field": field,
                    "value": actual,
                }),
            })
        }
        DebugCondition::ResourceField {
            resource,
            field,
            condition,
        } => {
            let value = reflected_resource_json(world, resource)?;
            let actual = if field.is_empty() {
                &value
            } else {
                json_path(&value, field)
                    .ok_or_else(|| format!("Field '{field}' not found in resource '{resource}'"))?
            };
            Ok(ConditionEvaluation {
                matched: compare_json(actual, &condition.op, &condition.value)?,
                actual: json!({
                    "resource": resource,
                    "field": field,
                    "value": actual,
                }),
            })
        }
        DebugCondition::StateEquals { state, value } => {
            let states = world
                .get_resource::<McpStateRegistry>()
                .ok_or_else(|| "McpStateRegistry is not available".to_string())?;
            let actual = states.get(state, world)?;
            Ok(ConditionEvaluation {
                matched: actual == *value,
                actual: json!({ "state": state, "value": actual }),
            })
        }
        DebugCondition::LogContains { level, text } => {
            let capture = world
                .get_resource::<LogCapture>()
                .ok_or_else(|| "LogCapture is not available".to_string())?;
            let needle = text.to_lowercase();
            let entries = capture.get_entries(level.as_deref(), 1000);
            let matching = entries
                .iter()
                .find(|entry| entry.message.to_lowercase().contains(&needle));
            Ok(ConditionEvaluation {
                matched: matching.is_some(),
                actual: matching
                    .map(|entry| {
                        json!({
                            "level": entry.level,
                            "message": entry.message,
                            "target": entry.target,
                            "timestamp": entry.timestamp,
                        })
                    })
                    .unwrap_or(Value::Null),
            })
        }
        DebugCondition::ChangeOccurred {
            entity,
            component,
            resource,
        } => evaluate_change_condition(
            world,
            frame,
            entity.as_ref(),
            component.as_deref(),
            resource.as_deref(),
        ),
        DebugCondition::FrameAtLeast { frame: target } => Ok(ConditionEvaluation {
            matched: frame >= *target,
            actual: json!({ "frame": frame, "target": target }),
        }),
    }
}

fn evaluate_change_condition(
    world: &World,
    frame: u64,
    entity: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
    component: Option<&str>,
    resource: Option<&str>,
) -> Result<ConditionEvaluation, String> {
    let tracker = world
        .get_resource::<WorldChangeTracker>()
        .ok_or_else(|| "WorldChangeTracker is not available".to_string())?;
    let since = frame.saturating_sub(1);

    if let Some(resource) = resource {
        let actual = tracker.resource_changes_since(since, Some(resource));
        let matched = actual["changes"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty());
        return Ok(ConditionEvaluation { matched, actual });
    }

    let resolved_entity = match entity {
        Some(handle) => Some(
            resolve_entity(world, handle).ok_or_else(|| format!("Entity {handle} not found"))?,
        ),
        None => None,
    };

    if resolved_entity.is_some() {
        let actual = tracker.entity_changes_since(since, resolved_entity);
        let matched = if let Some(component) = component {
            actual["components"].as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row["component"]
                        .as_str()
                        .is_some_and(|name| component_name_matches(name, component))
                })
            })
        } else {
            actual["spawned"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
                || actual["despawned"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
                || actual["components"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
        };
        return Ok(ConditionEvaluation { matched, actual });
    }

    if let Some(component) = component {
        let actual = tracker.component_changes_since(since, Some(component));
        let matched = actual["changes"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty());
        return Ok(ConditionEvaluation { matched, actual });
    }

    let actual = tracker.changes_since(since);
    let matched = actual["frames"].as_array().is_some_and(|frames| {
        frames.iter().any(|entry| {
            entry["spawned"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
                || entry["despawned"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
                || entry["components"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
                || entry["resources"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
        })
    });
    Ok(ConditionEvaluation { matched, actual })
}

fn query_count(world: &World, query: &AdvancedEntityQuery) -> Result<usize, String> {
    let with_ids = resolve_component_ids(world, &query.with_components)?;
    let without_ids = resolve_component_ids(world, &query.without_components)?;
    let parent_ids = resolve_component_ids(world, &query.parent_has)?;
    let child_ids = resolve_component_ids(world, &query.child_has)?;
    let tracker = world.get_resource::<WorldChangeTracker>();
    let mut count = 0usize;

    for entity_ref in world.iter_entities() {
        if entity_ref.contains::<bevy::ecs::resource::IsResource>() {
            continue;
        }
        if !with_ids.iter().all(|id| entity_ref.contains_id(*id))
            || without_ids.iter().any(|id| entity_ref.contains_id(*id))
        {
            continue;
        }
        if query.name_contains.as_ref().is_some_and(|needle| {
            entity_ref.get::<Name>().is_none_or(|name| {
                !name
                    .as_str()
                    .to_lowercase()
                    .contains(&needle.to_lowercase())
            })
        }) {
            continue;
        }
        if !query.changed.iter().all(|component| {
            tracker.is_some_and(|tracker| {
                tracker.component_changed_last_frame(entity_ref.id(), component)
            })
        }) {
            continue;
        }
        if !parent_matches(world, &entity_ref, &parent_ids)
            || !children_match(world, &entity_ref, &child_ids)
        {
            continue;
        }
        if !query.predicates.iter().all(|(path, condition)| {
            predicate_matches(world, &entity_ref, path, condition).unwrap_or(false)
        }) {
            continue;
        }
        count += 1;
        let limit = if query.limit == 0 {
            usize::MAX
        } else {
            query.limit as usize
        };
        if count >= limit {
            break;
        }
    }
    Ok(count)
}

fn parent_matches(
    world: &World,
    entity_ref: &EntityRef<'_>,
    required: &[bevy::ecs::component::ComponentId],
) -> bool {
    if required.is_empty() {
        return true;
    }
    let Some(parent) = entity_ref.get::<ChildOf>().map(ChildOf::parent) else {
        return false;
    };
    let Ok(parent_ref) = world.get_entity(parent) else {
        return false;
    };
    required.iter().all(|id| parent_ref.contains_id(*id))
}

fn children_match(
    world: &World,
    entity_ref: &EntityRef<'_>,
    required: &[bevy::ecs::component::ComponentId],
) -> bool {
    if required.is_empty() {
        return true;
    }
    let Some(children) = entity_ref.get::<Children>() else {
        return false;
    };
    required.iter().all(|required_id| {
        children.iter().any(|child| {
            world
                .get_entity(child)
                .is_ok_and(|child_ref| child_ref.contains_id(*required_id))
        })
    })
}

fn predicate_matches(
    world: &World,
    entity_ref: &EntityRef<'_>,
    path: &str,
    condition: &QueryCondition,
) -> Result<bool, String> {
    let (component, field_path) = path
        .split_once('.')
        .ok_or_else(|| format!("Predicate '{path}' must use Component.field notation"))?;
    let Some(value) = reflected_component_json(world, entity_ref, component)? else {
        return Ok(false);
    };
    let Some(actual) = json_path(&value, field_path) else {
        return Ok(false);
    };
    compare_json(actual, &condition.op, &condition.value)
}

fn reflected_component_json(
    world: &World,
    entity_ref: &EntityRef<'_>,
    requested: &str,
) -> Result<Option<Value>, String> {
    let app_registry = world
        .get_resource::<AppTypeRegistry>()
        .ok_or_else(|| "AppTypeRegistry is not available".to_string())?;
    let registry = app_registry.read();
    let Some(registration) = registry.iter().find(|registration| {
        let path = registration.type_info().type_path_table();
        path.short_path() == requested || path.path() == requested
    }) else {
        return Ok(None);
    };
    let Some(reflect_component) = registration.data::<bevy::ecs::reflect::ReflectComponent>()
    else {
        return Ok(None);
    };
    let Some(reflected) = reflect_component.reflect(*entity_ref) else {
        return Ok(None);
    };
    let serializer = ReflectSerializer::new(reflected.as_reflect(), &registry);
    let serialized = serde_json::to_value(&serializer).map_err(|error| error.to_string())?;
    Ok(Some(unwrap_reflect_value(serialized)))
}

fn reflected_resource_json(world: &World, requested: &str) -> Result<Value, String> {
    let app_registry = world
        .get_resource::<AppTypeRegistry>()
        .ok_or_else(|| "AppTypeRegistry is not available".to_string())?;
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| {
            let path = registration.type_info().type_path_table();
            path.short_path() == requested || path.path() == requested
        })
        .ok_or_else(|| format!("Resource '{requested}' is not registered"))?;
    let type_id = registration.type_id();
    let reflect_from_ptr = registration
        .data::<bevy::reflect::ReflectFromPtr>()
        .ok_or_else(|| format!("Resource '{requested}' does not expose ReflectFromPtr"))?;

    for (info, ptr) in world.iter_resources() {
        if info.type_id() == Some(type_id) {
            let reflected = unsafe { reflect_from_ptr.as_reflect(ptr) };
            let serializer = ReflectSerializer::new(reflected, &registry);
            let serialized =
                serde_json::to_value(&serializer).map_err(|error| error.to_string())?;
            return Ok(unwrap_reflect_value(serialized));
        }
    }
    Err(format!(
        "Resource '{requested}' is not present in the world"
    ))
}

fn resolve_component_ids(
    world: &World,
    names: &[String],
) -> Result<Vec<bevy::ecs::component::ComponentId>, String> {
    let app_registry = world
        .get_resource::<AppTypeRegistry>()
        .ok_or_else(|| "AppTypeRegistry is not available".to_string())?;
    let registry = app_registry.read();
    names
        .iter()
        .map(|name| {
            registry
                .iter()
                .find(|registration| {
                    let path = registration.type_info().type_path_table();
                    path.short_path() == name || path.path() == name
                })
                .and_then(|registration| world.components().get_id(registration.type_id()))
                .ok_or_else(|| format!("Component '{name}' is not registered"))
        })
        .collect()
}

fn json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn collect_condition_tracking_interests(
    condition: &DebugCondition,
    components: &mut Vec<String>,
    resources: &mut Vec<String>,
) {
    match condition {
        DebugCondition::QueryCount { query, .. } => {
            components.extend(query.with_components.clone());
            components.extend(query.without_components.clone());
            components.extend(query.changed.clone());
            components.extend(query.parent_has.clone());
            components.extend(query.child_has.clone());
            components.extend(query.predicates.keys().filter_map(|path| {
                path.split_once('.')
                    .map(|(component, _)| component.to_string())
            }));
        }
        DebugCondition::EntityField { component, .. } => components.push(component.clone()),
        DebugCondition::ResourceField { resource, .. } => resources.push(resource.clone()),
        DebugCondition::ChangeOccurred {
            component,
            resource,
            ..
        } => {
            if let Some(component) = component {
                components.push(component.clone());
            }
            if let Some(resource) = resource {
                resources.push(resource.clone());
            }
        }
        _ => {}
    }
}

fn reconcile_dynamic_tracking_interests(world: &mut World) {
    let (components, resources) = {
        let debugger = world.resource::<McpDebugger>();
        let mut components = Vec::new();
        let mut resources = Vec::new();

        for watchpoint in debugger
            .watchpoints
            .values()
            .filter(|watchpoint| watchpoint.enabled)
        {
            collect_condition_tracking_interests(
                &watchpoint.spec.condition,
                &mut components,
                &mut resources,
            );
        }

        for session in debugger
            .playtests
            .values()
            .filter(|session| session.status == PlaytestStatus::Running)
        {
            for step in session.plan.steps.iter().skip(session.step_index) {
                match step {
                    DebugPlaytestStep::Wait { condition, .. }
                    | DebugPlaytestStep::Assert { condition, .. } => {
                        collect_condition_tracking_interests(
                            condition,
                            &mut components,
                            &mut resources,
                        );
                    }
                    _ => {}
                }
            }
        }

        (components, resources)
    };

    world
        .resource_mut::<WorldChangeTracker>()
        .set_dynamic_interests(components, resources);
}

fn register_condition_tracking_interests(world: &mut World, condition: &DebugCondition) {
    let mut components = Vec::new();
    let mut resources = Vec::new();
    collect_condition_tracking_interests(condition, &mut components, &mut resources);
    world
        .resource_mut::<WorldChangeTracker>()
        .add_dynamic_interests(components, resources);
}

fn compare_json(actual: &Value, op: &str, expected: &Value) -> Result<bool, String> {
    match op {
        "eq" | "==" => Ok(actual == expected),
        "ne" | "!=" => Ok(actual != expected),
        "lt" | "<" => compare_numbers(actual, expected, |a, b| a < b),
        "lte" | "<=" => compare_numbers(actual, expected, |a, b| a <= b),
        "gt" | ">" => compare_numbers(actual, expected, |a, b| a > b),
        "gte" | ">=" => compare_numbers(actual, expected, |a, b| a >= b),
        "contains" => match (actual, expected) {
            (Value::String(actual), Value::String(expected)) => Ok(actual.contains(expected)),
            (Value::Array(values), expected) => Ok(values.contains(expected)),
            _ => Ok(false),
        },
        other => Err(format!("Unsupported condition operator '{other}'")),
    }
}

fn compare_numbers<F>(actual: &Value, expected: &Value, compare: F) -> Result<bool, String>
where
    F: FnOnce(f64, f64) -> bool,
{
    let actual = actual
        .as_f64()
        .ok_or_else(|| format!("Condition value {actual} is not numeric"))?;
    let expected = expected
        .as_f64()
        .ok_or_else(|| format!("Condition value {expected} is not numeric"))?;
    Ok(compare(actual, expected))
}

fn unwrap_reflect_value(value: Value) -> Value {
    match value {
        Value::Object(map) if map.len() == 1 => map
            .into_iter()
            .next()
            .map(|(_, value)| value)
            .unwrap_or(Value::Null),
        value => value,
    }
}

fn component_name_matches(actual: &str, requested: &str) -> bool {
    actual == requested
        || actual.rsplit("::").next() == Some(requested)
        || requested.rsplit("::").next() == actual.rsplit("::").next()
}

fn collect_evidence(
    world: &mut World,
    debugger: &mut McpDebugger,
    frame: u64,
    options: &EvidenceOptions,
    label: &str,
) -> Value {
    let changes = world
        .get_resource::<WorldChangeTracker>()
        .map(|tracker| tracker.changes_since(frame.saturating_sub(options.changes_frames)))
        .unwrap_or(Value::Null);

    let logs = world
        .get_resource::<LogCapture>()
        .map(|capture| {
            capture
                .get_entries(None, options.logs_limit as usize)
                .into_iter()
                .map(|entry| {
                    json!({
                        "level": entry.level,
                        "message": entry.message,
                        "target": entry.target,
                        "timestamp": entry.timestamp,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let events = world
        .get_resource::<EventCapture>()
        .map(|capture| {
            capture
                .get_events(None, options.events_limit as usize)
                .into_iter()
                .map(|event| {
                    json!({
                        "event_type": event.event_type,
                        "data": event.data,
                        "timestamp": event.timestamp,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let states = if options.include_states {
        world
            .get_resource::<McpStateRegistry>()
            .map(|states| Value::Array(states.list(world)))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    let timings = if options.include_system_timings {
        world
            .get_resource::<McpSystemTimings>()
            .map(|timings| {
                let mut rows: Vec<Value> = timings
                    .iter()
                    .map(|(name, timing)| {
                        json!({
                            "system": name,
                            "timing": timing.as_json(),
                        })
                    })
                    .collect();
                rows.sort_by(|left, right| {
                    right["timing"]["recent_average_ns"]
                        .as_u64()
                        .cmp(&left["timing"]["recent_average_ns"].as_u64())
                });
                Value::Array(rows)
            })
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    let runtime = world
        .get_resource::<McpRegistry>()
        .map(|registry| {
            json!({
                "frame": registry.frame,
                "paused": registry.paused,
                "time_scale": registry.time_scale,
            })
        })
        .unwrap_or(Value::Null);

    let screenshot_capture_id = if options.screenshot {
        let capture_id = debugger.allocate_id("capture");
        let capture = start_debug_capture(world, &capture_id, label);
        debugger.captures.insert(capture_id.clone(), capture);
        Some(capture_id)
    } else {
        None
    };

    json!({
        "frame": frame,
        "changes": changes,
        "logs": logs,
        "events": events,
        "states": states,
        "system_timings": timings,
        "runtime": runtime,
        "screenshot_capture_id": screenshot_capture_id,
    })
}

fn start_debug_capture(world: &mut World, capture_id: &str, label: &str) -> Value {
    let capture_dir = PathBuf::from(".bevy-mcp").join("evidence");
    if let Err(error) = fs::create_dir_all(&capture_dir) {
        return json!({ "status": "failed", "error": error.to_string() });
    }

    let filename = format!(
        "{}-{}.png",
        sanitize_filename(label),
        sanitize_filename(capture_id)
    );
    let path = capture_dir.join(filename);
    let response_path = path.clone();
    let id = capture_id.to_owned();

    world.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut debugger: ResMut<McpDebugger>| {
            let result = save_capture(&captured.image, &response_path)
                .map(|(width, height)| {
                    let absolute =
                        fs::canonicalize(&response_path).unwrap_or_else(|_| response_path.clone());
                    json!({
                        "status": "complete",
                        "path": response_path.to_string_lossy(),
                        "absolute_path": absolute.to_string_lossy(),
                        "width": width,
                        "height": height,
                    })
                })
                .unwrap_or_else(|error| json!({ "status": "failed", "error": error }));
            debugger.captures.insert(id.clone(), result);
        },
    );

    json!({
        "status": "pending",
        "path": path.to_string_lossy(),
    })
}

fn save_capture(image: &Image, path: &Path) -> Result<(u32, u32), String> {
    let width = image.width();
    let height = image.height();
    let dynamic = image
        .clone()
        .try_into_dynamic()
        .map_err(|error| format!("Could not convert captured image: {error:?}"))?;
    dynamic
        .to_rgb8()
        .save(path)
        .map_err(|error| format!("Could not save capture to {}: {error}", path.display()))?;
    Ok((width, height))
}

fn sanitize_filename(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .take(80)
        .collect();
    if value.is_empty() {
        "capture".to_string()
    } else {
        value
    }
}

fn watchpoint_json(watchpoint: &WatchpointRuntime, debugger: &McpDebugger) -> Value {
    json!({
        "id": watchpoint.id,
        "name": watchpoint.spec.name,
        "enabled": watchpoint.enabled,
        "condition": serde_json::to_value(&watchpoint.spec.condition).unwrap_or(Value::Null),
        "pause_on_trigger": watchpoint.spec.pause_on_trigger,
        "once": watchpoint.spec.once,
        "trigger_count": watchpoint.trigger_count,
        "last_trigger_frame": watchpoint.last_trigger_frame,
        "last_evaluation": watchpoint.last_evaluation,
        "last_error": watchpoint.last_error,
        "evidence": watchpoint.evidence.as_ref().map(|value| resolve_evidence(value, debugger)),
    })
}

fn playtest_json(session: &PlaytestRuntime, debugger: &McpDebugger) -> Value {
    let current_step = session
        .plan
        .steps
        .get(session.step_index)
        .and_then(|step| serde_json::to_value(step).ok());
    let captures: Vec<Value> = session
        .captures
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "capture": debugger.captures.get(id).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    json!({
        "id": session.id,
        "name": session.plan.name,
        "status": session.status.as_str(),
        "started_frame": session.started_frame,
        "finished_frame": session.finished_frame,
        "step_index": session.step_index,
        "steps_total": session.plan.steps.len(),
        "current_step": current_step,
        "step_results": session.step_results,
        "failure": session.failure,
        "evidence": session.evidence.as_ref().map(|value| resolve_evidence(value, debugger)),
        "captures": captures,
    })
}

fn resolve_evidence(value: &Value, debugger: &McpDebugger) -> Value {
    let mut resolved = value.clone();
    if let Some(capture_id) = value["screenshot_capture_id"].as_str() {
        resolved["screenshot"] = debugger
            .captures
            .get(capture_id)
            .cloned()
            .unwrap_or_else(|| json!({ "status": "unknown" }));
    }
    resolved
}

fn current_frame(world: &World) -> u64 {
    world
        .get_resource::<McpRegistry>()
        .map(|registry| registry.frame)
        .unwrap_or_default()
}

fn apply_key(world: &mut World, key: &str, pressed: bool) -> Result<(), String> {
    let keycode = parse_keycode(key).ok_or_else(|| format!("Unknown key '{key}'"))?;
    let mut input = world
        .get_resource_mut::<ButtonInput<KeyCode>>()
        .ok_or_else(|| "ButtonInput<KeyCode> is not available; add InputPlugin".to_string())?;
    if pressed {
        input.press(keycode);
    } else {
        input.release(keycode);
    }
    Ok(())
}

fn parse_keycode(key: &str) -> Option<KeyCode> {
    match key.to_lowercase().as_str() {
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
        "space" => Some(KeyCode::Space),
        "enter" => Some(KeyCode::Enter),
        "escape" | "esc" => Some(KeyCode::Escape),
        "tab" => Some(KeyCode::Tab),
        "arrowup" | "up" => Some(KeyCode::ArrowUp),
        "arrowdown" | "down" => Some(KeyCode::ArrowDown),
        "arrowleft" | "left" => Some(KeyCode::ArrowLeft),
        "arrowright" | "right" => Some(KeyCode::ArrowRight),
        "shift" | "shiftleft" => Some(KeyCode::ShiftLeft),
        "control" | "ctrl" | "controlleft" => Some(KeyCode::ControlLeft),
        "alt" | "altleft" => Some(KeyCode::AltLeft),
        _ => None,
    }
}

fn push_result(world: &World, request_id: u64, result: McpResult) {
    world
        .resource::<McpResultQueue>()
        .push(McpResponse { request_id, result });
}

fn push_error(world: &World, request_id: u64, code: impl Into<String>, message: impl Into<String>) {
    push_result(world, request_id, McpResult::error(code, message));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_conditions_share_query_semantics() {
        assert!(compare_json(&json!(5), ">", &json!(4)).unwrap());
        assert!(compare_json(&json!(5), "lte", &json!(5)).unwrap());
        assert!(!compare_json(&json!(5), "<", &json!(4)).unwrap());
    }

    #[test]
    fn debugger_capture_names_are_filesystem_safe() {
        assert_eq!(sanitize_filename("enemy wave #1"), "enemy-wave--1");
    }
}
