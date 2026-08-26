# Quick Start

Get the current `v.01` development build of bevy-mcp connected to a Bevy 0.19 game.

> `v.01` is an unreleased development branch and may be ahead of crates.io. Keep the bevy-mcp crates/binary on one matching source revision or release tag.

## Choose an execution mode

- **Supervised mode — recommended for autonomous coding.** The MCP client talks to a persistent `bevy-mcp` process while the game can be checked, rebuilt, restarted, and reconnected underneath it.
- **Embedded mode — simplest for runtime-only inspection/control.** The instrumented game binary is itself the MCP stdio server.

## Supervised mode

### 1. Instrument the game

For a checkout next to your game:

```toml
[dependencies]
bevy = "0.19"
bevy-mcp-host = { path = "../Bevy-mcp/crates/bevy-mcp-host" }
```

Enable the bridge supplied by the supervisor:

```rust
use bevy::prelude::*;
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};

fn main() {
    let mcp = BevyMcpPlugin::new()
        .with_permissions(McpPermissions::full())
        .with_supervisor_bridge_from_env()
        .expect("supervisor environment is required in supervised mode");

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(mcp)
        .run();
}
```

### 2. Get the persistent supervisor

For tagged releases, the easiest path is to download the prebuilt supervisor from the repository's **Releases** page:

- Windows x86_64: `bevy-mcp-windows-x86_64.zip`
- Linux x86_64: `bevy-mcp-linux-x86_64.tar.gz`

Each archive has a matching `.sha256` checksum file. See [Install bevy-mcp](docs/install.md) for extraction and verification instructions.

If you are working directly from the current `v.01` source branch, or need another platform, build it locally instead:

```bash
cargo build --locked -p bevy-mcp-supervisor --bin bevy-mcp
```

### 3. Point the MCP client at the supervisor

Using a downloaded release binary:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/bevy-mcp",
      "args": [
        "--project-dir",
        "/absolute/path/to/your-bevy-game"
      ]
    }
  }
}
```

On Windows, the command points to `bevy-mcp.exe`. For a local debug build from the repository, point it at `target/debug/bevy-mcp` instead.

If the workspace has more than one binary target, pass `package` and/or `bin` to build/rebuild tools instead of relying on automatic target selection.

### 4. Verify and launch the development loop

Start with:

- `development_status` — compact diagnosis and recommended next action
- `capabilities` — complete implementation/availability/permission contract

If no managed game is running, `rebuild_restart` can check, build, launch the Cargo-reported executable, authenticate the bridge, and wait for host readiness. Poll its operation ID with `operation_status`.

The normal loop is:

```text
edit source -> rebuild_restart -> operation_status -> development_status
            -> inspect/interact/assert/debug -> repeat
```

See [Supervised mode and autonomous rebuild/restart](docs/supervised-mode.md) for lifecycle permissions, failure semantics, process evidence, target discovery, and troubleshooting.

## Embedded mode

Use embedded mode when the game does not need to survive source rebuilds inside the same MCP session. It requires `bevy-mcp-core`, `bevy-mcp-host`, and `bevy-mcp-server`; create `AgentBevyMcpServer` with shared ingress/result queues and point the MCP client directly at the resulting game binary.

The client-specific guides under [docs/guides](docs/guides/) document this embedded setup in detail.

In embedded mode, Cargo build/check/test and OS process lifecycle remain external to the game process. Call `capabilities` rather than assuming supervisor-only tools are available.

## Make the game agent-aware

Reflection works immediately for registered reflected types, but the strongest workflows add semantic actions, typed state, checkpoint resources, capture targets, and exact system-access metadata. See the [Agent adapter checklist](docs/agent-adapter.md).
