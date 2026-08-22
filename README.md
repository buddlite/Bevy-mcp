<div align="center">

# bevy-mcp

**Give AI agents real power over your Bevy game.**

61 tools for ECS inspection, mutation, runtime control, input injection, and more —
all through the [Model Context Protocol](https://modelcontextprotocol.io/).

[![Crates.io](https://img.shields.io/crates/v/bevy-mcp-host)](https://crates.io/crates/bevy-mcp-host)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Bevy](https://img.shields.io/badge/Bevy-0.19-orange.svg)](https://bevyengine.org/)

</div>

---

## Why bevy-mcp?

Other engines bolt AI integration on from the outside. Bevy-mcp is **embedded directly in your game binary** — the AI agent lives inside the same process as your ECS, with zero network overhead and full access to Bevy's type system.

- **Stop guessing, start inspecting.** Your agent queries real entity data through Bevy's reflection system — not screen scraping, not log parsing, not brittle string matching. Ask "what entities have a `Health` component?" and get structured JSON back.

- **Mutate safely, every time.** All ECS mutations go through a deferred command queue that executes at safe schedule boundaries. No mid-frame corruption, no ordering surprises. The agent writes intent, the engine applies it at the right moment.

- **Control the whole lifecycle.** Pause, resume, step frame-by-frame, adjust time scale, inject keyboard/mouse/gamepad input, click UI buttons, and capture screenshots — your agent has the same control as a human player with a debugger attached.

- **Ship with confidence.** A built-in permission system gates what the agent can do: `Read` for observation only, `Write` for ECS mutation, `Full` for input and runtime control. No agent oversteps unless you explicitly allow it.

---

## Quick Start

### 1. Add the dependency

```toml
[dependencies]
bevy-mcp-host = "0.1"
```

### 2. Add the plugin to your app

```rust
use bevy::prelude::*;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use bevy_mcp_server::tools::{BevyMcpServer, BevyMcpState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let server = BevyMcpServer::new(BevyMcpState::embedded(ingress.clone(), results.clone()));

    std::thread::spawn(move || {
        App::new()
            .add_plugins(DefaultPlugins)
            .add_plugins(
                BevyMcpPlugin::new()
                    .with_queues(ingress, results)
                    .with_permissions(McpPermissions::read_only()),
            )
            .run();
    });

    server.serve(rmcp::transport::stdio()).await?.waiting().await?;
    Ok(())
}
```

### 3. Point your AI agent at it

**Claude Code / Claude Desktop** — add to your MCP settings:

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

That's it. Your agent now has a live, structured connection to your Bevy ECS.

---

## How It Works

1. **Your game binary embeds the MCP server.** The `bevy-mcp-host` plugin runs inside your Bevy app; the `bevy-mcp-server` handles the MCP protocol over stdio. Both live in the same process.

2. **The AI agent connects via stdio.** Any MCP-compatible client (Claude Code, Cursor, Claude Desktop, Codex CLI, etc.) launches your game binary as a subprocess and communicates over stdin/stdout.

3. **Requests flow through shared queues.** The server writes commands to an ingress queue; the host plugin reads them during ECS systems and writes results back. No sockets, no serialization over the wire — just in-process `Arc<Mutex<>>`.

4. **Mutations execute at safe boundaries.** ECS write operations are deferred and applied at the next command application point in Bevy's schedule, preventing mid-frame corruption.

5. **Reflection does the heavy lifting.** Every component read/write goes through Bevy's type registry. No manual serialization per component — if it's `Reflect`, it's accessible.

---

## Tools Overview

bevy-mcp ships **61 tools** organized into categories. Here's what your agent can do:

### ECS Inspection & Mutation
Query any entity by component filters, read full component values via reflection, spawn and despawn entities, insert/update/remove components, and manage entity hierarchies — reparent, duplicate, and traverse the full parent-child tree.

**Key tools:** `world_summary` · `entity_query` · `entity_get` · `component_get` · `component_schema` · `entity_spawn` · `entity_despawn` · `component_insert` · `component_update` · `component_remove` · `hierarchy` · `entity_reparent`

### Resources
List, read, and mutate any registered Bevy resource by type name. Schema introspection included.

**Key tools:** `resource_list` · `resource_get` · `resource_schema` · `resource_update`

### Runtime Control
Pause and resume the simulation, step frame-by-frame for deterministic debugging, adjust time scale, and manage the full app lifecycle (launch, stop, restart).

**Key tools:** `runtime_pause` · `runtime_resume` · `runtime_step` · `runtime_time_scale`

### Input Injection
Simulate keyboard, mouse, and gamepad input. Your agent can play the game exactly like a human — or run thousands of automated playthroughs.

**Key tools:** `input_key` · `input_mouse`

### UI Interaction
Query the UI node tree, inspect element details, click buttons, and type into text fields. Test menus and HUDs without a human in the loop.

**Key tools:** `ui_query` · `ui_inspect`

### Camera & Capture
List cameras, inspect properties, set transforms, look at targets, frame specific entities, and capture screenshots from any camera — including off-screen render targets.

**Key tools:** `camera_list` · `camera_inspect` · `capture_game` · `capture_camera`

### Assets
Browse loaded assets, check their status, inspect metadata, and trigger hot-reloads.

**Key tools:** `asset_list` · `asset_get` · `asset_status` · `asset_reload`

### Events & Diagnostics
Observe captured ECS events, read log output filtered by level, and get real-time diagnostics (FPS, frame time, entity count).

**Key tools:** `observe_events` · `logs` · `diagnostics`

### Build & Playtest
Run `cargo check`, `build`, and `test` with structured output directly from the agent. Define assertions against game state for automated playtesting.

**Key tools:** `cargo_check` · `cargo_build` · `cargo_test` · `assert`

### Batch Operations
Execute multiple read operations in a single call, with `dry_run` to preview and `verify` to confirm. Atomic mode ensures all-or-nothing semantics for write batches.

**Key tools:** `batch`

---

## Comparison

How bevy-mcp stacks up against AI integration in other engines:

| Capability | bevy-mcp | Unity MCP | Unreal MCP | Godot MCP |
|---|---|---|---|---|
| **Architecture** | Embedded in-process | External bridge | External bridge | External bridge |
| **Component access** | Reflection-based, automatic | Manual serialization | Manual serialization | Manual serialization |
| **ECS mutation safety** | Deferred commands at schedule boundaries | No built-in guard | No built-in guard | No built-in guard |
| **Permission system** | 3 levels (Read / Write / Full) | Basic or none | Basic or none | Basic or none |
| **Tool count** | 61 | ~15-20 | ~15-20 | ~10-15 |
| **Network overhead** | Zero (in-process queues) | IPC / socket | IPC / socket | IPC / socket |
| **Input injection** | Keyboard, mouse, gamepad | Limited | Limited | Limited |
| **Screenshot capture** | Any camera, any render target | Viewport only | Viewport only | Viewport only |
| **Batch operations** | Atomic, dry-run, verify modes | Not available | Not available | Not available |
| **Async operation tracking** | Built-in | Not available | Not available | Not available |
| **License** | MIT / Apache-2.0 | Varies | Varies | Varies |

bevy-mcp's embedded architecture means your agent doesn't talk to a bridge process — it talks directly to the ECS through shared memory. Reflection-based access means you never write per-component serialization code. Deferred commands mean mutations are safe by default.

---

## Agent Setup Guides

bevy-mcp works with any MCP-compatible client. Configuration is the same pattern everywhere — point the client at your game binary:

| Agent | Setup | Guide |
|---|---|---|
| **Claude Code** | Add to `.mcp.json` or `~/.claude/settings.json` | [Guide](docs/guides/claude-code.md) |
| **Claude Desktop** | Add to Settings → MCP Servers | [Guide](docs/guides/claude-desktop.md) |
| **Cursor** | Add to `.cursor/mcp.json` | [Guide](docs/guides/cursor.md) |
| **Codex CLI** | Add to `~/.codex/config.toml` | [Guide](docs/guides/codex-cli.md) |
| **Gemini CLI** | Add to `.gemini/settings.json` | [Guide](docs/guides/gemini-cli.md) |
| **Cline** | Add to `.cline/mcp.json` | [Guide](docs/guides/cline.md) |
| **Local LLMs** | Ollama / LM Studio via Cline | [Guide](docs/guides/local-llms.md) |

Example for any client:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/path/to/your-game-binary",
      "args": []
    }
  }
}
```

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│              MCP Client (AI Agent)                    │
│         Claude Code · Cursor · Claude Desktop         │
└──────────────────────┬───────────────────────────────┘
                       │ stdio (JSON-RPC)
                       ▼
┌──────────────────────────────────────────────────────┐
│              bevy-mcp-server                          │
│  ┌────────────┐  ┌────────────┐  ┌────────────────┐  │
│  │ Tool Router│  │ MCP Proto  │  │ Shared Queues  │  │
│  │ (61 tools) │  │ (stdio)    │  │ (Arc<Mutex>)   │  │
│  └────────────┘  └────────────┘  └────────────────┘  │
└──────────────────────┬───────────────────────────────┘
                       │ in-process queues
                       ▼
┌──────────────────────────────────────────────────────┐
│              bevy-mcp-host (Bevy Plugin)              │
│  ┌────────────┐  ┌────────────┐  ┌────────────────┐  │
│  │ Ingress    │  │ Deferred   │  │ Reflection     │  │
│  │ System     │  │ Commands   │  │ ECS Access     │  │
│  └────────────┘  └────────────┘  └────────────────┘  │
└──────────────────────┬───────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────┐
│                   Bevy ECS World                      │
└──────────────────────────────────────────────────────┘
```

