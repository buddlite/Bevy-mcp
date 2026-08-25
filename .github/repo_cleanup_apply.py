from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected cleanup anchor missing in {path}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


def insert_before(path: str, anchor: str, addition: str) -> None:
    replace_once(path, anchor, addition + anchor)


# Repository hygiene.
for stale in ("test_init.jsonl", "test_input.jsonl"):
    path = Path(stale)
    if path.exists():
        path.unlink()

replace_once(".gitignore", "/target\nCargo.lock\n", "/target\n")

# Keep the minimum supported Rust version in one place.
replace_once(
    "Cargo.toml",
    '[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n',
    '[workspace.package]\nversion = "0.1.0"\nedition = "2024"\nrust-version = "1.85"\n',
)
for manifest in (
    "crates/bevy-mcp-core/Cargo.toml",
    "crates/bevy-mcp-host/Cargo.toml",
    "crates/bevy-mcp-server/Cargo.toml",
    "crates/bevy-mcp-supervisor/Cargo.toml",
):
    replace_once(manifest, 'rust-version = "1.85"', "rust-version.workspace = true")
replace_once(
    "examples/e2e/Cargo.toml",
    "edition.workspace = true\npublish = false",
    "edition.workspace = true\nrust-version.workspace = true\npublish = false",
)

# Stage-numbered test names aged badly once later stages started extending them.
old_acceptance = Path("crates/bevy-mcp-supervisor/src/stage4_acceptance.rs")
new_acceptance = Path("crates/bevy-mcp-supervisor/src/supervisor_acceptance.rs")
if old_acceptance.exists():
    text = old_acceptance.read_text(encoding="utf-8").replace("stage4", "supervisor")
    new_acceptance.write_text(text, encoding="utf-8")
    old_acceptance.unlink()
replace_once(
    "crates/bevy-mcp-supervisor/src/lib.rs",
    "mod stage4_acceptance;",
    "mod supervisor_acceptance;",
)

# Bound retained Cargo operation history. Operation IDs stay queryable while retained,
# and only the oldest terminal records are evicted.
cargo_path = "crates/bevy-mcp-supervisor/src/cargo_executor.rs"
replace_once(
    cargo_path,
    "use crate::permissions::SupervisorPermissions;\n",
    "use crate::permissions::SupervisorPermissions;\n\nconst CARGO_OPERATION_HISTORY_LIMIT: usize = 256;\n",
)
replace_once(
    cargo_path,
    '''                Err(error) => {
                    self.finish_failure(
                        &operation_id,
                        &prepared,
                        started.elapsed(),
                        None,
                        output.clone(),
                        prepared.kind.failure_code(),
                        error,
                    );
                    self.clear_active(&operation_id);
                    return;
                }
''',
    '''                Err(error) => {
                    self.finish_failure(
                        &operation_id,
                        &prepared,
                        started.elapsed(),
                        None,
                        output.clone(),
                        prepared.kind.failure_code(),
                        error,
                    );
                    return;
                }
''',
)
replace_once(
    cargo_path,
    '''    fn clear_active(&self, operation_id: &str) {
        let mut active = self.inner.active_operation.lock().unwrap();
        if active.as_deref() == Some(operation_id) {
            *active = None;
        }
    }
}
''',
    '''    fn clear_active(&self, operation_id: &str) {
        {
            let mut active = self.inner.active_operation.lock().unwrap();
            if active.as_deref() == Some(operation_id) {
                *active = None;
            }
        }
        let mut operations = self.inner.operations.lock().unwrap();
        prune_cargo_history(&mut operations, CARGO_OPERATION_HISTORY_LIMIT);
    }
}

fn prune_cargo_history(
    operations: &mut HashMap<String, CargoOperationRecord>,
    limit: usize,
) {
    let limit = limit.max(1);
    if operations.len() <= limit {
        return;
    }
    let mut terminal: Vec<_> = operations
        .iter()
        .filter(|(_, record)| record.snapshot.state.is_terminal())
        .map(|(id, record)| (id.clone(), record.snapshot.created_unix_ms))
        .collect();
    terminal.sort_by_key(|(_, created)| *created);
    let remove_count = operations.len().saturating_sub(limit);
    for (id, _) in terminal.into_iter().take(remove_count) {
        operations.remove(&id);
    }
}
''',
)
# Pure unit coverage avoids running hundreds of Cargo subprocesses.
with Path(cargo_path).open("a", encoding="utf-8") as file:
    file.write(r'''

#[cfg(test)]
mod history_tests {
    use super::*;

    fn record(id: &str, created: u128, state: CargoOperationState) -> CargoOperationRecord {
        CargoOperationRecord {
            snapshot: CargoOperationSnapshot {
                operation_id: id.to_string(),
                kind: CargoOperationKind::Check,
                state,
                created_unix_ms: created,
                started_unix_ms: Some(created),
                finished_unix_ms: state.is_terminal().then_some(created + 1),
                invocation: CargoInvocation::new(None, None, None, None, None),
                result: None,
                failure: None,
            },
            cancel_requested: false,
        }
    }

    #[test]
    fn cargo_history_prunes_oldest_terminal_records_only() {
        let mut operations = HashMap::from([
            ("old".to_string(), record("old", 1, CargoOperationState::Succeeded)),
            ("new".to_string(), record("new", 2, CargoOperationState::Failed)),
            ("active".to_string(), record("active", 3, CargoOperationState::Running)),
        ]);
        prune_cargo_history(&mut operations, 2);
        assert!(!operations.contains_key("old"));
        assert!(operations.contains_key("new"));
        assert!(operations.contains_key("active"));
    }
}
''')

