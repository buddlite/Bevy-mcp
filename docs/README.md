# bevy-mcp Documentation

Setup guides, API reference, and workflows for using bevy-mcp with AI agents.

> `v.01` is the active development branch and may be ahead of published crates. Follow the root Quick Start for matching dependency instructions.

---

## Agent Setup Guides

Step-by-step instructions for connecting bevy-mcp to your preferred AI agent. Each guide is self-contained — pick the one you use and follow it.

| Agent | Type | Config File |
|---|---|---|
| [Claude Code](guides/claude-code.md) | CLI | `.mcp.json` |
| [Claude Desktop](guides/claude-desktop.md) | Desktop app | `claude_desktop_config.json` |
| [Cursor](guides/cursor.md) | IDE | `.cursor/mcp.json` |
| [Codex CLI](guides/codex-cli.md) | CLI | `~/.codex/config.toml` |
| [Gemini CLI](guides/gemini-cli.md) | CLI | `settings.json` |
| [Cline](guides/cline.md) | VS Code extension | `.cline/mcp.json` |
| [Local LLMs (Ollama / LM Studio)](guides/local-llms.md) | Local | Varies |

---

## What You Need (All Agents)

1. **A Bevy project** with `bevy-mcp-host` added as a dependency
2. **The `BevyMcpPlugin`** registered in your app
3. **Your game binary compiled** — the binary *is* the MCP server
4. **An MCP-compatible agent** configured to launch your binary

If you haven't set up the Rust side yet, see the [Quick Start](../README.md#quick-start) in the main README.

---

## How It Works

```
Your MCP Client (Claude, Cursor, Codex, etc.)
       │
       │  launches your game binary as a subprocess
       │  communicates over stdio (JSON-RPC)
       │
       ▼
Your Game Binary
├── bevy-mcp-server  (handles MCP protocol)
├── bevy-mcp-host    (Bevy plugin — ECS bridge)
└── Your Game Logic
```

There is no separate server process. The MCP server is embedded in your game binary. The agent launches the binary and talks to it over stdin/stdout.

---

## Quick Links

- [Supervisor implementation specification](supervisor-implementation-spec.md) — approved staged design for persistent MCP, process lifecycle, Cargo execution, restart identity, and liveness semantics
- [Agent adapter checklist](agent-adapter.md) — register semantic actions, typed state, checkpoint resources, and exact system-access metadata
- [Main README](../README.md) — Overview, tools list, architecture
- [Quick Start](../QUICKSTART.md) — Minimal setup
- [Contributing](../CONTRIBUTING.md) — Development setup and guidelines
