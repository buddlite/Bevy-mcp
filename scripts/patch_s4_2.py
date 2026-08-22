from pathlib import Path
import re

ROOT = Path('.')

def read(path):
    return (ROOT / path).read_text()

def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)

def replace_once(text, old, new, label):
    if old not in text:
        raise RuntimeError(f'missing replacement target: {label}')
    if text.count(old) != 1:
        raise RuntimeError(f'non-unique replacement target ({text.count(old)}): {label}')
    return text.replace(old, new, 1)

def regex_once(text, pattern, repl, label):
    out, n = re.subn(pattern, repl, text, count=1, flags=re.S)
    if n != 1:
        raise RuntimeError(f'regex replacement count={n}: {label}')
    return out

# Host debugger imports + requests + recording/replay tick + tracking interests.
path = 'crates/bevy-mcp-host/src/debugger.rs'
t = read(path)
if 'use crate::checkpoint::' not in t:
    t = replace_once(t, 'use crate::change_tracking::WorldChangeTracker;\n', 'use crate::change_tracking::WorldChangeTracker;\nuse crate::checkpoint::{McpCheckpointRegistry, McpCheckpointStore, McpRecorder, RecordedAction, ReplayStatus, StoredCheckpoint};\n', 'debugger checkpoint import')

anchor = '''        DebugRequest::PlaytestCancel { id } => {
            let frame = current_frame(world);
            let mut debugger = world.resource_mut::<McpDebugger>();
            match debugger.playtests.get_mut(&id) {
                Some(session) if session.status == PlaytestStatus::Running => {
                    session.status = PlaytestStatus::Cancelled;
                    session.finished_frame = Some(frame);
                    push_result(world, request_id, McpResult::success(json!({ "cancelled": id })));
                }
                Some(_) => push_error(world, request_id, "PLAYTEST_NOT_RUNNING", format!("Playtest '{id}' is not running")),
                None => push_error(world, request_id, "PLAYTEST_NOT_FOUND", format!("Playtest '{id}' not found")),
            }
        }
'''
extra_arms = r'''        DebugRequest::CheckpointCreate { name } => {
            let frame = current_frame(world);
            let captured = world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| registry.capture(world));
            match captured {
                Ok(values) => {
                    let mut store = world.resource_mut::<McpCheckpointStore>();
                    let id = store.next_id();
                    store.insert(StoredCheckpoint { id: id.clone(), name: name.clone(), frame, values });
                    drop(store);
                    let coverage = world.resource::<McpCheckpointRegistry>().coverage();
                    push_result(world, request_id, McpResult::success(json!({ "id": id, "name": name, "frame": frame, "coverage": coverage })));
                }
                Err(error) => push_error(world, request_id, "CHECKPOINT_CAPTURE_FAILED", error),
            }
        }
        DebugRequest::CheckpointList => {
            let checkpoints = world.resource::<McpCheckpointStore>().list();
            let coverage = world.resource::<McpCheckpointRegistry>().coverage();
            push_result(world, request_id, McpResult::success(json!({ "checkpoints": checkpoints, "coverage": coverage })));
        }
        DebugRequest::CheckpointRestore { id } => {
            let checkpoint = world.resource::<McpCheckpointStore>().get(&id).cloned();
            match checkpoint {
                Some(checkpoint) => {
                    let restored = world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| registry.restore(world, &checkpoint.values));
                    match restored {
                        Ok(()) => push_result(world, request_id, McpResult::success(json!({ "restored": id, "source_frame": checkpoint.frame, "frame": current_frame(world) }))),
                        Err(error) => push_error(world, request_id, "CHECKPOINT_RESTORE_FAILED", error),
                    }
                }
                None => push_error(world, request_id, "CHECKPOINT_NOT_FOUND", format!("Checkpoint '{id}' not found")),
            }
        }
        DebugRequest::RecordingStart { name } => {
            let frame = current_frame(world);
            let result = world.resource_mut::<McpRecorder>().start(name, frame);
            match result {
                Ok(id) => push_result(world, request_id, McpResult::success(json!({ "id": id, "start_frame": frame }))),
                Err(error) => push_error(world, request_id, "RECORDING_START_FAILED", error),
            }
        }
        DebugRequest::RecordingStop => {
            match world.resource_mut::<McpRecorder>().stop() {
                Ok(recording) => push_result(world, request_id, McpResult::success(json!({ "id": recording.id, "name": recording.name, "events": recording.events.len() }))),
                Err(error) => push_error(world, request_id, "RECORDING_STOP_FAILED", error),
            }
        }
        DebugRequest::RecordingList => {
            let rows = world.resource::<McpRecorder>().list_recordings();
            push_result(world, request_id, McpResult::success(json!({ "recordings": rows })));
        }
        DebugRequest::ReplayStart { recording_id, checkpoint_id } => {
            if let Some(checkpoint_id) = checkpoint_id.as_ref() {
                let checkpoint = world.resource::<McpCheckpointStore>().get(checkpoint_id).cloned();
                let Some(checkpoint) = checkpoint else {
                    push_error(world, request_id, "CHECKPOINT_NOT_FOUND", format!("Checkpoint '{checkpoint_id}' not found"));
                    return;
                };
                if let Err(error) = world.resource_scope(|world, registry: Mut<McpCheckpointRegistry>| registry.restore(world, &checkpoint.values)) {
                    push_error(world, request_id, "CHECKPOINT_RESTORE_FAILED", error);
                    return;
                }
            }
            let frame = current_frame(world);
            match world.resource_mut::<McpRecorder>().start_replay(recording_id, checkpoint_id, frame) {
                Ok(id) => push_result(world, request_id, McpResult::success(json!({ "id": id, "status": "running", "start_frame": frame }))),
                Err(error) => push_error(world, request_id, "REPLAY_START_FAILED", error),
            }
        }
        DebugRequest::ReplayStatus { id } => {
            match world.resource::<McpRecorder>().replay_json(&id) {
                Some(value) => push_result(world, request_id, McpResult::success(value)),
                None => push_error(world, request_id, "REPLAY_NOT_FOUND", format!("Replay '{id}' not found")),
            }
        }
        DebugRequest::ReplayCancel { id } => {
            let mut recorder = world.resource_mut::<McpRecorder>();
            match recorder.replays.get_mut(&id) {
                Some(replay) if replay.status == ReplayStatus::Running => { replay.status = ReplayStatus::Cancelled; push_result(world, request_id, McpResult::success(json!({ "cancelled": id }))); }
                Some(_) => push_error(world, request_id, "REPLAY_NOT_RUNNING", format!("Replay '{id}' is not running")),
                None => push_error(world, request_id, "REPLAY_NOT_FOUND", format!("Replay '{id}' not found")),
            }
        }
'''
t = replace_once(t, anchor, anchor + extra_arms, 'debug checkpoint request arms')

