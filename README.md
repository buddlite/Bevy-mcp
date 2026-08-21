# bevy-mcp

An MCP (Model Context Protocol) server for the [Bevy](https://bevyengine.org/) game engine. Enables AI agents to inspect, mutate, and control a running Bevy application.

## Features

- **Embedded MCP bridge** for ECS inspection, mutation, runtime control, and input injection
- **Reflection-based** component reading/writing via Bevy's type registry
- **Deferred command architecture** — mutations execute at safe schedule boundaries
- **Enforced permission system** — controls operations at the Bevy ingress boundary (Read/Write/Full)
- **Sequential read batches** with a preview mode

## Installation

### Embed the server in your Bevy application

The server and Bevy host intentionally communicate through in-process shared queues.
Use the same executable as your MCP command; the standalone `bevy-mcp` binary cannot
attach to an arbitrary, already-running Bevy process.

Add to your `Cargo.toml`:

```toml
[dependencies]
bevy-mcp-host = "0.1"
```

Then add the plugin to your Bevy app:

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

## MCP Client Configuration

### Claude Desktop / Claude Code

Add to your MCP settings:

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

### With a Bevy project

Point the client at the executable that embeds both the Bevy host and `BevyMcpServer`.
An external `bevy-mcp` process does not automatically connect to a running game.

## Available Tools

### Session & Environment
| Tool | Description |
|------|-------------|
| `health` | Server health, FPS, entity count, bevy version |
| `capabilities` | List server capabilities |
| `instances` | List connected Bevy app instances |
| `project_info` | Project name, path, bevy version |
| `runtime_status` | Paused state, time scale, frame count |
| `errors` | Recent application errors |

### ECS Inspection
| Tool | Description |
|------|-------------|
| `world_summary` | Entity count, archetypes, component types |
| `entity_query` | Query entities by component filters |
| `entity_get` | Full component list for an entity |
| `component_get` | Read component value via reflection |
| `component_schema` | Type info (fields, variants) |

### ECS Mutation
| Tool | Description |
|------|-------------|
| `entity_spawn` | Spawn entity with components |
| `entity_despawn` | Despawn entity |
| `entity_reparent` | Reparent entity in hierarchy |
| `component_insert` | Insert component (reflection deserialize) |
| `component_update` | Update component value |
| `component_remove` | Remove component |

### Resources
| Tool | Description |
|------|-------------|
| `resource_list` | List all registered resources |
| `resource_get` | Read resource value via reflection |
| `resource_schema` | Resource type info |
| `resource_update` | Update resource value |

### Runtime Control
| Tool | Description |
|------|-------------|
| `runtime_pause` | Pause simulation |
| `runtime_resume` | Resume simulation |
| `runtime_step` | Advance N frames |
| `runtime_time_scale` | Set time scale multiplier |

### Input
| Tool | Description |
|------|-------------|
| `input_key` | Keyboard key press/release |
| `input_mouse` | Mouse motion/button |

### UI
| Tool | Description |
|------|-------------|
| `ui_query` | Query UI tree (Node, Text, Button) |
| `ui_inspect` | Inspect UI element details |

### Capture & Camera
| Tool | Description |
|------|-------------|
| `camera_list` | List cameras in scene |
| `camera_inspect` | Inspect camera properties |

### Assets
| Tool | Description |
|------|-------------|

### Events & Diagnostics
| Tool | Description |
|------|-------------|
| `observe_events` | Query captured events |
| `logs` | Log output by level |
| `diagnostics` | FPS, frame time, entity count |

### Operations
| Tool | Description |
|------|-------------|
| `operation_status` | Status of async operations |
| `operation_cancel` | Cancel running operation |
| `batch` | Run supported read operations sequentially or preview them |

### Playtest
| Tool | Description |
|------|-------------|
| `assert` | Assert game state condition |

### Hierarchy & Plugins
| Tool | Description |
|------|-------------|
| `hierarchy` | Entity parent-child tree |
| `list_plugins` | List installed Bevy plugins |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      MCP Client (AI Agent)                  │
└─────────────────────────────────────────────────────────────┘
                              │ stdio
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    bevy-mcp-server                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Tool Router │  │  MCP Proto  │  │  Shared Queues      │ │
│  └─────────────┘  └─────────────┘  │  (Arc<Mutex>)       │ │
│                                     └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      bevy-mcp-host                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Ingress     │  │  Deferred   │  │  ECS Systems        │ │
│  │  System      │  │  Commands   │  │  (reflection-based) │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Bevy ECS World                         │
└─────────────────────────────────────────────────────────────┘
```

## Permissions

Control what operations the MCP server can perform:

```rust
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions, PermissionLevel};

App::new()
        .add_plugins(BevyMcpPlugin::new()
        .with_permissions(McpPermissions::read_only())  // Query only (default)
        // .with_permissions(McpPermissions::write())    // Query + ECS mutation
        // .with_permissions(McpPermissions::full())     // Also input and runtime controls
    )
    .run();
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
