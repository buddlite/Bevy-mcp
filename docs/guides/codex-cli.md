# Using bevy-mcp with Codex CLI

Connect [Codex CLI](https://github.com/openai/codex) to your live Bevy game for ECS inspection, mutation, and runtime control — all from the terminal.

> This guide targets the current `v.01` development surface. Use `AgentBevyMcpServer` for the full base + advanced + debugger/playtest router; call `capabilities` to discover what is available and permitted at runtime.

> **Mode note:** This guide documents **embedded mode**, where the MCP client launches the instrumented game binary directly. For autonomous Rust edit/build/restart workflows, use the persistent [supervised mode](../supervised-mode.md), which keeps the MCP session alive across game rebuilds and crashes.

---

## What You'll Need

- **Codex CLI** installed (`npm install -g @openai/codex` or see [Codex docs](https://github.com/openai/codex))
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

## Step 4: Configure Codex CLI

### Option A: Use the CLI command

```bash
codex mcp add bevy -- /absolute/path/to/your-game/target/debug/your-game-name
```

### Option B: Edit the config file

Edit `~/.codex/config.toml`:

```toml
[mcp_servers.bevy]
command = "/absolute/path/to/your-game/target/debug/your-game-name"
args = []
```

### Project-scoped config

You can also create `.codex/config.toml` in your project root to keep the config local to the project.

---

## Step 5: Verify the Connection

Launch Codex in your project directory:

```bash
codex
```

Ask *"What MCP tools are available?"* — you should see the bevy-mcp tool list (health, world_summary, entity_query, etc.). If no tools appear, see Troubleshooting below.

---

## Step 6: Start Using It

Once tools are verified, you can interact with your game. Try asking Codex to call `health` or `world_summary` to explore your ECS.

---

## Example Workflow

```
You: What's the current state of the Bevy ECS?

Codex: [calls world_summary]
→ 47 entities across 12 archetypes. Top components: Transform (31), Sprite (18), Player (1)
```

```
You: Find all enemies and double their speed

Codex: [calls entity_query for Enemy]
→ Found 8 enemies

Codex: [calls component_get on each, then component_update to double speed]
→ Updated velocity on 8 entities
```

```
You: Pause the game and show me a screenshot

Codex: [calls runtime_pause, then capture_game]
→ Game paused at frame 1204. Screenshot captured.
```

---

## Tips

- **Codex supports `full-auto` mode.** In this mode, Codex can autonomously call MCP tools without asking for confirmation. Use it for batch operations.
- **Use `--approval-mode full-auto`** when launching Codex for unattended game testing workflows.
- **The game binary runs as a subprocess.** Codex launches it when needed and shuts it down when the session ends.
- **Rebuild after code changes.** The next Codex session uses whatever binary exists at the configured path.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in your Codex MCP config is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart Codex after changing MCP config. Some sessions cache the tool list.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Codex swallows stderr from the subprocess.