# Bound composite rebuild/restart history as well.
rebuild_path = "crates/bevy-mcp-supervisor/src/rebuild_restart.rs"
replace_once(
    rebuild_path,
    "use crate::process_manager::{\n",
    "const REBUILD_OPERATION_HISTORY_LIMIT: usize = 128;\n\nuse crate::process_manager::{\n",
)
replace_once(
    rebuild_path,
    '''    fn clear_active(&self, operation_id: &str) {
        let mut active = self.inner.active_operation.lock().unwrap();
        if active.as_deref() == Some(operation_id) {
            *active = None;
        }
    }
}
''',
    '''    fn clear_active(&self, operation_id: &str) {
        {
            let mut active = self.inner.active_operation.lock().unwrap();
            if active.as_deref() == Some(operation_id) {
                *active = None;
            }
        }
        let mut operations = self.inner.operations.lock().unwrap();
        prune_rebuild_history(&mut operations, REBUILD_OPERATION_HISTORY_LIMIT);
    }
}

fn prune_rebuild_history(
    operations: &mut HashMap<String, RebuildRestartRecord>,
    limit: usize,
) {
    let limit = limit.max(1);
    if operations.len() <= limit {
        return;
    }
    let mut terminal: Vec<_> = operations
        .iter()
        .filter(|(_, record)| record.snapshot.state.is_terminal())
        .map(|(id, record)| (id.clone(), record.snapshot.created_unix_ms))
        .collect();
    terminal.sort_by_key(|(_, created)| *created);
    let remove_count = operations.len().saturating_sub(limit);
    for (id, _) in terminal.into_iter().take(remove_count) {
        operations.remove(&id);
    }
}
''',
)
with Path(rebuild_path).open("a", encoding="utf-8") as file:
    file.write(r'''

#[cfg(test)]
mod history_tests {
    use super::*;

    fn record(id: &str, created: u128, state: RebuildRestartState) -> RebuildRestartRecord {
        RebuildRestartRecord {
            snapshot: RebuildRestartSnapshot {
                operation_id: id.to_string(),
                state,
                created_unix_ms: created,
                started_unix_ms: Some(created),
                finished_unix_ms: state.is_terminal().then_some(created + 1),
                invocation: CargoInvocation::new(None, None, None, None, None),
                evidence: RebuildRestartEvidence::default(),
                failure: None,
            },
            cancel_requested: false,
            current_cargo_operation_id: None,
        }
    }

    #[test]
    fn rebuild_history_prunes_oldest_terminal_records_only() {
        let mut operations = HashMap::from([
            ("old".to_string(), record("old", 1, RebuildRestartState::Succeeded)),
            ("new".to_string(), record("new", 2, RebuildRestartState::Failed)),
            ("active".to_string(), record("active", 3, RebuildRestartState::Building)),
        ]);
        prune_rebuild_history(&mut operations, 2);
        assert!(!operations.contains_key("old"));
        assert!(operations.contains_key("new"));
        assert!(operations.contains_key("active"));
    }
}
''')

