from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")


# ---------------------------------------------------------------------------
# Root README: make the development/release story explicit and remove a badge
# that can imply the development branch matches the published crate.
# ---------------------------------------------------------------------------
readme = read("README.md")
readme = readme.replace(
    '[![Crates.io](https://img.shields.io/crates/v/bevy-mcp-host)](https://crates.io/crates/bevy-mcp-host)\n',
    '',
)
status_note = (
    '> **Development status:** `v.01` is the active development branch and may be ahead of '
    'published crates. The examples below use matching source dependencies so the documented '
    'tool surface and the code you run stay aligned.\n\n'
)
marker = "</div>\n\n---\n"
if status_note not in readme:
    readme = readme.replace(marker, f"</div>\n\n{status_note}---\n", 1)
readme = readme.replace(
    "Use matching published crate versions when consuming releases from crates.io.\n",
    "For the current `v.01` development surface, use a source checkout or matching git dependencies. Use crates.io only when a tagged release explicitly documents the same capability set.\n",
)
agent_hooks = (
    "These adapters let an agent move from generic ECS manipulation toward higher-level operations such as \"start mission\", \"buy upgrade\", \"enter build mode\", or \"restore this test state\" without hard-coding those concepts into bevy-mcp itself.\n"
)
if "docs/agent-adapter.md" not in readme:
    readme = readme.replace(
        agent_hooks,
        agent_hooks + "\nSee the [agent adapter checklist](docs/agent-adapter.md) for one minimal example that registers an action, typed state, checkpoint resource, and system-access specification together.\n",
    )
write("README.md", readme)


# ---------------------------------------------------------------------------
# Changelog: current source truth only. The previous entry mixed planned and
# unavailable features and therefore was not a trustworthy release record.
# ---------------------------------------------------------------------------
write(
    "CHANGELOG.md",
    """# Changelog

All notable changes to the current development line are documented here.

The project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) conventions. Until a tagged release is cut from the current codebase, `v.01` should be treated as an **unreleased development branch** and the live `capabilities` tool remains authoritative for runtime availability.

## [Unreleased] — `v.01`

### Added

- Concurrent MCP response dispatching so base, advanced, and debugger requests can be in flight without stealing one another's responses.
- Causal-debugging surfaces: scoped change tracking, schedule/system inspection, explicit system-access metadata, writer candidates, and timing summaries.
- Checkpoint, recording, replay, watchpoint, and frame-driven playtest infrastructure.
- A truthful live capability contract that separates implementation, runtime availability, permission allowance, and deprecation state.
- Native software-pointer interaction through Bevy picking, including hit testing, move, click, drag, scroll, UI click verification, and editable-text input.
- Reflection-backed state assertions, including nested component/resource field paths.
- Known-path asset inspection, load-status reporting, and reload.
- Bounds-aware camera framing over target/descendant AABBs for perspective and orthographic cameras, including parented rigs.
- Prevalidated atomic reflected mutation batches for `component_insert`, `component_update`, `component_remove`, and `resource_update`, with validating dry-run support.

### Changed

- The normal autonomous-agent entry point is `AgentBevyMcpServer`; `BevyMcpServer` remains the base/legacy router.
- Front-page documentation now describes the live runtime surface instead of a fixed tool count.
- Agent-facing mutation, interaction, assertion, debugging, and replay flows are documented around the loop `inspect -> mutate -> step/interact -> assert -> diagnose -> replay/checkpoint -> retry`.

### Current limitations

- Embedded `build_check`, `build`, and `test` tools return `BUILD_NOT_AVAILABLE`; agents should use their trusted development shell for Cargo commands.
- `asset_list` is reserved; loaded-asset enumeration is not implemented.
- Atomic batches intentionally exclude entity lifecycle, hierarchy changes, input/runtime operations, semantic actions, and arbitrary side effects. `verify` mode is not implemented.
- Entity duplication remains reserved until safe reflected cloning is implemented.
- Embedded runtime launch/stop/restart remain externally owned.
- Generic `input_action` is not implemented; register semantic actions instead.
- Checkpoint restoration covers only explicitly registered checkpoint state/adapters.

### Release history note

The previous `0.1.0` changelog entry was removed because it described a mixture of planned and unavailable capabilities (including a fixed tool count, embedded Cargo tools, loaded-asset enumeration, broad atomic/verify batching, and entity duplication) rather than a reliable shipped-state record. A tagged release should be added here only when its published crates and documented capability set are reconciled.
""",
)


