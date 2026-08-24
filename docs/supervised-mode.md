# Supervised mode and autonomous rebuild/restart

Supervised mode keeps the MCP control plane alive while the Bevy game process is rebuilt, stopped, replaced, or crashes. Use it when a coding agent needs one durable MCP session across the full Rust edit/compile/run/debug loop.

## Architecture

```text
MCP client / coding agent
        |
        | stdio / MCP
        v
persistent bevy-mcp supervisor
  |        |          |
  |        |          +-- Cargo check/build/test
  |        +------------- process ownership + stdout/stderr evidence
  +---------------------- authenticated supervisor transport
                            |
                            v
                    Bevy game process
                    BevyMcpPlugin + bridge
```

The Bevy host still owns ECS inspection, reflection, mutation, input, assertions, playtests, captures, and runtime instrumentation. The supervisor owns operations that must survive game-process replacement: Cargo execution, managed launch/stop/restart, reconnect identity, startup readiness, and the composite `rebuild_restart` cycle.

Embedded mode remains available when process replacement is not required.

## 1. Instrument the game

Add the Bevy MCP host to the game and enable the supervisor bridge from the environment supplied by the persistent supervisor:

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

The bridge reads `BEVY_MCP_SUPERVISOR_ADDR`, `BEVY_MCP_SUPERVISOR_TOKEN`, and `BEVY_MCP_INSTANCE_ID`. Do not hard-code these values. The supervisor rotates process identity when it launches a replacement game.

Host permissions and supervisor permissions are separate boundaries. `McpPermissions` controls what the connected agent may do inside the live Bevy world. Supervisor flags control trusted Cargo execution and operating-system process lifecycle.

## 2. Build the supervisor

From a bevy-mcp checkout:

```bash
cargo build -p bevy-mcp-supervisor --bin bevy-mcp
```

Use the resulting `bevy-mcp` executable as the MCP server command. The coding agent should connect to the supervisor, not directly to the game binary.

## 3. Configure the MCP client

A minimal configuration points the supervisor at the game project:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/bevy-mcp",
      "args": [
        "--project-dir",
        "/absolute/path/to/my-bevy-game"
      ]
    }
  }
}
```

Run the supervisor with the game project as its working directory when the game expects relative asset/config paths. Alternatively, configure an explicit managed launch template:

```json
{
  "mcpServers": {
    "bevy": {
      "command": "/absolute/path/to/bevy-mcp",
      "args": [
        "--project-dir",
        "/absolute/path/to/my-bevy-game",
        "--game-executable",
        "/absolute/path/to/an/existing/game-binary",
        "--game-cwd",
        "/absolute/path/to/my-bevy-game",
        "--game-arg",
        "--some-game-argument"
      ]
    }
  }
}
```

`--game-executable` is useful for direct `process_launch` / `process_restart` and also supplies args/current-directory/environment as the launch template for a later Cargo-reported replacement artifact. `rebuild_restart` itself never guesses the newly built executable path.

## Cargo target discovery

At startup the supervisor runs `cargo metadata --format-version=1` against `--project-dir`.

Zero-configuration target selection is allowed only when exactly one viable binary target exists. If the workspace has multiple viable packages or binaries, pass the typed `package` and/or `bin` fields to `build_check`, `build`, `test`, or `rebuild_restart`.

Features are validated against Cargo metadata before Cargo is launched. Profiles are limited to `dev` and `release`. Arbitrary shell fragments and arbitrary extra Cargo arguments are intentionally not exposed.

Useful target-selection errors include:

- `TARGET_AMBIGUOUS`
- `TARGET_NOT_FOUND`
- `FEATURE_UNKNOWN`
- `INVALID_PROFILE`

## Recommended agent loop

Start by calling `capabilities`. In supervised mode this is a merged contract containing both the current Bevy-host capabilities and supervisor-owned build/lifecycle capabilities.

For a source-code change, the normal autonomous loop is:

```text
1. edit source
2. rebuild_restart
3. operation_status until terminal
4. inspect returned check/build/startup evidence
5. process_evidence when additional stdout/stderr context is useful
6. inspect the new live world
7. interact / assert / playtest / diagnose
8. repeat
```

`rebuild_restart` returns immediately with an operation ID such as:

```text
supervisor:rebuild_restart:<uuid>
```

Poll it using `operation_status`. `operation_cancel` can cancel the active Cargo child immediately; lifecycle-stage cancellation is observed at safe boundaries so the supervisor does not intentionally leave a half-transitioned process tree.

## Conservative rebuild/restart semantics

The composite operation deliberately favors preserving a known-good running game over aggressively rebuilding underneath it:

```text
cargo check while current game remains live
    |
    +-- fails -> operation fails; current game is untouched
    |
    v
stop current managed game
    |
    v
cargo build
    |
    +-- fails -> operation fails; game remains stopped
    |
    v
use Cargo compiler-artifact executable path
    |
    v
launch replacement process
    |
    v
authenticated bridge handshake
    |
    v
frame-processed host probe
    |
    v
