# bevy-mcp Documentation

Setup guides, architecture notes, and agent workflows for bevy-mcp.

> `v.01` is the active unreleased development branch and may be ahead of published crates. Use matching source revisions for the tool surface documented here.

## Execution modes

### Supervised mode — recommended for autonomous development

A persistent `bevy-mcp` process owns the MCP session, Cargo operations, managed game lifecycle, restart identity, and startup/crash evidence. The Bevy game contains `BevyMcpPlugin` plus the supervisor bridge and may be rebuilt/replaced without disconnecting the coding agent.

Start here:

- [Quick Start](../QUICKSTART.md)
- [Supervised mode and autonomous rebuild/restart](supervised-mode.md)
- `development_status` for the compact current diagnosis
- `capabilities` for the complete live contract

### Embedded mode — supported for runtime-only workflows

The MCP stdio server runs alongside the Bevy host in the instrumented game process. This is useful when the client only needs to inspect/control a running game and process replacement is handled externally.

The client guides below currently document embedded mode and link back to supervised mode when rebuild/restart continuity is required.

## Embedded client setup guides

| Agent | Type | Config File |
|---|---|---|
| [Claude Code](guides/claude-code.md) | CLI | `.mcp.json` |
| [Claude Desktop](guides/claude-desktop.md) | Desktop app | `claude_desktop_config.json` |
| [Cursor](guides/cursor.md) | IDE | `.cursor/mcp.json` |
| [Codex CLI](guides/codex-cli.md) | CLI | `~/.codex/config.toml` |
| [Gemini CLI](guides/gemini-cli.md) | CLI | `settings.json` |
| [Cline](guides/cline.md) | VS Code extension | `.cline/mcp.json` |
| [Local LLMs (Ollama / LM Studio)](guides/local-llms.md) | Local | Varies |

## Architecture and agent workflows

- [Supervisor implementation specification](supervisor-implementation-spec.md) — architecture contract behind the persistent control plane
- [Tool capabilities](tool-capabilities.md) — capability-oriented tool reference
- [Agent adapter checklist](agent-adapter.md) — semantic actions, typed state, checkpoint resources, and system-access metadata
- [Agent interaction](agent-interaction.md) — native pointer/UI/camera interaction
- [Agent debugger](agent-debugger.md) — runtime debugging surfaces
- [Debugging intelligence](debugging-intelligence.md) — causal/change-tracking workflows

## Repository links

- [Main README](../README.md) — overview and capability summary
- [Quick Start](../QUICKSTART.md) — recommended supervised setup plus embedded alternative
- [Contributing](../CONTRIBUTING.md) — development setup and quality gates
