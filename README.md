<div align="center">

# bevy-mcp

**Give AI agents structured, runtime control over a live Bevy game.**

Inspect the ECS, mutate reflected state, interact through Bevy's native input and picking paths,
run assertions and agent playtests, capture runtime evidence, and debug what changed — all through the
[Model Context Protocol](https://modelcontextprotocol.io/).

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Bevy](https://img.shields.io/badge/Bevy-0.19-orange.svg)](https://bevyengine.org/)

</div>

> **Development status:** `v.01` is the active development branch and may be ahead of published crates. The examples below use matching source dependencies so the documented tool surface and the code you run stay aligned.

---

## What bevy-mcp is

bevy-mcp supports two execution modes over the same MCP tool model. **Embedded mode** keeps the MCP server and Bevy host in one process for direct runtime inspection/control. **Supervised mode** keeps a persistent `bevy-mcp` control-plane process alive while game binaries rebuild and restart underneath it, so a coding agent can survive compile errors, crashes, and process replacement without losing its MCP session.

The goal is not just inspection. The current tool surface supports a practical autonomous development loop:

```text
inspect -> identify target -> mutate -> run/step -> interact -> assert
   ^                                                     |
   |                                                     v
retry <- replay/checkpoint <- inspect evidence <- diagnose failure
```

That makes bevy-mcp useful for coding agents that need to understand and manipulate a running game instead of guessing from source code alone.

---

## Current capabilities

### ECS inspection and reflection

Inspect the world, query entities, read reflected components and resources, inspect schemas, traverse hierarchy, and run richer agent-oriented queries with field predicates, hierarchy relationships, name matching, included reflected values, and recent-change filters.

**Representative tools:** `world_summary` · `world_context_scan` · `entity_query` · `entity_query_advanced` · `entity_get` · `component_get` · `component_schema` · `resource_list` · `resource_get` · `resource_schema` · `hierarchy`

### Safe ECS mutation

Spawn and despawn entities, insert/update/remove reflected components, update resources, reparent entities, transition registered Bevy states, invoke game-defined semantic actions, and create procedural meshes/templates. ECS mutations are deferred to safe schedule boundaries.

For multi-write edits, `batch` with `atomic: true` provides a prevalidated all-or-nothing transaction for reflected `component_insert`, `component_update`, `component_remove`, and `resource_update` operations. The entire batch is validated against one exclusive world snapshot before the first write; `dry_run: true` performs the same validation without committing.

**Representative tools:** `entity_spawn` · `entity_despawn` · `component_insert` · `component_update` · `component_remove` · `resource_update` · `batch` · `entity_reparent` · `state_transition` · `semantic_action_invoke` · `mesh_spawn` · `template_save` · `template_load`

### Native agent interaction

Drive the game through Bevy's native input and picking paths. The MCP software pointer can move, identify ordered picking hits, click, drag, and scroll. UI nodes can be queried, inspected, clicked by entity, and supplied with native editable-text input.

**Representative tools:** `input_key` · `input_mouse` · `input_gamepad` · `pick_at` · `pointer_move` · `pointer_click` · `pointer_drag` · `pointer_scroll` · `ui_query` · `ui_inspect` · `ui_click` · `ui_type`

### Camera control and visual evidence

Inspect and move the active camera, aim at entities, capture the primary viewport or a camera render target, crop captures, and target registered UI-only render targets.

`camera_frame_entity` performs editor-style framing rather than merely aiming at an entity. It aggregates `Aabb` bounds across the target and its descendants, transforms them into world space, fits perspective cameras using FOV/aspect/clip planes, fits orthographic cameras through projection scale, supports a configurable margin, and preserves world-space behavior for parented camera rigs.

**Representative tools:** `camera_list` · `camera_inspect` · `camera_set_transform` · `camera_look_at` · `camera_frame_entity` · `capture_viewport`

### Reflected assertions

Assert live game state directly through reflection:

- `entity_exists`
- `component_exists`
- `entity_count`
- `component_equals`
- `resource_equals`

`component_equals` and `resource_equals` support dot-separated reflected field paths, including array indices, so an agent can assert nested state instead of only checking top-level values.

**Tool:** `assert`

### Agent playtests and watchpoints

Run non-blocking, frame-driven playtests with semantic actions, state transitions, key input, explicit frame stepping, conditional waits, assertions, and captures. Failures can pause the runtime and return an evidence bundle containing recent changes, logs, events, registered states, system timings, and screenshot status.

Frame-evaluated watchpoints can monitor conditions such as entity existence, query counts, reflected fields, state values, logs, change events, and frame thresholds.

**Representative tools:** `playtest_start` · `playtest_status` · `playtest_list` · `playtest_cancel` · `watchpoint_add` · `watchpoint_list` · `watchpoint_remove` · `watchpoint_clear`

### Runtime and causal debugging

Inspect changes since a frame, entity/component/resource deltas, Bevy schedules and systems, tracked access, writer candidates, change-tracking configuration, system timings, logs, events, and diagnostics.

For causal debugging, exact writer information is available for MCP-registered system access; Bevy conflict evidence is used as a fallback where exact provenance is not instrumented.

**Representative tools:** `changes_since` · `entity_changes` · `component_changes` · `resource_changes` · `schedule_list` · `schedule_inspect` · `system_list` · `system_inspect` · `system_access` · `component_writers` · `resource_writers` · `tracking_config` · `tracking_status` · `system_timings` · `logs` · `errors` · `observe_events` · `diagnostics`

### Deterministic debugging

Create and restore checkpoints for explicitly registered checkpoint state, record semantic actions/state transitions/debugger key injections with frame offsets, and replay those recordings — optionally after restoring a checkpoint.

**Representative tools:** `checkpoint_create` · `checkpoint_list` · `checkpoint_restore` · `recording_start` · `recording_stop` · `recording_list` · `replay_start` · `replay_status` · `replay_cancel`

### Asset-path debugging

Inspect known asset paths, including active asset IDs, runtime type information, and load/dependency state. Active loaded asset paths can be queued for reload.

**Representative tools:** `asset_get` · `asset_status` · `asset_reload`

### Runtime capability contract

Call `capabilities` to get the live contract. In embedded mode it reports the Bevy-host surface. In supervised mode the persistent supervisor merges that host contract with Cargo/build permissions, managed-process lifecycle state, and `rebuild_restart` availability. Agents should prefer this over assuming that every registered MCP tool is usable in every game configuration.

---

## Important current limitations

The front page intentionally distinguishes registered tools from capabilities that are actually available today:

- **Build tools are mode-dependent.** Embedded `build_check`, `build`, and `test` remain unavailable; supervised mode owns trusted Cargo execution and the composite `rebuild_restart` development cycle.
- **Loaded-asset enumeration is not implemented.** `asset_list` is reserved; use known-path inspection with `asset_get` / `asset_status`.
- **Atomic batch scope is intentionally narrow.** Atomic mode currently accepts reflected `component_insert`, `component_update`, `component_remove`, and `resource_update`. Entity lifecycle, hierarchy changes, runtime/input operations, semantic actions, and other arbitrary side effects are not transaction members. `verify` mode remains unavailable.
- **Entity duplication is reserved.** Safe reflected component cloning is not implemented yet.
- **Embedded lifecycle ownership remains external.** `runtime_launch`, `runtime_stop`, and `runtime_restart` are unavailable when the game process owns its own lifecycle.
- **Generic high-level `input_action` is not implemented.** Games should register semantic actions and use `semantic_action_list` / `semantic_action_invoke`.
- **Checkpoint coverage is explicit, not magical.** Only resources/custom state adapters registered for checkpointing are restored.
- Some features depend on the runtime configuration: renderer/capture targets, picking, gamepad resources, registered states/actions, instrumentation, and permissions. Use `capabilities` to discover the live surface.

---

## Quick Start

### 1. Add matching dependencies

For a source checkout/workspace integration:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-core = { path = "path/to/Bevy-mcp/crates/bevy-mcp-core" }
bevy-mcp-host = { path = "path/to/Bevy-mcp/crates/bevy-mcp-host" }
bevy-mcp-server = { path = "path/to/Bevy-mcp/crates/bevy-mcp-server" }
rmcp = { version = "3", features = ["server", "transport-io"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

For the current `v.01` development surface, use a source checkout or matching git dependencies. Use crates.io only when a tagged release explicitly documents the same capability set.

### 2. Embed the full agent server

Use `AgentBevyMcpServer` for the complete base + advanced + debugger/playtest tool surface:

```rust
use bevy::prelude::*;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::tools::BevyMcpState;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let state = BevyMcpState::embedded(ingress.clone(), results.clone());

    std::thread::spawn(move || {
        App::new()
            .add_plugins(DefaultPlugins)
            .add_plugins(
                BevyMcpPlugin::new()
                    .with_queues(ingress, results)
                    .with_permissions(McpPermissions::full()),
            )
            .run();
    });

    let server = AgentBevyMcpServer::new(state).serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
```

`BevyMcpServer` remains available when only the base/legacy surface is desired. `AgentBevyMcpServer` is the normal choice for autonomous-agent workflows.

### 3. Point an MCP client at the game binary

For clients that use an MCP JSON configuration:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/your-game-with-mcp",
      "args": []
    }
  }
}
```

The exact configuration file differs by client, but the transport pattern is the same: the MCP client launches the game binary and communicates over stdio.

---

## Supervised mode for autonomous development

For coding agents that need to edit Rust, compile, relaunch, and continue interacting with the new game process, use the persistent supervisor rather than making the game binary itself the MCP stdio server.

The intended loop is:

```text
edit source -> rebuild_restart -> cargo check while old game stays live
                              -> stop old game only after check passes
                              -> cargo build -> launch Cargo-reported artifact
                              -> authenticated reconnect -> host probe -> ready
                              -> inspect/interact/assert/debug