# Make development_status supersession failure-specific. A successful check should
# clear an old compile failure, but must not hide a still-unresolved test failure.
status_path = "crates/bevy-mcp-supervisor/src/development_status.rs"
replace_once(
    status_path,
    '''    let generation = generation(&process, &cargo_operations, &rebuild_operations);
    let latest_success = latest_success_unix_ms(&process, &cargo_operations, &rebuild_operations);
    let current_failure = last_failure
        .as_ref()
        .filter(|failure| failure.occurred_unix_ms.unwrap_or_default() > latest_success);
''',
    '''    let generation = generation(&process, &cargo_operations, &rebuild_operations);
    let current_failure = last_failure.as_ref().filter(|failure| {
        failure_is_current(failure, &process, &cargo_operations, &rebuild_operations)
    });
''',
)
replace_once(
    status_path,
    '''    [cargo_failure, rebuild_failure, process_failure]
        .into_iter()
        .flatten()
        .max_by_key(|failure| failure.occurred_unix_ms.unwrap_or_default())
''',
    '''    [cargo_failure, rebuild_failure, process_failure]
        .into_iter()
        .flatten()
        .max_by_key(|failure| {
            (
                failure.occurred_unix_ms.unwrap_or_default(),
                failure_source_priority(&failure.source),
            )
        })
''',
)
replace_once(
    status_path,
    '''fn latest_success_unix_ms(
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> u128 {
    let cargo_success = cargo
        .iter()
        .filter(|operation| operation.state == CargoOperationState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();
    let rebuild_success = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();
    let ready_process = (process.state == ProcessState::Running && process.host == "ready")
        .then_some(process.started_unix_ms.unwrap_or_default())
        .unwrap_or_default();
    cargo_success.max(rebuild_success).max(ready_process)
}
''',
    '''fn failure_source_priority(source: &str) -> u8 {
    match source {
        "rebuild_restart" => 2,
        "cargo" => 1,
        _ => 0,
    }
}

fn failure_is_current(
    failure: &DevelopmentFailure,
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> bool {
    let occurred = failure.occurred_unix_ms.unwrap_or_default();
    let latest_rebuild_success = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();

    if failure.code.starts_with("TEST_") || failure.stage.as_deref() == Some("test") {
        let latest_test_success = cargo
            .iter()
            .filter(|operation| {
                operation.kind == CargoOperationKind::Test
                    && operation.state == CargoOperationState::Succeeded
            })
            .filter_map(|operation| operation.finished_unix_ms)
            .max()
            .unwrap_or_default();
        return occurred > latest_test_success;
    }

    if failure.code.starts_with("BUILD_")
        || matches!(failure.stage.as_deref(), Some("check" | "build"))
    {
        let latest_compile_success = cargo
            .iter()
            .filter(|operation| {
                matches!(operation.kind, CargoOperationKind::Check | CargoOperationKind::Build)
                    && operation.state == CargoOperationState::Succeeded
            })
            .filter_map(|operation| operation.finished_unix_ms)
            .max()
            .unwrap_or_default()
            .max(latest_rebuild_success);
        return occurred > latest_compile_success;
    }

    let latest_ready_process = (process.state == ProcessState::Running && process.host == "ready")
        .then_some(process.started_unix_ms.unwrap_or_default())
        .unwrap_or_default();
    occurred > latest_ready_process.max(latest_rebuild_success)
}
''',
)
# Strengthen the startup tie regression and add test/check supersession coverage.
replace_once(
    status_path,
    '''        let status = compose_status(
            process(ProcessState::Crashed, "waiting"),
            true,
''',
    '''        let mut crashed = process(ProcessState::Crashed, "waiting");
        crashed.exited_unix_ms = Some(450);
        let status = compose_status(
            crashed,
            true,
''',
)
insert_before(
    status_path,
    '''    #[test]
    fn active_rebuild_takes_precedence_over_old_failure() {
''',
    r'''    #[test]
    fn successful_check_does_not_hide_unresolved_test_failure() {
        let mut failed_test = failed_check();
        failed_test.operation_id = "supervisor:test:1".to_string();
        failed_test.kind = CargoOperationKind::Test;
        failed_test.failure.as_mut().unwrap().code = "TEST_FAILED".to_string();
        failed_test.failure.as_mut().unwrap().message = "Cargo test exited unsuccessfully".to_string();

        let mut successful_check = failed_check();
        successful_check.operation_id = "supervisor:check:2".to_string();
        successful_check.state = CargoOperationState::Succeeded;
        successful_check.created_unix_ms = 230;
        successful_check.started_unix_ms = Some(231);
        successful_check.finished_unix_ms = Some(240);
        successful_check.failure = None;
        successful_check.result.as_mut().unwrap().success = true;
        successful_check.result.as_mut().unwrap().error_count = 0;
        successful_check.result.as_mut().unwrap().diagnostics.clear();

        let status = compose_status(
            process(ProcessState::Running, "ready"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            vec![failed_test, successful_check],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(status.state, DevelopmentState::TestFailed);
        assert_eq!(status.recovery.action, "fix_failing_tests");
    }

''',
)

