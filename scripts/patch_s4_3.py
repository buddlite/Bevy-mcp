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

# Record direct advanced semantic/state actions too.
path = 'crates/bevy-mcp-host/src/advanced.rs'
t = read(path)
if 'McpRecorder' not in t.split('\n', 40)[0:40]:
    t = replace_once(t, 'use crate::change_tracking::WorldChangeTracker;\n', 'use crate::change_tracking::WorldChangeTracker;\nuse crate::checkpoint::{McpRecorder, RecordedAction};\n', 'advanced recorder import')
old = '''        AdvancedRequest::StateTransition { state, value } => {
            let result = world.resource_scope(|world, registry: Mut<McpStateRegistry>| {
                registry.set(&state, world, value)
            });
            push_result(
                world,
                request_id,
                result
                    .map(McpResult::success)
                    .unwrap_or_else(|message| McpResult::error("STATE_TRANSITION_FAILED", message)),
            );
        }
'''
new = '''        AdvancedRequest::StateTransition { state, value } => {
            let recorded_value = value.clone();
            let result = world.resource_scope(|world, registry: Mut<McpStateRegistry>| {
                registry.set(&state, world, value)
            });
            if result.is_ok() {
                let frame = world.get_resource::<McpRegistry>().map(|r| r.frame).unwrap_or_default();
                world.resource_mut::<McpRecorder>().record(frame, RecordedAction::StateTransition { state: state.clone(), value: recorded_value });
            }
            push_result(world, request_id, result.map(McpResult::success).unwrap_or_else(|message| McpResult::error("STATE_TRANSITION_FAILED", message)));
        }
'''
t = replace_once(t, old, new, 'record advanced state')
old = '''        AdvancedRequest::SemanticActionInvoke { action, args } => {
            let result = world.resource_scope(|world, registry: Mut<McpActionRegistry>| {
                registry.invoke(&action, world, args)
            });
            push_result(
                world,
                request_id,
                result
                    .map(|value| McpResult::success(json!({ "action": action, "result": value })))
                    .unwrap_or_else(|message| McpResult::error("ACTION_FAILED", message)),
            );
        }
'''
new = '''        AdvancedRequest::SemanticActionInvoke { action, args } => {
            let recorded_args = args.clone();
            let result = world.resource_scope(|world, registry: Mut<McpActionRegistry>| {
                registry.invoke(&action, world, args)
            });
            if result.is_ok() {
                let frame = world.get_resource::<McpRegistry>().map(|r| r.frame).unwrap_or_default();
                world.resource_mut::<McpRecorder>().record(frame, RecordedAction::SemanticAction { action: action.clone(), args: recorded_args });
            }
            push_result(world, request_id, result.map(|value| McpResult::success(json!({ "action": action, "result": value }))).unwrap_or_else(|message| McpResult::error("ACTION_FAILED", message)));
        }
'''
t = replace_once(t, old, new, 'record advanced semantic')
write(path, t)

# Debug server params and tools for checkpoints/replay.
path = 'crates/bevy-mcp-server/src/debug_tools.rs'
t = read(path)
insert_after = '''pub struct IdParams {
    pub id: String,
}
'''
extra = r'''
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NameParams { pub name: String }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplayStartParams {
    pub recording_id: String,
    #[schemars(description = "Optional checkpoint restored immediately before replay.")]
    pub checkpoint_id: Option<String>,
}
'''
t = replace_once(t, insert_after, insert_after + extra, 'debug S4 params')
marker = '''}

/// Top-level MCP server exposing legacy, advanced, and debugger/playtest tools.'''
if marker not in t:
    raise RuntimeError('debug tool impl end marker missing')