**Three crates, zero Bevy dependency in two of them:**

- **`bevy-mcp-core`** — Shared protocol types and queue definitions. No Bevy dependency. Can be used by any tooling that needs to speak the bevy-mcp protocol.
- **`bevy-mcp-server`** — MCP server implementation over stdio. No Bevy dependency. Handles JSON-RPC routing and tool dispatch.
- **`bevy-mcp-host`** — The Bevy plugin. Depends on `bevy` and bridges MCP commands into the ECS through deferred commands and reflection.

---

## Permissions

bevy-mcp enforces a permission model at the Bevy ingress boundary. Set it when you configure the plugin:

```rust
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};

// Read-only: inspect entities, components, resources, events (default)
.with_permissions(McpPermissions::read_only())

// Write: everything above + ECS mutation (spawn, insert, update, remove)
.with_permissions(McpPermissions::write())

// Full: everything above + input injection, runtime control, UI interaction
.with_permissions(McpPermissions::full())
```

The permission level is enforced server-side — the agent cannot bypass it regardless of what it requests. Use `Read` during development observation, `Write` for automated testing, and `Full` when you want the agent to actually play the game.

---

## FAQ

### Does the agent need to be running before I start my game?

Yes. bevy-mcp is embedded in your game binary — the agent launches your game as a subprocess and communicates over stdio. Your game doesn't run standalone and then "connect" to an external MCP server. The MCP server *is* your game.