t = replace_once(t, '''        DebugRequest::PlaytestStart { .. } | DebugRequest::PlaytestCancel { .. } => {
            permissions.can_control_runtime() && permissions.can_inject_input()
        }
''', '''        DebugRequest::PlaytestStart { .. } | DebugRequest::PlaytestCancel { .. }
        | DebugRequest::ReplayStart { .. } | DebugRequest::ReplayCancel { .. } => {
            permissions.can_control_runtime() && permissions.can_inject_input()
        }
        DebugRequest::CheckpointRestore { .. } => permissions.can_mutate(),
''', 'debug permissions S4')

semantic_pattern = r'''            DebugPlaytestStep::SemanticAction \{ action, args \} => \{.*?            \}\n            DebugPlaytestStep::StateTransition'''
semantic_repl = r'''            DebugPlaytestStep::SemanticAction { action, args } => {
                let requested_args = args.clone();
                let result = world.resource_scope(|world, actions: Mut<McpActionRegistry>| {
                    actions.invoke(&action, world, args)
                });
                match result {
                    Ok(value) => {
                        world.resource_mut::<McpRecorder>().record(frame, RecordedAction::SemanticAction { action: action.clone(), args: requested_args });
                        complete_step(session, frame, json!({ "type": "semantic_action", "action": action, "result": value }));
                    }
                    Err(error) => {
                        fail_playtest(world, debugger, session, frame, "ACTION_FAILED", error);
                        return;
                    }
                }
            }
            DebugPlaytestStep::StateTransition'''
t = regex_once(t, semantic_pattern, semantic_repl, 'rewrite semantic action recording')
state_pattern = r'''            DebugPlaytestStep::StateTransition \{ state, value \} => \{.*?            \}\n            DebugPlaytestStep::Key'''
state_repl = r'''            DebugPlaytestStep::StateTransition { state, value } => {
                let requested_value = value.clone();
                let result = world.resource_scope(|world, states: Mut<McpStateRegistry>| {
                    states.set(&state, world, value)
                });
                match result {
                    Ok(result_value) => {
                        world.resource_mut::<McpRecorder>().record(frame, RecordedAction::StateTransition { state: state.clone(), value: requested_value });
                        complete_step(session, frame, json!({ "type": "state_transition", "state": state, "result": result_value }));
                    }
                    Err(error) => {
                        fail_playtest(world, debugger, session, frame, "STATE_TRANSITION_FAILED", error);
                        return;
                    }
                }
            }
            DebugPlaytestStep::Key'''
