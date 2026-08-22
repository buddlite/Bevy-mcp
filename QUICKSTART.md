# Quick Start

Get bevy-mcp running with your Bevy game in 5 minutes.

---

## 1. Add dependencies

In your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-host = "0.1"
```

---

## 2. Add the plugin to your app

In your `main.rs`:

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

> **Permission levels:** `read_only()` for observation, `write()` for ECS mutation, `full()` for input injection and runtime control. See [Permissions](README.md#permissions).

---

## 3. Build your game

```bash
cargo build
```

---

## 4. Configure your MCP client

Point your MCP client at the compiled game binary — the game binary *is* the MCP server:

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

The exact config file depends on your agent. See the [agent setup guides](docs/README.md) for client-specific instructions (Claude Desktop, Cursor, Codex CLI, etc.).

---

## 5. Run and explore

Your MCP client will launch the game binary automatically when it needs MCP tools. Once connected, try asking your agent to:

- `health` — Check if the Bevy app is connected
- `world_summary` — See entity count and archetypes
- `entity_query` — Find entities by component
- `component_get` — Read component values
- `diagnostics` — View FPS, frame time, entity count

## Example Workflow

```
Agent: health
→ {"status": "ok", "entity_count": 42, "frame": 1234, "paused": false}

Agent: world_summary
→ {"entity_count": 42, "archetype_count": 8, "component_types": ["Transform", "Sprite", "Player", ...]}

Agent: entity_query(with_components=["Player"])
→ {"entities": [{"handle": "entity://default/main/5/0", "id": 5}]}

Agent: component_get(entity="entity://default/main/5/0", component="Health")
→ {"value": {"health": 100.0, "max_health": 100.0}}

Agent: capture_game()
→ {"status": "requested", "screenshot_id": "..."}
```

> To run mutations (spawn, update, remove), change permissions to `McpPermissions::write()`. To inject input and control the simulation, use `McpPermissions::full()`.