```

`rebuild_restart` is asynchronous and returns a `supervisor:rebuild_restart:*` operation ID. Poll it with `operation_status`; use `process_evidence` for bounded stdout/stderr plus process state when startup or runtime failures need diagnosis. A failed preflight check leaves the old managed game untouched. A build failure after the stop phase deliberately leaves the game stopped rather than relaunching stale code.

See **[Supervised mode and autonomous rebuild/restart](docs/supervised-mode.md)** for game instrumentation, MCP client configuration, zero-config target discovery, lifecycle permissions, failure semantics, and troubleshooting.

---

## How it works

1. **Your game embeds `BevyMcpPlugin`.** The host plugin runs inside the Bevy process and owns ECS-facing inspection, mutation, input, debugging, and runtime integration.
2. **`AgentBevyMcpServer` exposes MCP over stdio.** The external AI/coding agent communicates using normal MCP JSON-RPC.
3. **Server requests cross an in-process queue boundary.** The MCP server and Bevy host share request/result queues; no engine-side socket bridge is required.
4. **Mutations are deferred.** ECS writes are applied at safe schedule boundaries instead of mutating the world mid-system.
5. **Reflection supplies structured state.** Registered reflected components/resources can be inspected, compared, and updated without writing a bespoke serializer for every type.
6. **Optional agent adapters add semantics.** Games can register semantic actions, typed states, capture targets, checkpoint state, system-access metadata, and timing instrumentation where generic ECS reflection is not enough.

---

## Architecture

```text
┌──────────────────────────────────────────────────────┐
│                 MCP Client / AI Agent                │
│        Claude Code · Cursor · Codex · others         │
└──────────────────────┬───────────────────────────────┘
                       │ stdio / MCP JSON-RPC
                       ▼
