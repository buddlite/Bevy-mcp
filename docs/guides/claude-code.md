# Using bevy-mcp with Claude Code

Connect [Claude Code](https://docs.anthropic.com/en/docs/claude-code) to your live Bevy game for ECS inspection, mutation, and runtime control — all from the terminal.

---

## What You'll Need

- **Claude Code** installed and working (`claude --version` should respond)
- **Rust toolchain** (rustup, cargo)
- **A Bevy project** (existing or new)

---

## Step 1: Add bevy-mcp to Your Project

In your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-host = "0.1"
```

---

## Step 2: Add the Plugin to Your App

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

> **Permission levels:** Start with `McpPermissions::read_only()` for safe observation. Use `write()` for ECS mutation, and `full()` when you want the agent to inject input and control runtime.

---

## Step 3: Build Your Game

```bash
cargo build
```

Note the path to your compiled binary. For debug builds:

```
target/debug/your-game-name
```

---

## Step 4: Configure Claude Code

Create or edit `.mcp.json` in your project root:

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

**Or** add it globally via `~/.claude/settings.json`:

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

> Replace `/absolute/path/to/your-game/target/debug/your-game-name` with the actual path to your compiled binary.

---

## Step 5: Verify the Connection

Launch Claude Code in your project directory and check that bevy-mcp tools appear:

```bash
claude
```

Type `/tools` or ask *"What MCP tools are available?"* — you should see the bevy-mcp tool list (health, world_summary, entity_query, etc.). If no tools appear, see Troubleshooting below.

---

## Step 6: Start Using It

Once tools are verified, you can interact with your game. Try asking Claude Code to call `health` or `world_summary` to explore your ECS.

---

## Example Workflow

Here's a typical session:

```
You: Check the health of the Bevy game

Claude: [calls health tool]
→ {"status": "ok", "entity_count": 47, "frame": 312, "paused": false}
```

```
You: What entities have a Player component?

Claude: [calls entity_query with component filter "Player"]
→ Found 1 entity: entity://default/main/5/0 (Player)
```

```
You: Show me the Health component on that entity

Claude: [calls component_get]
→ {"health": 100.0, "max_health": 100.0}
```

```
You: Set the player health to 75

Claude: [calls component_update with {"health": 75.0}]
→ Component updated successfully
```

```
You: Take a screenshot of the game

Claude: [calls capture_game]
→ Screenshot saved
```

```
You: Pause the game and step forward 3 frames

Claude: [calls runtime_pause, then runtime_step x3]
→ Game paused, stepped to frame 315
```

---

## Tips

- **Start with read-only permissions** during development. Switch to `write` or `full` when you need the agent to mutate state.
- **Use `world_summary` first** — it gives the agent a high-level map of your ECS (entity count, archetype list) that helps it reason about follow-up queries.
- **The game binary runs as a subprocess.** When Claude Code disconnects, the game shuts down. This is expected — the next connection restarts it.
- **Rebuild after code changes.** The agent launches whatever binary exists at the configured path. If you recompile, the next connection uses the new build.
- **Check `diagnostics` for performance.** Ask the agent to call `diagnostics` to see FPS, frame time, and entity count in real time.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in `.mcp.json` is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart Claude Code after changing MCP config. Some sessions cache the tool list.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Claude Code swallows stderr from the subprocess.