# CI should enforce the same quality bar documented for contributors.
Path(".github/workflows/ci.yml").write_text(
    '''name: CI

on:
  push:
  pull_request:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  rust-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Install Bevy Linux dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \\
            libasound2-dev \\
            libudev-dev \\
            libwayland-dev \\
            libxkbcommon-dev
      - name: Verify formatting
        run: cargo fmt --all -- --check
      - name: Check workspace
        run: cargo check --workspace --all-targets
      - name: Clippy workspace
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test workspace
        run: cargo test --workspace

  process-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - name: Verify formatting
        run: cargo fmt --all -- --check
      - name: Check supervisor on Windows
        run: cargo check -p bevy-mcp-supervisor --all-targets
      - name: Test supervisor process lifecycle on Windows
        run: cargo test -p bevy-mcp-supervisor --lib
''',
    encoding="utf-8",
)

# Reconcile the high-level onboarding around the now-recommended supervised path.
Path("QUICKSTART.md").write_text(r'''# Quick Start

Get the current `v.01` development build of bevy-mcp connected to a Bevy 0.19 game.

> `v.01` is an unreleased development branch and may be ahead of crates.io. Keep the bevy-mcp crates/binary on one matching source revision.

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

### 2. Build the persistent supervisor

From the bevy-mcp checkout:

```bash
cargo build -p bevy-mcp-supervisor --bin bevy-mcp
```

### 3. Point the MCP client at the supervisor

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/Bevy-mcp/target/debug/bevy-mcp",
      "args": [
        "--project-dir",
        "/absolute/path/to/your-bevy-game"
      ]
    }
  }
}
```

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
''', encoding="utf-8")

Path("docs/README.md").write_text(r'''# bevy-mcp Documentation

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
''', encoding="utf-8")

Path("CONTRIBUTING.md").write_text(r'''# Contributing to bevy-mcp

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
''', encoding="utf-8")

# Label every existing client guide accurately instead of silently presenting embedded mode
# as the only architecture.
mode_note = (
    "> **Mode note:** This guide documents **embedded mode**, where the MCP client launches "
    "the instrumented game binary directly. For autonomous Rust edit/build/restart workflows, "
    "use the persistent [supervised mode](../supervised-mode.md), which keeps the MCP session "
    "alive across game rebuilds and crashes.\n\n"
)
for guide in (
    "docs/guides/claude-code.md",
    "docs/guides/claude-desktop.md",
    "docs/guides/cline.md",
    "docs/guides/codex-cli.md",
    "docs/guides/cursor.md",
    "docs/guides/gemini-cli.md",
    "docs/guides/local-llms.md",
):
    path = Path(guide)
    text = path.read_text(encoding="utf-8")
    marker = "\n---\n"
    if marker not in text:
        raise SystemExit(f"guide separator missing in {guide}")
    path.write_text(text.replace(marker, "\n" + mode_note + "---\n", 1), encoding="utf-8")

