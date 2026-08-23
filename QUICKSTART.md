# Quick Start

Get the current `v.01` development build of bevy-mcp running inside a Bevy 0.19 game.

> `v.01` is the active development branch and may be ahead of crates.io. Use matching source or git dependencies when you want the capabilities documented in this repository.

## 1. Add matching dependencies

For a checkout next to your game, use path dependencies:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-core = { path = "../Bevy-mcp/crates/bevy-mcp-core" }
bevy-mcp-host = { path = "../Bevy-mcp/crates/bevy-mcp-host" }
bevy-mcp-server = { path = "../Bevy-mcp/crates/bevy-mcp-server" }
rmcp = { version = "3", features = ["server", "transport-io"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

Equivalent matching git dependencies are also fine. Avoid mixing published and source versions across the three bevy-mcp crates.

## 2. Embed the full agent server

`AgentBevyMcpServer` combines the base, advanced, and debugger/playtest routers. `BevyMcpServer` is intentionally the smaller base/legacy surface.

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
                    .with_permissions(McpPermissions::read_only()),
            )
            .run();
    });

    let server = AgentBevyMcpServer::new(state).serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
```

Start with `read_only()` when exploring. Use `write()` for reflected/state mutation and `full()` when the agent must inject input, run interactive playtests, or control the runtime. The `capabilities` tool reports what is implemented, available, and allowed in the current game.

## 3. Build the game with Cargo

```bash
cargo build
```

Cargo build/check/test are development-shell operations. The embedded MCP tools named `build_check`, `build`, and `test` are deliberately unavailable in the current host and return `BUILD_NOT_AVAILABLE`.

## 4. Point your MCP client at the game binary

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/your-game/target/debug/your-game-name",
      "args": []
    }
  }
}
```

The game binary is both the Bevy application and the stdio MCP server. Client-specific configuration lives under [docs/guides](docs/guides/).

## 5. Verify the live contract

Start with:

- `capabilities` — authoritative live feature/availability/permission contract
- `health` — connection/runtime health
- `world_summary` — ECS overview
- `world_context_scan` — richer agent-oriented context
- `entity_query` / `component_get` — inspect concrete state

For autonomous workflows, continue with assertions, native interaction, watchpoints/playtests, checkpoints/replay, and atomic reflected mutation batches as reported available by `capabilities`.

## 6. Make your game agent-aware

Reflection works immediately for registered reflected types, but the strongest workflows use a small game adapter: semantic actions, typed state, checkpoint resources, and explicit system-access metadata. See [Agent adapter checklist](docs/agent-adapter.md).
