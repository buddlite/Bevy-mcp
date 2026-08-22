# Agent Debugger and Playtests

The agent debugger turns `bevy-mcp` from a collection of inspection tools into a frame-driven debugging and verification harness.

It is designed around one loop:

> reproduce -> wait/watch -> stop at the fault -> collect evidence -> inspect -> fix -> replay -> verify

The debugger is included by `BevyMcpPlugin` and exposed by `AgentBevyMcpServer`.

## Tools

| Tool | Purpose |
| --- | --- |
| `watchpoint_add` | Evaluate a condition every completed frame and optionally pause on the rising edge |
| `watchpoint_list` | Read trigger state, the last evaluation, and collected evidence |
| `watchpoint_remove` | Remove one watchpoint |
| `watchpoint_clear` | Remove all watchpoints |
| `playtest_start` | Start a non-blocking frame-driven playtest |
| `playtest_status` | Read live progress, assertions, failure data, captures, and evidence |
| `playtest_list` | List running and completed playtests |
| `playtest_cancel` | Cancel a running playtest |

Only one playtest runs at a time. Watchpoints may run alongside it.

## Shared condition model

The same condition format is used by watchpoints, playtest `wait` steps, and playtest `assert` steps.

Supported condition kinds:

- `entity_exists`
- `query_count`
- `entity_field`
- `resource_field`
- `state_equals`
- `log_contains`
- `change_occurred`
- `frame_at_least`

Field and query comparisons use the same operators as advanced entity queries:

- `eq` / `==`
- `ne` / `!=`
- `lt` / `<`
- `lte` / `<=`
- `gt` / `>`
- `gte` / `>=`
- `contains`

## Example: pause when any Health falls below 20

Call `watchpoint_add` with:

```json
{
  "name": "low-health-breakpoint",
  "pause_on_trigger": true,
  "once": true,
  "condition": {
    "kind": "query_count",
    "query": {
      "with_components": ["Health"],
      "predicates": {
        "Health.current": {
          "op": "lt",
          "value": 20
        }
      }
    },
    "op": "gt",
    "value": 0
  }
}
```

The watchpoint triggers only when the condition transitions from false to true. A persistent condition therefore does not generate an evidence bundle every frame.

If `pause_on_trigger` is enabled, the MCP runtime is paused on the trigger frame. The next `watchpoint_list` response includes the trigger frame, the evaluated value, and the evidence bundle.

## Example: break on a component change

```json
{
  "name": "contract-state-mutated",
  "pause_on_trigger": true,
  "condition": {
    "kind": "change_occurred",
    "component": "ContractState"
  }
}
```

`change_occurred` can be filtered by an entity, a component, or a resource.

## Example: break when an error is logged

```json
{
  "name": "error-log-breakpoint",
  "condition": {
    "kind": "log_contains",
    "level": "ERROR",
    "text": "cargo transfer"
  }
}
```

Log watchpoints operate on `LogCapture`, so the application's tracing subscriber must include the capture layer if log evidence is required.

## Playtest steps

A playtest is a state machine advanced from `PostUpdate`; it never blocks Bevy while waiting for a condition.

Supported steps:

- `semantic_action` - invoke a game action registered through `McpAgentAppExt::register_mcp_action`
- `state_transition` - queue a registered typed Bevy state transition
- `key` - press or release a keyboard key
- `step_frames` - wait for a number of completed Bevy frames
- `wait` - wait until a debugger condition matches or a frame timeout expires
- `assert` - require a debugger condition to match immediately
- `capture` - request a primary-window screenshot without failing the playtest

## Example autonomous playtest

Assume the game registers semantic actions named `spawn_enemy_wave` and `give_player_money`.

```json
{
  "name": "enemy-wave-smoke-test",
  "pause_on_failure": true,
  "steps": [
    {
      "type": "semantic_action",
      "action": "give_player_money",
      "args": { "amount": 1000 }
    },
    {
      "type": "semantic_action",
      "action": "spawn_enemy_wave",
      "args": { "count": 5 }
    },
    {
      "type": "wait",
      "timeout_frames": 120,
      "condition": {
        "kind": "query_count",
        "query": {
          "with_components": ["Enemy"]
        },
        "op": "gte",
        "value": 5
      }
    },
    {
      "type": "assert",
      "message": "The spawned wave must contain at least five living enemies",
      "condition": {
        "kind": "query_count",
        "query": {
          "with_components": ["Enemy", "Health"],
          "predicates": {
            "Health.current": {
              "op": "gt",
              "value": 0
            }
          }
        },
        "op": "gte",
        "value": 5
      }
    },
    {
      "type": "capture",
      "name": "enemy-wave-ready"
    }
  ]
}
```

`playtest_start` returns immediately with a playtest ID. Poll `playtest_status` to read progress. The game continues running between polls.

## Failure evidence bundles

When an assertion, wait, semantic action, state transition, or input step fails, the playtest automatically records an evidence bundle.

By default it contains:

- ECS entity/component/resource changes from the previous 120 completed frames
- the 50 most recent captured log entries
- the 50 most recent captured game/ECS events
- all states registered with `McpAgentAppExt::register_mcp_state`
- all explicit `McpSystemTimings` samples
- current MCP runtime frame, pause state, and time scale
- a primary-window screenshot

Screenshot capture is asynchronous. A failure response may initially contain:

```json
{
  "screenshot": {
    "status": "pending"
  }
}
```

A later `playtest_status` or `watchpoint_list` call resolves it to `complete` with the saved path and dimensions, or to `failed` with an error.

Evidence screenshots are stored under:

```text
.bevy-mcp/evidence/
```

Evidence volume can be adjusted per watchpoint or playtest:

```json
{
  "evidence": {
    "changes_frames": 240,
    "logs_limit": 100,
    "events_limit": 100,
    "include_states": true,
    "include_system_timings": true,
    "screenshot": true
  }
}
```

## Semantic actions are preferred over brittle input

Keyboard input is available for end-to-end flows, but stable agent playtests should prefer game-defined semantic actions for setup and high-level intent.

For example:

```rust
use bevy_mcp_host::McpAgentAppExt;
use serde_json::json;

app.register_mcp_action(
    "give_player_money",
    "Add money to the active player for agent testing",
    |world, args| {
        let amount = args["amount"]
            .as_u64()
            .ok_or_else(|| "amount must be an unsigned integer".to_string())?;

        // Update your game resource/component here.
        Ok(json!({ "granted": amount }))
    },
);
```

This gives the agent a stable vocabulary such as `create_contract`, `spawn_enemy_wave`, `teleport_player`, or `complete_tutorial` instead of forcing every test to reproduce low-level UI/input sequences.

## Permissions

Non-pausing watchpoints are observation tools and are available at normal read permission.

A watchpoint with `pause_on_trigger: true` requires runtime-control permission.

Starting or cancelling an agent playtest requires full runtime/input permission because a plan may inject keys, invoke game actions, transition states, or pause on failure.

## Architecture

Debugger requests use a separate versioned operation envelope:

```text
bevy-mcp:debug:v1:
```

The host debugger ingress runs before the existing advanced and legacy ingress systems. Requests that do not use the debugger prefix are re-queued unchanged, preserving the existing tool surface.

The playtest/watchpoint tick runs in `PostUpdate` after world change tracking, which means conditions and evidence see the frame's completed gameplay mutations rather than a partially updated ECS.