# ---------------------------------------------------------------------------
# Canonical Quick Start: source-aligned dependencies + full agent router.
# ---------------------------------------------------------------------------
write(
    "QUICKSTART.md",
    """# Quick Start

Get the current `v.01` development build of bevy-mcp running inside a Bevy 0.19 game.

> `v.01` is the active development branch and may be ahead of crates.io. Use matching source or git dependencies when you want the capabilities documented in this repository.

## 1. Add matching dependencies

For a checkout next to your game, use path dependencies:

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

Equivalent matching git dependencies are also fine. Avoid mixing published and source versions across the three bevy-mcp crates.

## 2. Embed the full agent server

`AgentBevyMcpServer` combines the base, advanced, and debugger/playtest routers. `BevyMcpServer` is intentionally the smaller base/legacy surface.

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

Start with `read_only()` when exploring. Use `write()` for reflected/state mutation and `full()` when the agent must inject input, run interactive playtests, or control the runtime. The `capabilities` tool reports what is implemented, available, and allowed in the current game.

## 3. Build the game with Cargo

```bash
cargo build
```

Cargo build/check/test are development-shell operations. The embedded MCP tools named `build_check`, `build`, and `test` are deliberately unavailable in the current host and return `BUILD_NOT_AVAILABLE`.

## 4. Point your MCP client at the game binary

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

The game binary is both the Bevy application and the stdio MCP server. Client-specific configuration lives under [docs/guides](docs/guides/).

## 5. Verify the live contract

Start with:

- `capabilities` — authoritative live feature/availability/permission contract
- `health` — connection/runtime health
- `world_summary` — ECS overview
- `world_context_scan` — richer agent-oriented context
- `entity_query` / `component_get` — inspect concrete state

For autonomous workflows, continue with assertions, native interaction, watchpoints/playtests, checkpoints/replay, and atomic reflected mutation batches as reported available by `capabilities`.

## 6. Make your game agent-aware

Reflection works immediately for registered reflected types, but the strongest workflows use a small game adapter: semantic actions, typed state, checkpoint resources, and explicit system-access metadata. See [Agent adapter checklist](docs/agent-adapter.md).
""",
)


# ---------------------------------------------------------------------------
# Canonical adapter onboarding.
# ---------------------------------------------------------------------------
write(
    "docs/agent-adapter.md",
    """# Agent Adapter Checklist

bevy-mcp works with generic reflected ECS state, but a few explicit registrations give agents a much more stable vocabulary for setup, testing, recovery, and causal debugging.

A useful first adapter should register four things:

1. one **semantic action** for stable high-level intent;
2. one **typed Bevy state** for readable/controllable game mode;
3. one **checkpoint resource** for deterministic restore coverage; and
4. one **system-access specification** for exact writer/read attribution on an important system.

## Minimal example

```rust
use bevy::prelude::*;
use bevy_mcp_host::{McpAgentAppExt, McpSystemAccessSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(States, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
enum GameState {
    #[default]
    Menu,
    Running,
}

#[derive(Resource, Default, Serialize, Deserialize)]
struct Economy {
    credits: u64,
}

#[derive(Component)]
struct Cargo;

fn install_agent_adapter(app: &mut App) {
    app.register_mcp_action(
        "give_credits",
        "Add credits to the player for agent setup/testing",
        |world, args| {
            let amount = args["amount"]
                .as_u64()
                .ok_or_else(|| "amount must be an unsigned integer".to_string())?;
            world.resource_mut::<Economy>().credits += amount;
            Ok(json!({ "granted": amount }))
        },
    )
    .register_mcp_state::<GameState>(
        "game_state",
        "Current top-level game state",
    )
    .register_mcp_checkpoint_resource::<Economy>(
        "economy",
        "Economy state restored by MCP checkpoints",
    )
    .register_mcp_system_access(
        McpSystemAccessSpec::new("cargo_transfer")
            .schedule("Update")
            .write::<Cargo>()
            .write_resource::<Economy>(),
    );
}
```

Call `install_agent_adapter(&mut app)` after the relevant state/resources are initialized.

## What each registration unlocks

| Registration | Agent benefit |
| --- | --- |
| `register_mcp_action` | Stable high-level operations for playtests/replay instead of brittle input sequences |
| `register_mcp_state` | `state_get` / `state_transition`, state evidence, and readable playtest setup |
| `register_mcp_checkpoint_resource` | Explicit deterministic checkpoint/restore coverage for that resource |
| `register_mcp_system_access` | Exact declared component/resource access for causal debugging; otherwise writer tools may only have conflict-candidate evidence |

## Recommended progression

Start with reflection and `capabilities`. Add semantic actions for repeated setup operations, then register the small set of state/resources that must survive checkpoint restore. Instrument only the systems where exact writer attribution is worth maintaining; bevy-mcp intentionally labels weaker conflict evidence rather than pretending Bevy exposes universal writer provenance.

The goal is not to mirror every game API into MCP. Register the few concepts that make autonomous tests stable and recovery deterministic.
""",
)


# ---------------------------------------------------------------------------
# Update docs index.
# ---------------------------------------------------------------------------
docs_index = read("docs/README.md")
if "agent-adapter.md" not in docs_index:
    docs_index = docs_index.replace(
        "## Quick Links\n\n",
        "## Quick Links\n\n- [Agent adapter checklist](agent-adapter.md) — register semantic actions, typed state, checkpoint resources, and exact system-access metadata\n",
    )