┌──────────────────────────────────────────────────────┐
│              AgentBevyMcpServer                      │
│   base tools + advanced tools + debugger/playtests   │
└──────────────────────┬───────────────────────────────┘
                       │ in-process request/result queues
                       ▼
┌──────────────────────────────────────────────────────┐
│              bevy-mcp-host / BevyMcpPlugin           │
│  reflection · deferred mutation · input · debugger   │
└──────────────────────┬───────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────┐
│                    Bevy ECS World                    │
└──────────────────────────────────────────────────────┘
```

The workspace is split into four crates:

- **`bevy-mcp-core`** — shared protocol types, commands, entity handles, queues, and debug/advanced request models; no Bevy dependency.
- **`bevy-mcp-server`** — MCP routers and stdio-facing server surface; no Bevy dependency.
- **`bevy-mcp-host`** — the Bevy plugin and runtime integration layer.
- **`bevy-mcp-supervisor`** — the persistent control plane for authenticated game reconnection, Cargo execution, process ownership, evidence capture, and `rebuild_restart`.

---

## Permissions

bevy-mcp enforces permissions at the host boundary:

```rust
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};

// Observation only
.with_permissions(McpPermissions::read_only())

// Observation + ECS/state mutation
.with_permissions(McpPermissions::write())

// Observation + mutation + runtime/input interaction
.with_permissions(McpPermissions::full())
```

The live `capabilities` response combines implementation status, runtime availability, and permission allowance so an agent can tell the difference between "this tool exists" and "this operation is currently usable".

---

## Agent integration hooks

Reflection covers a large part of Bevy automatically, but some agent workflows benefit from explicit game-level semantics. The host exposes registration APIs for:

- semantic actions (`McpActionRegistry`)
- typed game states (`McpStateRegistry`)
- dedicated capture targets (`McpCaptureTargets`)
- system access metadata (`McpSystemAccessRegistry`)
- system timing instrumentation (`McpSystemTimings`)
- deterministic checkpoint state (`McpCheckpointRegistry`)

These adapters let an agent move from generic ECS manipulation toward higher-level operations such as "start mission", "buy upgrade", "enter build mode", or "restore this test state" without hard-coding those concepts into bevy-mcp itself.

See the [agent adapter checklist](docs/agent-adapter.md) for one minimal example that registers an action, typed state, checkpoint resource, and system-access specification together.

---

## Agent setup guides

bevy-mcp uses standard MCP stdio transport, so any compatible client can launch the instrumented game binary. Repository guides are available for common clients:

| Agent | Guide |
|---|---|
| **Claude Code** | [docs/guides/claude-code.md](docs/guides/claude-code.md) |
| **Claude Desktop** | [docs/guides/claude-desktop.md](docs/guides/claude-desktop.md) |
| **Cursor** | [docs/guides/cursor.md](docs/guides/cursor.md) |
| **Codex CLI** | [docs/guides/codex-cli.md](docs/guides/codex-cli.md) |
| **Gemini CLI** | [docs/guides/gemini-cli.md](docs/guides/gemini-cli.md) |
| **Cline** | [docs/guides/cline.md](docs/guides/cline.md) |
| **Local LLMs** | [docs/guides/local-llms.md](docs/guides/local-llms.md) |

---

## FAQ

### Is bevy-mcp an external editor bridge?

No. The MCP client is external, but the Bevy-facing host runs inside the game process. The stdio server forwards requests to the host through shared in-process queues.

### Does every tool work in every game?

No. Some operations require a renderer, picking, gamepad resources, registered semantic actions/states, checkpoint adapters, or a higher permission level. Call `capabilities` to discover the live contract instead of assuming availability.

### Do I need custom serialization for reflected components?

Usually not. Registered `Reflect` data can be inspected and mutated through Bevy's type registry. Game-specific semantic behavior can be layered on with explicit agent adapters when reflection alone is too low-level.

### Can the agent interact with the running game instead of only editing ECS state?

Yes. The software pointer, native picking pipeline, UI interaction tools, keyboard/gamepad input, camera controls, viewport capture, watchpoints, assertions, and frame-driven playtests are intended to support that loop.

### Can bevy-mcp run Cargo builds and tests for the agent?

Not from the embedded MCP server today. The build/check/test tools intentionally return `BUILD_NOT_AVAILABLE`; run those commands from a trusted development shell or coding harness.

### Is checkpoint/replay a full snapshot of the entire Bevy world?

No. Deterministic checkpoints restore explicitly registered checkpoint state. This keeps the contract truthful and avoids pretending arbitrary engine/plugin state can always be cloned safely.

### Should this ship in production builds?

bevy-mcp is primarily a development, testing, and agent-automation tool. Gate the plugin behind an appropriate feature/build configuration and do not expose an unrestricted development-control surface in a shipped game.

---

## Contributing

Contributions are welcome. Bug reports, runtime integrations, new tools, tests, and documentation improvements are all useful.

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing, and code-style guidance.

---

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
