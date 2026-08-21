# Quick Start

## 1. Install the MCP server

```bash
cargo install bevy-mcp-server
```

## 2. Add the Bevy plugin to your game

In your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-host = "0.1"
```

In your `main.rs`:

```rust
use bevy::prelude::*;
use bevy_mcp_host::BevyMcpPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(BevyMcpPlugin::new())
        .run();
}
```

## 3. Configure your MCP client

### Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or
`%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "bevy": {
      "command": "bevy-mcp",
      "args": []
    }
  }
}
```

### Claude Code

The MCP server is automatically detected from the `bevy-mcp` binary.

## 4. Run your game

```bash
cargo run
```

## 5. Use the MCP tools

In your AI agent, you can now use tools like:

- `health` — Check if the Bevy app is connected
- `world_summary` — See entity count and archetypes
- `entity_query` — Find entities by component
- `component_get` — Read component values
- `entity_spawn` — Create new entities
- `runtime_pause` / `runtime_resume` — Control simulation

## Example Workflow

```
Agent: health
→ {"entity_count": 42, "frame": 1234, "paused": false}

Agent: entity_query(with_components=["Player"])
→ {"entities": [{"handle": "entity://default/main/5/0", "id": 5}]}

Agent: component_get(entity="entity://default/main/5/0", component="Health")
→ {"value": {"health": 100.0, "max_health": 100.0}}

Agent: component_update(entity="entity://default/main/5/0", component="Health", value={"health": 75.0})
→ {"updated": true}

Agent: capture_game()
→ {"status": "requested"}
```