if "active development branch" not in docs_index:
    docs_index = docs_index.replace(
        "Setup guides, API reference, and workflows for using bevy-mcp with AI agents.\n",
        "Setup guides, API reference, and workflows for using bevy-mcp with AI agents.\n\n> `v.01` is the active development branch and may be ahead of published crates. Follow the root Quick Start for matching dependency instructions.\n",
    )
write("docs/README.md", docs_index)


# ---------------------------------------------------------------------------
# Client guides: keep client-specific connection instructions, but reconcile
# the shared Rust integration and remove claims that embedded MCP runs Cargo.
# ---------------------------------------------------------------------------
guide_paths = [
    "docs/guides/claude-code.md",
    "docs/guides/claude-desktop.md",
    "docs/guides/cursor.md",
    "docs/guides/codex-cli.md",
    "docs/guides/gemini-cli.md",
    "docs/guides/cline.md",
]

deps_old = '''```toml\n[dependencies]\nbevy = "0.19"\nbevy-mcp-host = "0.1"\n```'''
deps_new = '''```toml\n[dependencies]\nbevy = "0.19"\nbevy-mcp-core = { path = "../Bevy-mcp/crates/bevy-mcp-core" }\nbevy-mcp-host = { path = "../Bevy-mcp/crates/bevy-mcp-host" }\nbevy-mcp-server = { path = "../Bevy-mcp/crates/bevy-mcp-server" }\nrmcp = { version = "3", features = ["server", "transport-io"] }\ntokio = { version = "1", features = ["full"] }\nanyhow = "1"\n```\n\n> These examples target the current `v.01` source tree. Keep all three bevy-mcp crates on the same source/release version.'''

old_import = 'use bevy_mcp_server::tools::{BevyMcpServer, BevyMcpState};'
new_import = 'use bevy_mcp_server::AgentBevyMcpServer;\nuse bevy_mcp_server::tools::BevyMcpState;\nuse rmcp::{ServiceExt, transport::stdio};'
old_server = '    let server = BevyMcpServer::new(BevyMcpState::embedded(ingress.clone(), results.clone()));'
new_server = '    let state = BevyMcpState::embedded(ingress.clone(), results.clone());'
old_serve = '    server.serve(rmcp::transport::stdio()).await?.waiting().await?;'
new_serve = '    let server = AgentBevyMcpServer::new(state).serve(stdio()).await?;\n    server.waiting().await?;'

for path in guide_paths:
    text = read(path)
    text = text.replace(deps_old, deps_new)
    text = text.replace(old_import, new_import)
    text = text.replace(old_server, new_server)
    text = text.replace(old_serve, new_serve)
    if "current `v.01` development surface" not in text:
        first_sep = text.find("\n\n---\n")
        if first_sep != -1:
            note = "\n\n> This guide targets the current `v.01` development surface. Use `AgentBevyMcpServer` for the full base + advanced + debugger/playtest router; call `capabilities` to discover what is available and permitted at runtime."
            text = text[:first_sep] + note + text[first_sep:]
    text = text.replace("Cline: [calls cargo_test]", "Cline: [runs `cargo test` in its development shell]")
    text = text.replace("Composer: [calls cargo_check]", "Composer: [runs `cargo check` in its development shell]")
    text = text.replace("Gemini: [calls cargo_build]", "Gemini: [runs `cargo build` in its development shell]")
    text = text.replace("Codex: [calls cargo_check]", "Codex: [runs `cargo check` in its development shell]")
    text = text.replace("Codex: [calls cargo_build]", "Codex: [runs `cargo build` in its development shell]")
    write(path, text)

# Local-model guide delegates Rust integration to the root Quick Start, but
# still needs the development-version warning so readers do not mix crates.
local = read("docs/guides/local-llms.md")
if "current `v.01` development surface" not in local:
    first_sep = local.find("\n\n---\n")
    if first_sep != -1:
        note = "\n\n> This guide targets the current `v.01` development surface. Follow the root Quick Start for matching source dependencies and `AgentBevyMcpServer` integration before configuring a local-model client."
        local = local[:first_sep] + note + local[first_sep:]
write("docs/guides/local-llms.md", local)


# ---------------------------------------------------------------------------
# Self-checks: fail the workflow instead of publishing contradictory docs.
# ---------------------------------------------------------------------------
assert "61 MCP tools" not in read("CHANGELOG.md")
assert "cargo tools" not in read("CHANGELOG.md").lower()
assert "BevyMcpServer::new" not in read("QUICKSTART.md")
assert "AgentBevyMcpServer::new" in read("QUICKSTART.md")
assert "BUILD_NOT_AVAILABLE" in read("QUICKSTART.md")
assert "docs/agent-adapter.md" in read("README.md")
for path in guide_paths:
    text = read(path)
    assert "BevyMcpServer::new" not in text, path
    assert "AgentBevyMcpServer::new" in text, path
    assert "bevy-mcp-core" in text and "bevy-mcp-server" in text, path
    assert "[calls cargo_" not in text, path

print("documentation reconciliation checks passed")
