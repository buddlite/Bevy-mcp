# Using bevy-mcp with Gemini CLI

Connect [Gemini CLI](https://github.com/google-gemini/gemini-cli) to your live Bevy game for ECS inspection, mutation, and runtime control — all from the terminal.

> This guide targets the current `v.01` development surface. Use `AgentBevyMcpServer` for the full base + advanced + debugger/playtest router; call `capabilities` to discover what is available and permitted at runtime.

> **Mode note:** This guide documents **embedded mode**, where the MCP client launches the instrumented game binary directly. For autonomous Rust edit/build/restart workflows, use the persistent [supervised mode](../supervised-mode.md), which keeps the MCP session alive across game rebuilds and crashes.

---

## What You'll Need

- **Gemini CLI** installed (`npm install -g @google/gemini-cli` or see [Gemini CLI docs](https://github.com/google-gemini/gemini-cli))
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

## Step 4: Configure Gemini CLI

Gemini CLI uses a `settings.json` file for MCP server configuration.

### Option A: Project-scoped config

Create `.gemini/settings.json` in your project root:

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

### Option B: Global config

Edit `~/.gemini/settings.json`:

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

Launch Gemini CLI in your project directory:

```bash
gemini
```

Ask *"What MCP tools are available?"* — you should see the bevy-mcp tool list (health, world_summary, entity_query, etc.). If no tools appear, see Troubleshooting below.

---

## Step 6: Start Using It

Once tools are verified, you can interact with your game. Try asking Gemini to call `health` or `world_summary` to explore your ECS.

---

## Example Workflow

```
You: What's in the Bevy world right now?

Gemini: [calls world_summary]
→ 47 entities across 12 archetypes. Top components: Transform (31), Sprite (18), Player (1)
```

```
You: Show me the player's Health component

Gemini: [calls entity_query for Player, then component_get for Health]
→ {"health": 100.0, "max_health": 100.0}
```

```
You: Spawn a new enemy at position (500, 300, 0)

Gemini: [calls entity_spawn with Enemy, Health, Transform components]
→ Entity spawned: entity://default/main/48/0
```

```
You: Run cargo build and check for errors

Gemini: [runs `cargo build` in its development shell]
→ Build succeeded with 0 warnings
```

---

## Tips

- **Gemini CLI supports MCP natively.** No plugins or extensions needed — just add the config and go.
- **Project-scoped config is recommended.** It keeps the bevy-mcp config with your project and avoids conflicts with other projects.
- **The game binary runs as a subprocess.** Gemini CLI launches it when the conversation starts and shuts it down when you exit.
- **Use `diagnostics` for quick health checks.** Ask Gemini to call `diagnostics` to see FPS, frame time, and entity count.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in `settings.json` is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart Gemini CLI after changing the config. Some sessions cache the tool list.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Gemini CLI swallows stderr from the subprocess.