### Will this slow down my game?

Negligibly. Read operations are simple ECS queries with reflection. Write operations are deferred and batched at schedule boundaries. The MCP server runs on a separate thread. In practice, the overhead is comparable to having a debug inspector open.

### Do I need to write serialization code for my components?

No. If your component implements `Reflect` (which most Bevy components do), bevy-mcp can read and write it automatically through the type registry. No per-component bridge code needed.

### Can the agent break my game?

Only if you let it. The `Read` permission level is completely safe — observation only. `Write` allows ECS mutation but not input injection. `Full` gives the agent keyboard/mouse/gamepad control. Start with `Read` and escalate as needed.

### What happens if the agent sends a bad mutation?

Deferred commands are validated before application. Invalid component data, missing entities, or type mismatches return structured errors to the agent — they don't crash your game. The permission system adds another layer of protection.

### Does this work with hot-reloading?

Asset hot-reload works through Bevy's standard `asset_reload` tool. Code hot-reloading (via `bevy-inspector-egui` style approaches) is orthogonal — bevy-mcp doesn't interfere with it.

### Can I use this in production builds?

bevy-mcp is designed for development and testing workflows. You'd typically gate the `BevyMcpPlugin` behind a feature flag and exclude it from release builds. The permission system provides safety, but an open MCP endpoint in a shipped game is not recommended.

---

## Contributing

Contributions are welcome! Whether it's a bug report, a new tool, or documentation improvement — open an issue or submit a pull request.

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on development setup, testing, and code style.

---

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
