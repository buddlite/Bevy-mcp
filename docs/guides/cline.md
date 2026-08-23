# Using bevy-mcp with Cline

Connect [Cline](https://github.com/cline/cline) (VS Code extension) to your live Bevy game for ECS inspection, mutation, and runtime control — all from inside VS Code.

> This guide targets the current `v.01` development surface. Use `AgentBevyMcpServer` for the full base + advanced + debugger/playtest router; call `capabilities` to discover what is available and permitted at runtime.

---

## What You'll Need

- **VS Code** with the **Cline** extension installed ([Cline on Marketplace](https://marketplace.visualstudio.com/items?itemName=saoudrizwan.claude-dev))
- **Rust toolchain** (rustup, cargo)
- **A Bevy project** (existing or new)

---

## Step 1: Add bevy-mcp to Your Project

In your `Cargo.toml`:

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

> These examples target the current `v.01` source tree. Keep all three bevy-mcp crates on the same source/release version.

---

## Step 2: Add the Plugin to Your App

In your `main.rs`:

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

> **Permission levels:** Start with `McpPermissions::read_only()` for safe observation. Use `write()` for ECS mutation, and `full()` when you want the agent to inject input and control runtime.

---

## Step 3: Build Your Game

```bash
cargo build
```

---

## Step 4: Configure Cline

### Option A: Use the Cline Settings UI

1. Open VS Code with Cline installed
2. Click the **Cline** icon in the sidebar
3. Click the **MCP Servers** icon (plug icon) in the Cline panel
4. Click **Edit MCP Settings** (or **Configure MCP Servers**)
5. Add the bevy server:

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

6. Save the file

### Option B: Edit the config file directly

Create or edit `.cline/mcp.json` in your project root:

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

---

## Step 5: Verify the Connection

1. Open the Cline panel in VS Code
2. Click the **MCP Servers** icon (plug icon)
3. You should see `bevy` listed with a green status indicator
4. If it shows red, click the refresh icon to retry the connection

---

## Example Workflow

```
You: Check the health of my Bevy game

Cline: [calls health tool]
→ {"status": "ok", "entity_count": 47, "frame": 312, "paused": false}
```

```
You: List all cameras and take a screenshot

Cline: [calls camera_list]
→ Found 2 cameras: MainCamera (active), UICamera

Cline: [calls capture_game]
→ Screenshot captured from MainCamera
```

```
You: Query all entities with both Transform and Velocity, 
     then set all their velocity to zero

Cline: [calls entity_query with filters]
→ Found 15 entities

Cline: [calls component_update on each]
→ Updated velocity on 15 entities
```

```
You: Run cargo test and show me the results

Cline: [runs `cargo test` in its development shell]
→ 12 tests passed, 0 failed
```

---

## Tips

- **Cline works well for iterative game dev.** Ask it to observe game state, make code changes, rebuild, and verify — all in one conversation.
- **The MCP server icon shows connection status.** Green means connected, red means the game binary couldn't start or crashed.
- **Start with read-only permissions.** Use `McpPermissions::read_only()` during initial development, then switch to `write()` or `full()` as needed.
- **Rebuild after code changes.** Cline launches whatever binary exists at the configured path. After `cargo build`, the next MCP connection uses the new build.
- **Use Cline's auto-approve carefully.** If you have auto-approve enabled for tool calls, the agent may call MCP tools without confirmation. Start with manual approval.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in your MCP config is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart VS Code after changing MCP config. Click the refresh icon next to the MCP server in Cline's panel to force a reconnection.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Cline swallows stderr from the MCP subprocess.
