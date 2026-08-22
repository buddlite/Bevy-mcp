# Contributing to bevy-mcp

Thanks for your interest in contributing! bevy-mcp gives AI agents real power over Bevy games, and every improvement — from bug fixes to new tools to better docs — makes that story stronger.

---

## Development Setup

1. **Clone the repo:**

   ```bash
   git clone https://github.com/buddlite/Bevy-mcp.git
   cd Bevy-mcp
   ```

2. **Install Rust** (stable channel). If you use [rustup](https://rustup.rs/), it handles everything:

   ```bash
   rustup show
   ```

3. **Build the workspace:**

   ```bash
   cargo build
   ```

4. **Run the tests:**

   ```bash
   cargo test
   ```

That's it — no external services, no API keys, no special tooling. If `cargo build` and `cargo test` pass, you're good.

---

## Project Structure

```
bevy-mcp/
├── crates/
│   ├── bevy-mcp-core/     # Shared protocol types, queue definitions (no Bevy dep)
│   ├── bevy-mcp-server/   # MCP server over stdio, tool routing (no Bevy dep)
│   └── bevy-mcp-host/     # Bevy plugin — bridges MCP into ECS
├── examples/
│   └── e2e/               # End-to-end integration examples
└── Cargo.toml             # Workspace root
```

- **`bevy-mcp-core`** is the foundation — protocol types and shared queues. It has no Bevy dependency, so external tooling can depend on it freely.
- **`bevy-mcp-server`** handles MCP JSON-RPC over stdio and dispatches to tool handlers. Also Bevy-free.
- **`bevy-mcp-host`** is the Bevy plugin that reads from the ingress queue, executes deferred commands, and writes results back through reflection.
- **`examples/e2e`** contains runnable examples for testing the full pipeline.

When adding a new tool, you'll typically touch `bevy-mcp-core` (types), `bevy-mcp-server` (handler), and `bevy-mcp-host` (ECS logic).

---

## Code Style

We follow standard Rust conventions:

- **Formatting:** `cargo fmt` — run it before committing. We use the default rustfmt settings.
- **Linting:** `cargo clippy -- -D warnings` — fix all warnings before submitting.
- **Naming:** Standard Rust naming (`snake_case` for functions/variables, `CamelCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants).

No surprises. If rustfmt and clippy are happy, the style is fine.

---

## Testing

- **Unit tests:** `cargo test` from the workspace root. Every crate has its own tests.
- **Integration tests:** Run the examples under `examples/` against a real or mock MCP client.
- **E2E validation:** The `examples/e2e` crate exercises the full server ↔ host pipeline. Run it manually when touching protocol or queue code.

When adding a new tool, include at least one test covering the handler logic. If the tool interacts with ECS, test through the queue interface in `bevy-mcp-core`.

---

## Pull Request Process

1. **Fork the repo** and create a branch from `main`:

   ```bash
   git checkout -b feat/my-new-tool
   ```

2. **Keep commits focused.** One logical change per commit. Write clear commit messages:
   - `feat: add camera_frame_entity tool`
   - `fix: handle missing entity in component_update`
   - `docs: clarify permission system in README`

3. **Run the checklist before pushing:**

   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```

4. **Open a PR** against `main` with a clear description of what changes and why. Link any related issues.

5. **Respond to review feedback.** We'll aim to review within a few days. Small, focused PRs land faster.

---

## Reporting Issues

Found a bug or have a feature request? [Open an issue](https://github.com/buddlite/Bevy-mcp/issues/new) with:

- **Bug reports:** Steps to reproduce, expected vs. actual behavior, Bevy version, and `bevy-mcp` version.
- **Feature requests:** What the feature does, why it's useful, and any API ideas you have.

If you're unsure whether something is a bug or a design question, open an issue anyway — we'd rather hear from you.

## License

By contributing, you agree that your contributions will be dual-licensed under the [MIT License](LICENSE-MIT) and [Apache License 2.0](LICENSE-APACHE), consistent with the rest of the project.