Path("config/README.md").write_text(r'''# MCP client configuration samples

The JSON files in this directory use the persistent `bevy-mcp` **supervisor** command.

For a project-local MCP configuration, run the client from the game project directory or add an explicit project path:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/bevy-mcp",
      "args": ["--project-dir", "/absolute/path/to/my-bevy-game"]
    }
  }
}
```

The supervisor can discover a single Cargo binary automatically. Multi-binary workspaces should provide `package` and/or `bin` to build/rebuild tools. See [supervised mode](../docs/supervised-mode.md) for the complete contract.

The client guides under `docs/guides/` document the alternative embedded mode.
''', encoding="utf-8")

# Patch stale front-page wording while preserving the detailed capability reference.
replace_once(
    "README.md",
    "## Quick Start\n\n### 1. Add matching dependencies",
    "## Quick Start\n\n> **For autonomous coding, start with the [Quick Start guide](QUICKSTART.md) and supervised mode.** The inline example below intentionally shows the simpler embedded mode.\n\n### 1. Add matching dependencies",
)
replace_once(
    "README.md",
    "## How it works\n\n1. **Your game embeds `BevyMcpPlugin`.**",
    "## How it works\n\nBoth execution modes share the same Bevy host and tool model. The sequence below describes embedded mode; supervised mode moves MCP stdio, Cargo execution, and process lifecycle into the persistent `bevy-mcp` process while the game connects through the authenticated bridge.\n\n### Embedded mode\n\n1. **Your game embeds `BevyMcpPlugin`.**",
)
replace_once(
    "README.md",
    "## Architecture\n\n```text",
    "## Architecture\n\n### Embedded mode\n\n```text",
)
replace_once(
    "README.md",
    "The workspace is split into four crates:",
    '''### Supervised mode

```text
MCP Client / AI Agent
        | stdio / MCP
        v
persistent bevy-mcp supervisor
   | Cargo + process lifecycle
   | authenticated bridge
   v
BevyMcpPlugin -> Bevy ECS World
```

The workspace is split into four crates:''',
)
replace_once(
    "README.md",
    "bevy-mcp uses standard MCP stdio transport, so any compatible client can launch the instrumented game binary. Repository guides are available for common clients:",
    "bevy-mcp uses standard MCP stdio transport. For autonomous coding, the client should launch the persistent `bevy-mcp` supervisor; the current client-specific guides document the alternative embedded mode where the client launches the instrumented game binary directly:",
)
replace_once(
    "README.md",
    "No. The MCP client is external, but the Bevy-facing host runs inside the game process. The stdio server forwards requests to the host through shared in-process queues.",
    "No. The Bevy-facing host always runs inside the game process. In embedded mode the stdio MCP server shares that process; in supervised mode a persistent external `bevy-mcp` process owns MCP stdio and forwards game commands across the authenticated supervisor bridge.",
)
replace_once(
    "README.md",
    "Not from the embedded MCP server today. The build/check/test tools intentionally return `BUILD_NOT_AVAILABLE`; run those commands from a trusted development shell or coding harness.",
    "Yes in supervised mode: the persistent supervisor owns trusted `build_check`, `build`, `test`, and `rebuild_restart` operations. Embedded mode deliberately keeps Cargo and OS process lifecycle external and reports `BUILD_NOT_AVAILABLE` for those build tools.",
)

replace_once(
    "docs/supervisor-implementation-spec.md",
    "Status: design approved for staged implementation. This document defines the contract for the persistent `bevy-mcp` supervisor before Stage 1 code is written.",
    "Status: implemented architecture contract. Supervisor Stages 1–4 are merged; later agent-oriented diagnostics extend this design. Use `supervised-mode.md` and the live `capabilities` / `development_status` responses for current operational behavior.",
)

# Changelog the cleanup itself.
insert_before(
    "CHANGELOG.md",
    "### Current limitations\n",
    '''- Repository CI now enforces rustfmt and Clippy in addition to cross-platform compile/test coverage.
- Supervisor Cargo and rebuild/restart operation histories are bounded by evicting the oldest terminal records.
- Onboarding and contributor documentation now distinguishes supervised and embedded execution modes consistently.
- Stage-numbered supervisor acceptance tests were renamed to reflect their continuing cross-stage role.

''',
)
