# Using bevy-mcp with Cursor

Connect [Cursor IDE](https://cursor.sh) to your live Bevy game for ECS inspection, mutation, and runtime control — all from inside your editor.

---

## What You'll Need

- **Cursor IDE** installed ([download](https://cursor.sh))
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

Note the path to your compiled binary (`target/debug/your-game-name`).

---

## Step 4: Configure Cursor

Create `.cursor/mcp.json` in your project root:

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

> Replace the path with your actual binary location. Use the absolute path.

### Alternatively: Use Cursor Settings UI

1. Open Cursor Settings (`Ctrl+,` / `Cmd+,`)
2. Search for **MCP**
3. Click **Add new MCP server**
4. Set:
   - **Name:** `bevy`
   - **Type:** `command`
   - **Command:** `/absolute/path/to/your-game/target/debug/your-game-name`
5. Save

---

## Step 5: Verify the Connection

1. Open Cursor's **Agent** panel (Composer or Chat)
2. You should see the bevy-mcp tools listed under available MCP tools
3. Try asking: *"Check the health of the Bevy game"*

If the tools don't appear, click the refresh icon next to the MCP server in Cursor Settings.

---

## Using Cursor Composer with bevy-mcp

Cursor Composer (the multi-file agent mode) can use bevy-mcp tools alongside code editing. This is where bevy-mcp shines in Cursor — the agent can:

1. **Read game state** to understand what's happening at runtime
2. **Edit your source code** based on what it observes
3. **Rebuild and reconnect** to verify the changes

### Example Composer Session

```
You: Look at the player entity and tell me what components it has. 
     Then add a health regeneration system.

Composer: [calls entity_query for Player]
→ Player entity has: Transform, Sprite, Health, Velocity, Player

Composer: [reads your source code, creates a new system]
→ Added health_regen_system: +1 HP per second when Health < max_health

Composer: [calls cargo_check]
→ Build succeeded

You: Set permissions to full and make the player take 10 damage, 
     then watch if regeneration kicks in

Composer: [calls component_update to set health to 90]
→ Health set to 90

Composer: [waits a few frames via runtime_step]
→ Health is now 91 — regeneration is working
```

---

## Tips

- **Use Composer, not just Chat.** Composer can edit files AND use MCP tools in the same session. Chat is read-only for files.
- **The game runs as a subprocess of Cursor.** When Cursor restarts the MCP connection, the game binary restarts too.
- **Rebuild triggers.** After `cargo build`, the next MCP connection uses the new binary. You don't need to manually restart anything in Cursor.
- **Check the MCP status indicator.** Cursor shows a green dot next to connected MCP servers and a red dot for failed connections.
- **Start with read-only permissions** if you're just exploring. Switch to `full()` when you need the agent to control runtime.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in `.cursor/mcp.json` is the absolute path to your compiled game binary, not the source directory. Check that the binary exists after `cargo build`.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run `target/debug/your-game-name` directly in a terminal.
- **Tools not showing up:** Restart Cursor after changing MCP config. Click the refresh icon next to the MCP server in Settings to force a reconnection.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Cursor swallows stderr from the MCP subprocess.