t = regex_once(t, state_pattern, state_repl, 'rewrite state transition recording')
t = replace_once(t, '''                Ok(()) => complete_step(session, frame, json!({
                    "type": "key",
                    "key": key,
                    "pressed": pressed,
                })),
''', '''                Ok(()) => {
                    world.resource_mut::<McpRecorder>().record(frame, RecordedAction::Key { key: key.clone(), pressed });
                    complete_step(session, frame, json!({ "type": "key", "key": key, "pressed": pressed }));
                }
''', 'record key')

t = replace_once(t, '''            push_result(
                world,
                request_id,
                McpResult::success(json!({ "id": id, "name": spec.name, "enabled": true })),
            );
''', '''            drop(debugger);
            register_condition_tracking_interests(world, &spec.condition);
            push_result(
                world,
                request_id,
                McpResult::success(json!({ "id": id, "name": spec.name, "enabled": true })),
            );
''', 'watchpoint tracking interests')
t = replace_once(t, '''    push_result(
        world,
        request_id,
        McpResult::success(json!({
            "id": id,
''', '''    for step in &plan.steps {
        match step {
            DebugPlaytestStep::Wait { condition, .. } | DebugPlaytestStep::Assert { condition, .. } => register_condition_tracking_interests(world, condition),
            _ => {}
        }
    }

    push_result(
        world,
        request_id,
        McpResult::success(json!({
            "id": id,
''', 'playtest tracking interests')

needle = '    world.insert_resource(debugger);\n}\n\nfn advance_playtest('
replay_tick = r'''    tick_replays(world, frame);
    world.insert_resource(debugger);
}

fn tick_replays(world: &mut World, frame: u64) {
    let mut recorder = world.remove_resource::<McpRecorder>().unwrap_or_default();
    let ids: Vec<String> = recorder.replays.iter().filter(|(_, r)| r.status == ReplayStatus::Running).map(|(id, _)| id.clone()).collect();
    for id in ids {
        let Some(mut replay) = recorder.replays.remove(&id) else { continue; };
        let Some(recording) = recorder.recordings.get(&replay.recording_id).cloned() else {
            replay.status = ReplayStatus::Failed;
            replay.failure = Some(format!("Recording '{}' disappeared", replay.recording_id));
            recorder.replays.insert(id, replay);
            continue;
        };
        while let Some(event) = recording.events.get(replay.next_event) {
            if frame.saturating_sub(replay.start_frame) < event.offset_frames { break; }
            let result = match &event.action {
                RecordedAction::SemanticAction { action, args } => world.resource_scope(|world, actions: Mut<McpActionRegistry>| actions.invoke(action, world, args.clone())).map(|_| ()),
                RecordedAction::StateTransition { state, value } => world.resource_scope(|world, states: Mut<McpStateRegistry>| states.set(state, world, value.clone())).map(|_| ()),
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

fn advance_playtest('''
if needle not in t:
    raise RuntimeError('missing replay tick insertion')
t = t.replace(needle, replay_tick, 1)

marker = 'fn compare_json(actual: &Value, op: &str, expected: &Value) -> Result<bool, String> {'
idx = t.index(marker)
interest_helper = r'''fn register_condition_tracking_interests(world: &mut World, condition: &DebugCondition) {
    let mut components = Vec::new();
    let mut resources = Vec::new();
    match condition {
        DebugCondition::QueryCount { query, .. } => {
            components.extend(query.with_components.clone());
            components.extend(query.without_components.clone());
            components.extend(query.changed.clone());
            components.extend(query.parent_has.clone());
            components.extend(query.child_has.clone());
            components.extend(query.predicates.keys().filter_map(|path| path.split_once('.').map(|(component, _)| component.to_string())));
        }
        DebugCondition::EntityField { component, .. } => components.push(component.clone()),
        DebugCondition::ResourceField { resource, .. } => resources.push(resource.clone()),
        DebugCondition::ChangeOccurred { component, resource, .. } => {
            if let Some(component) = component { components.push(component.clone()); }
            if let Some(resource) = resource { resources.push(resource.clone()); }
        }
        _ => {}
    }
    world.resource_mut::<WorldChangeTracker>().add_dynamic_interests(components, resources);
}

'''
t = t[:idx] + interest_helper + t[idx:]
write(path, t)