ready with new instance_id + connection_id
```

The build step is intentionally after the stop step. This avoids relying on replacement of a running executable, which is especially problematic on Windows.

A successful operation verifies that a replacement process receives a new `instance_id` and a new transport `connection_id` rather than accidentally continuing a stale connection generation.

## Failure behavior and evidence

### Preflight check failure

If `cargo check` fails, the current managed game remains running. The operation returns the structured Cargo diagnostics, warning/error counts, selected target/profile/features, and bounded raw output tail from the check.

### Build failure

Once the preflight check has passed, the old game is stopped before `cargo build`. If that build fails, the supervisor leaves the game stopped. It does not silently relaunch stale code.

### Startup exit

If the replacement executable exits before the Bevy host reaches ready state, the failure is `PROCESS_EXITED_DURING_STARTUP`. Evidence includes the exit code and bounded stderr. The composite operation also records the build artifact and process-stage evidence accumulated before the failure.

### Startup timeout or unresponsive host

If the process connects but the Bevy host never passes the frame-processed readiness probe, startup fails rather than declaring the game ready merely because a socket exists. Use `process_evidence` to inspect current process state and bounded stdout/stderr.

### Crash after ready

The supervisor remains alive when its managed game crashes. `process_status` reports the crash state/exit code and `process_evidence` provides the captured output tails, allowing the agent to diagnose and rebuild without reconnecting its MCP client.

### External process ownership

A game that connects to the supervisor but was not launched by it is classified as external. Supervisor lifecycle tools never kill an externally owned process. `rebuild_restart` rejects external ownership with `PROCESS_NOT_MANAGED` rather than taking control implicitly.

## Process evidence

`process_evidence` returns:

- the latest process snapshot
- bounded stdout tail
- bounded stderr tail

`process_logs` remains available when only one stream or a larger line window is needed. Game stdout/stderr is captured separately from the supervisor's MCP stdio channel so game output cannot corrupt the MCP protocol stream.

## Capability reporting

In supervised mode, `capabilities` replaces the embedded-only build/lifecycle entries with supervisor-aware values. Important sections include:

- `build.check`
- `build.build`
- `build.test`
- `runtime.launch`
- `runtime.stop`
- `runtime.restart`
- `runtime.rebuild_restart`
- `supervisor.cargo`
- `supervisor.process`
- `supervisor.rebuild_restart`

Each capability continues to distinguish implementation, current availability, permission allowance, and the resulting `operational` value.

This matters when the game is stopped: host-only ECS/input capabilities may be unavailable while supervisor Cargo and `rebuild_restart` can still be operational.

## Supervisor permissions and trust boundary

Cargo is a code-execution boundary. A Rust build may execute project-controlled `build.rs` scripts and procedural macros. Enable supervisor Cargo permissions only for projects you trust.

Available CLI restrictions:

```text
--deny-cargo-check
--deny-cargo-build
--deny-cargo-test
--deny-process-lifecycle
```

`rebuild_restart` requires all of:

- Cargo check permission
- Cargo build permission
- process stop permission
- process launch permission

These permissions are independent of `McpPermissions::read_only()`, `write()`, or `full()` inside the Bevy host.

## Timeouts

Supervisor defaults are:

- `cargo check`: 120 seconds
- `cargo build`: 300 seconds
- `cargo test`: 300 seconds
- host readiness after launch: 20 seconds

Override them with:

```text
--check-timeout-secs
--build-timeout-secs
--test-timeout-secs
--ready-timeout-secs
```

Cargo timeout/cancellation terminates the owned Cargo process tree rather than only the group leader. Managed game stop/restart similarly owns the launched process tree.

## Common errors

| Error | Meaning |
|---|---|
| `CARGO_NOT_AVAILABLE` | `cargo` could not be launched |
| `PROJECT_METADATA_FAILED` | project discovery failed |
| `TARGET_AMBIGUOUS` | more than one viable binary target requires explicit selection |
| `TARGET_NOT_FOUND` | requested package/bin was not found |
| `FEATURE_UNKNOWN` | requested feature was not declared for the selected package |
| `CARGO_OPERATION_IN_PROGRESS` | another Cargo operation owns the executor |
| `REBUILD_RESTART_IN_PROGRESS` | another composite rebuild/restart is active |
| `SUPERVISOR_PERMISSION_DENIED` | required Cargo/process permission is disabled |
| `BUILD_FAILED` | check/build returned an unsuccessful status |
| `BUILD_TIMEOUT` | check/build exceeded its timeout |
| `BUILD_CANCELLED` | active check/build was cancelled |
| `BUILD_ARTIFACT_MISSING` | successful Cargo build did not provide a usable executable artifact |
| `PROCESS_NOT_MANAGED` | lifecycle mutation was requested for an externally owned game |
| `PROCESS_EXITED_DURING_STARTUP` | replacement process exited before host readiness |
| `PROCESS_START_TIMEOUT` | replacement process did not become host-ready in time |
| `REBUILD_IDENTITY_NOT_ROTATED` | replacement reused stale process/connection identity |

## Direct lifecycle tools

`process_launch`, `process_stop`, and `process_restart` remain useful when no rebuild is needed. Direct launch/restart uses the executable configured with `--game-executable`.

For source edits, prefer `rebuild_restart`: its build phase launches the exact executable path reported by Cargo and records one coherent operation/evidence trail across check, stop, build, launch, authentication, and readiness.
