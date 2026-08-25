# Contributing to bevy-mcp

Contributions to runtime tooling, supervisor reliability, tests, and documentation are welcome.

## Development setup

```bash
git clone https://github.com/buddlite/Bevy-mcp.git
cd Bevy-mcp
git checkout v.01
cargo build --workspace
```

The workspace requires stable Rust with a minimum supported version of Rust 1.85. Linux builds of Bevy may require the same audio/input/window development packages installed by `.github/workflows/ci.yml`.

## Project structure

```text
bevy-mcp/
├── crates/
│   ├── bevy-mcp-core/        # Shared protocol/wire types; no Bevy dependency
│   ├── bevy-mcp-host/        # Bevy plugin, ECS/debug/input/runtime integration
│   ├── bevy-mcp-server/      # MCP routers and GameCommandBackend abstraction
│   └── bevy-mcp-supervisor/  # Persistent MCP process, Cargo/process lifecycle
├── examples/
│   └── e2e/                  # Embedded end-to-end example
├── docs/                     # Setup, architecture, and workflow documentation
└── Cargo.toml                # Workspace root
```

New game-facing tools usually involve shared request/result types in `bevy-mcp-core`, routing in `bevy-mcp-server`, and execution in `bevy-mcp-host`. Supervisor-owned build/process functionality belongs in `bevy-mcp-supervisor` rather than the host.

## Required quality checks

Run the same checks enforced by CI before submitting:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

On Windows, supervisor process-lifecycle behavior is also compiled and tested by CI.

## Testing expectations

- Add focused unit tests for pure state/contract logic.
- Add integration tests for ECS-facing behavior where practical.
- Supervisor Cargo/process changes should cover failure and cancellation paths as well as success.
- Keep tests deterministic and bound timeouts/evidence; do not rely on arbitrary sleeps when a state can be observed directly.
- Update `capabilities`, onboarding docs, and the changelog when a user-visible contract changes.

## Pull request process

1. Create a focused feature branch from `v.01`.
2. Keep commits and the PR description scoped to one coherent change.
3. Run the required quality checks above.
4. Open the PR against `v.01`.
5. Treat the live capability contract and tests as source of truth; do not document planned tools as shipped.

## Reporting issues

Open an issue with reproduction steps, expected/actual behavior, Bevy version, bevy-mcp revision, execution mode (embedded or supervised), and relevant `development_status` / `capabilities` / process evidence where applicable.

## License

By contributing, you agree that your contributions are dual-licensed under the [MIT License](LICENSE-MIT) and [Apache License 2.0](LICENSE-APACHE).