methods = r'''

    #[tool(description = "Create a deterministic checkpoint from resources/custom adapters registered by the game.")]
    async fn checkpoint_create(&self, Parameters(params): Parameters<NameParams>) -> String {
        self.state.call(DebugRequest::CheckpointCreate { name: params.name }).await
    }

    #[tool(description = "List deterministic checkpoints and current checkpoint adapter coverage.")]
    async fn checkpoint_list(&self) -> String {
        self.state.call(DebugRequest::CheckpointList).await
    }

    #[tool(description = "Restore a deterministic checkpoint. Only explicitly registered checkpoint state is modified.")]
    async fn checkpoint_restore(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state.call(DebugRequest::CheckpointRestore { id: params.id }).await
    }

    #[tool(description = "Start recording semantic actions, state transitions, and debugger key injections with frame offsets.")]
    async fn recording_start(&self, Parameters(params): Parameters<NameParams>) -> String {
        self.state.call(DebugRequest::RecordingStart { name: params.name }).await
    }

    #[tool(description = "Stop and persist the active deterministic action recording.")]
    async fn recording_stop(&self) -> String {
        self.state.call(DebugRequest::RecordingStop).await
    }

    #[tool(description = "List saved deterministic action recordings.")]
    async fn recording_list(&self) -> String {
        self.state.call(DebugRequest::RecordingList).await
    }

    #[tool(description = "Restore an optional checkpoint and replay a saved action recording at its original frame offsets.")]
    async fn replay_start(&self, Parameters(params): Parameters<ReplayStartParams>) -> String {
        self.state.call(DebugRequest::ReplayStart { recording_id: params.recording_id, checkpoint_id: params.checkpoint_id }).await
    }

    #[tool(description = "Read live deterministic replay progress and failure state.")]
    async fn replay_status(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state.call(DebugRequest::ReplayStatus { id: params.id }).await
    }

    #[tool(description = "Cancel a running deterministic replay.")]
    async fn replay_cancel(&self, Parameters(params): Parameters<IdParams>) -> String {
        self.state.call(DebugRequest::ReplayCancel { id: params.id }).await
    }
'''
t = t.replace(marker, methods + marker, 1)
write(path, t)

write('crates/bevy-mcp-host/tests/intelligence.rs', r'''use bevy::prelude::*;
use bevy_mcp_host::{McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore};
use serde::{Deserialize, Serialize};

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct SimSeed(u64);

#[test]
fn checkpoint_resource_round_trip_is_deterministic() {
    let mut app = App::new();
    app.insert_resource(SimSeed(42));
    app.register_mcp_checkpoint_resource::<SimSeed>("sim_seed", "deterministic simulation seed");

    let values = app.world().resource::<McpCheckpointRegistry>().capture(app.world()).unwrap();
    app.world_mut().resource_mut::<SimSeed>().0 = 99;
    app.world_mut().resource_scope(|world, registry: Mut<McpCheckpointRegistry>| registry.restore(world, &values)).unwrap();
    assert_eq!(app.world().resource::<SimSeed>().0, 42);

    app.world_mut().init_resource::<McpCheckpointStore>();
}
''')

write('docs/debugging-intelligence.md', r'''# Debugging intelligence and deterministic replay

This layer adds four high-value capabilities for agent-driven Bevy development.

## Concurrent MCP transport

All legacy, advanced, and debugger requests share one `McpResponseDispatcher`. It is the only consumer of the result queue and routes results by request ID to one-shot channels. Independent MCP calls no longer need a global serialization mutex.

## Runtime-to-system causality

Use `system_access` to inspect a system's declared ECS reads/writes and `component_writers` / `resource_writers` to identify candidate systems capable of causing a runtime mutation. Unbounded `&World` / `&mut World` access is reported explicitly.

## Scoped change tracking

`tracking_config` accepts `mode: "full" | "scoped"`, history length, component/resource allowlists, and exclusions. Full mode preserves existing behavior. Scoped mode retains spawn/despawn tracking but only snapshots change ticks for subscribed component/resource types. Debugger watchpoints and playtest conditions automatically add relevant dynamic subscriptions.

Example:

```json
{
  "mode": "scoped",
  "history_frames": 300,
  "components": ["Health", "Cargo"],
  "resources": ["Economy"],
  "exclude_components": ["Transform"]
}
```

## Deterministic checkpoints and replay

Arbitrary Bevy worlds cannot be safely cloned while preserving every entity identity and non-reflected engine object. Checkpoints are therefore explicit: games register deterministic resources or custom adapters.

```rust
app.register_mcp_checkpoint_resource::<SimulationRng>(
    "simulation_rng",
    "RNG state used by deterministic simulation",
);
```

Then an agent can:

1. `checkpoint_create`
2. `recording_start`
3. invoke semantic actions / state transitions / debugger key steps
4. `recording_stop`
5. change code or simulation state
6. `replay_start` with the checkpoint ID
7. poll `replay_status`

Replay preserves the original frame offsets between recorded actions. Checkpoint coverage is returned by `checkpoint_list` so agents can tell exactly what state is deterministic rather than assuming the whole engine was snapshotted.
''')
