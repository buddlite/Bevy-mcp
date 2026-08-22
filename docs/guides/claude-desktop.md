# Using bevy-mcp with Claude Desktop

Connect [Claude Desktop](https://claude.ai/download) to your live Bevy game for ECS inspection, mutation, and runtime control — all from the desktop app.

---

## What You'll Need

- **Claude Desktop** installed ([download](https://claude.ai/download))
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

## Step 4: Configure Claude Desktop

### Option A: Edit the config file directly

Find your config file:

| OS | Path |
|---|---|
| **macOS** | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| **Windows** | `%APPDATA%\Claude\claude_desktop_config.json` |

Open it and add the bevy-mcp server:

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

> If the file already has other servers, add `"bevy"` inside the existing `"mcpServers"` object.

### Option B: Use the Settings UI

1. Open Claude Desktop
2. Go to **Settings** (gear icon) → **Developer** → **Edit Config**
3. This opens `claude_desktop_config.json` in your editor
4. Add the `bevy` server entry as shown above
5. Save the file

### Windows example

```json
{
  "mcpServers": {
    "bevy": {
      "command": "C:\\Users\\you\\projects\\my-game\\target\\debug\\my-game.exe",
      "args": []
    }
  }
}
```

> Use double backslashes (`\\`) or forward slashes (`/`) in the path on Windows.

---

## Step 5: Restart Claude Desktop

Quit and reopen Claude Desktop for the config change to take effect.

You should see a small hammer icon (🔧) in the chat input area indicating that MCP tools are available. Click it to confirm bevy-mcp tools (health, world_summary, entity_query, etc.) are listed. If the icon is missing or tools don't appear, see Troubleshooting below.

---

## Example Workflow

```
You: Check the health of my running Bevy game

Claude: [calls health tool]
→ {"status": "ok", "entity_count": 47, "frame": 312, "paused": false}
```

```
You: What entities have a Transform component?

Claude: [calls entity_query]
→ Found 23 entities with Transform
```

```
You: Show me the Camera entity and take a screenshot

Claude: [calls camera_list, then capture_game]
→ Screenshot captured from MainCamera
```

```
You: Spawn a new entity with a Sprite component at position (100, 200, 0)

Claude: [calls entity_spawn with Sprite and Transform components]
→ Entity spawned: entity://default/main/48/0
```

---

## Tips

- **Claude Desktop launches your game automatically.** You don't need to run the game separately — the app spawns it as a subprocess when a conversation uses MCP tools.
- **The game shuts down when you close the chat or quit Claude Desktop.** This is expected.
- **Rebuild after code changes.** Claude Desktop launches whatever binary exists at the configured path.
- **Use the hammer icon** to see which MCP tools are available in the current conversation.
- **Permission levels matter.** Start with `McpPermissions::read_only()` if you only want Claude to observe. Use `full()` for input injection and runtime control.

---

## Troubleshooting

- **Binary not found / path errors:** Ensure the path in `claude_desktop_config.json` is the absolute path to your compiled game binary, not the source directory. On Windows, use double backslashes (`\\`) or forward slashes.
- **MCP server not appearing:** Verify the binary compiles and runs standalone first — `cargo build`, then run the binary directly in a terminal.
- **Tools not showing up:** Quit and reopen Claude Desktop after changing the config. The app caches tool lists per session.
- **Permission errors:** The default permission is `read_only()`. If mutation tools are missing, upgrade to `McpPermissions::write()` or `McpPermissions::full()`.
- **Game crashes on startup:** Run the binary directly in a terminal to see error output — Claude Desktop swallows subprocess stderr.
