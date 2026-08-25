# Supervisor Architecture and Implementation Specification

Status: implemented architecture contract. Supervisor Stages 1–4 are merged; later agent-oriented diagnostics extend this design. Use `supervised-mode.md` and the live `capabilities` / `development_status` responses for current operational behavior.

## Purpose

The supervisor turns bevy-mcp from an in-process game control surface into a persistent development control plane that survives game rebuilds, restarts, hangs, and crashes.

The target autonomous loop is:

```text
edit code
  -> cargo check
  -> build
  -> restart game
  -> wait for host readiness
  -> inspect runtime
  -> playtest
  -> assert
  -> diagnose
  -> fix
```

The MCP client must remain connected to the same long-lived supervisor throughout that loop.

## Non-goals for v1

- Arbitrary shell execution.
- Multi-game or multi-project supervision from one supervisor process.
- Zero-downtime executable swapping.
- Hot reload of Rust code.
- Remote-network supervision.
- TLS or Internet-facing IPC.
- Generic process execution outside the selected Cargo project and managed game target.
- Perfect structured output for the Rust test harness when Cargo itself only exposes text.

## Core architecture

```text
MCP client
    |
    | stdio / MCP JSON-RPC
    v
+-----------------------------+
| bevy-mcp supervisor         |
| persistent process          |
|                             |
| MCP server/router           |
| supervisor tool surface     |
| GameCommandBackend          |
| Cargo executor              |
| process manager             |
| operation registry          |
| IPC listener                |
+-------------+---------------+
              |
              | authenticated loopback TCP
              | versioned length-prefixed JSON
              v
+-----------------------------+
| Bevy game process           |
| disposable / restartable    |
|                             |
| BevyMcpPlugin               |
| supervisor bridge           |
| ECS/debugger/input/assert   |
+-----------------------------+
```

The supervisor owns the MCP connection. The game process is disposable.

## Crate ownership

| Crate | Responsibility |
|---|---|
| `bevy-mcp-core` | command/result types, entity handles, wire-safe shared protocol types |
| `bevy-mcp-host` | Bevy ECS access, debugger, assertions, checkpoints, input, game-side supervisor bridge |
| `bevy-mcp-server` | MCP tool definitions/routing and game-command backend abstraction |
| `bevy-mcp-supervisor` | persistent binary, IPC server, process lifecycle, Cargo execution, supervisor permissions, merged capabilities |

`bevy-mcp-host` must not own Cargo/process permissions or external process lifecycle.

## Hard invariants

1. The outer MCP connection survives game stop, rebuild, restart, crash, and reconnect.
2. Only one active game instance is supported per supervisor in v1.
3. Every launched game process has a unique `instance_id`.
4. Every accepted IPC connection has a unique `connection_id`.
5. Entity handles are scoped to `instance_id`, not merely Bevy entity generation.
6. Pending game requests are scoped to `connection_id`.
7. No response from an old connection may satisfy a request from a later connection.
8. Disconnecting a game fails all requests belonging to that connection immediately.
9. `Hello`/socket connectivity is not sufficient for host readiness.
10. Host readiness requires a probe processed through the normal Bevy frame/ingress path.
11. An IPC task that is alive while the Bevy main thread is blocked must not make the host look healthy.
12. Supervisor Cargo/process authority is separate from `McpPermissions` inside the game.
13. Cargo and game processes are invoked directly through OS process APIs, never through a shell.
14. Game stdout/stderr never share supervisor MCP stdout.
15. Embedded mode remains supported and passes its existing tests.
16. Public capability reporting distinguishes implemented, currently available, permission-allowed, and operational state.

## Identity model

Two identities are required.

### `instance_id`

Identifies one launched Bevy process incarnation.

Example:

```text
run-f6815c4a
```

A new value is generated for every process launch, including restart.

It scopes:

- entity handles;
- process lifecycle/evidence;
- game connection expectation;
- game-local debugger data where practical.

### `connection_id`

Identifies one accepted IPC transport connection.

Example:

```text
conn-8d44b132
```

A reconnect from the same process keeps the same `instance_id` but receives a new `connection_id`.

It scopes:

- pending request correlation;
- response routing;
- transport generation;
- cancellation of in-flight calls on disconnect.

### Entity handles

Current handles have the form:

```text
entity://<instance>/<world>/<id>/<generation>
```

The host must stop hardcoding `default` as the instance namespace. In supervised mode the instance segment is the active `instance_id`.

Resolution must become typed rather than `Option`-only so a stale process can be distinguished from a missing entity.

Required errors:

```text
STALE_INSTANCE
ENTITY_NOT_FOUND
INVALID_WORLD
INVALID_ENTITY_HANDLE
```

Example stale error:

```json
{
  "error": "STALE_INSTANCE",
  "handle_instance": "run-old",
  "current_instance": "run-new"
}
```

An agent receiving `STALE_INSTANCE` should discard cached entity handles and reacquire world context. `ENTITY_NOT_FOUND` means the handle belongs to the current instance but no longer resolves.

## Game command backend

All base, advanced, and debugger/playtest MCP surfaces must call the same backend abstraction. No tool surface may retain direct ownership of `McpResponseDispatcher` after Stage 1.

Conceptual contract:

```rust
pub trait GameCommandBackend: Send + Sync {
    fn call(
        &self,
        command: McpCommand,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<McpResult, GameCallError>>;
}
```

Implementations:

```text
EmbeddedBackend
  -> existing in-process ingress/result queues and dispatcher

SupervisorBackend
  -> current authenticated IPC connection
```

The exact async-trait mechanism is an implementation detail. The trait must be object-safe because server surfaces share `Arc<dyn GameCommandBackend>`.

### Backend errors

At minimum:

```text
GAME_UNAVAILABLE
GAME_DISCONNECTED
GAME_UNRESPONSIVE
REQUEST_TIMEOUT
CONNECTION_REPLACED
PROTOCOL_ERROR
```

Tool formatting may preserve the current JSON-string API initially, but backend state and failure routing must be typed internally.

## IPC transport

### Network scope

v1 binds only to loopback TCP. It must never bind to wildcard/external interfaces by default.

The supervisor generates:

- a random loopback port;
- a cryptographically random authentication token;
- the expected `instance_id`.

Managed games receive these values through environment variables such as:

```text
BEVY_MCP_SUPERVISOR_ADDR=127.0.0.1:49152
BEVY_MCP_SUPERVISOR_TOKEN=<random-secret>
BEVY_MCP_INSTANCE_ID=run-f6815c4a
```

### Framing

Use versioned, length-prefixed JSON.

Recommended v1 frame:

```text
[u32 big-endian payload length][UTF-8 JSON payload]
```

Set an explicit maximum frame size. Initial default: 32 MiB. Oversized frames are rejected before allocation beyond the configured bound.

Large future artifacts should prefer file references rather than continually increasing the frame limit.

### Envelope

Conceptual shape:

```rust
struct WireEnvelope {
    protocol_version: u32,
    connection_id: Option<String>,
    message: WireMessage,
}

#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum WireMessage {
    Hello(Hello),
    HelloAccepted(HelloAccepted),
    Command(WireCommand),
    Response(WireResponse),
    TransportPing { nonce: u64 },
    TransportPong { nonce: u64 },
    Shutdown(ShutdownRequest),
}
```

Do not use Serde's implicit/default enum representation as the public wire contract.

### Handshake

`Hello` must include enough information to reject the wrong process or incompatible host before accepting commands:

```text
protocol version
secret token
instance_id
host/bevy-mcp version metadata
optional PID for diagnostics only
```

Supervisor validation order:

```text
accept loopback socket
 -> parse bounded Hello
 -> validate protocol version
 -> validate token
 -> validate expected instance_id
 -> ensure no conflicting active connection
 -> allocate connection_id
 -> send HelloAccepted
```

If the same instance reconnects after transport loss, it receives a new `connection_id`.

A second simultaneous game connection is rejected in v1.

### Connection-generation correctness

Each pending request record stores the `connection_id` on which it was sent.

On disconnect:

```text
- mark transport disconnected;
- atomically detach the old connection;
- fail every request for that connection with GAME_DISCONNECTED;
- never carry pending IDs into the next connection.
```

When a response arrives, both request correlation and connection generation must match before resolution.

Unknown/late responses are discarded and logged as protocol diagnostics; they must not be routed to another request.

## Liveness and readiness

Liveness has three independent dimensions.

### Process state

```text
stopped
starting
running
stopping
exited
crashed
```

### Transport state

```text
disconnected
connecting
connected
```

### Host state

```text
waiting
ready
unresponsive
```

`unresponsive` is not an OS process state.

### Transport heartbeat

Transport ping/pong is handled by the IPC task, independently of Bevy schedules. It proves only:

- the process/IPC task has not disappeared;
- the socket path still functions.

It does not prove that ECS commands can execute.

### Frame-aware host probe

The supervisor must have an internal host probe that traverses the normal game-command ingress and is acknowledged only after Bevy processes it.

Conceptually:

```text
Supervisor sends probe
 -> IPC client receives command
 -> command enters normal Bevy MCP ingress
 -> Bevy schedule processes probe
 -> response includes probe ID, instance ID, frame number
 -> supervisor receives acknowledgement
```

The probe is deliberately starvable by a blocked/long-running Bevy main-thread system. That is the desired signal.

Do not create an out-of-band "healthy" path that can report ready while ordinary MCP/ECS calls cannot execute.

### Startup readiness

`process_launch` is successful only after:

```text
1. OS process spawned
2. IPC socket connected
3. Hello received
4. token validated
5. protocol validated
6. expected instance validated
7. connection_id assigned
8. host probe sent through normal ingress
9. host probe acknowledged
10. host marked ready
```

`spawn()` success alone is not launch success.

### Host unresponsiveness

Do not infer unresponsiveness solely from "no new frame for N seconds".

Host unresponsiveness is probe-driven:

```text
process is running
+ transport is connected
+ host probe was sent
+ no Bevy-processed acknowledgement before configured deadline
= host unresponsive
```

A frame timestamp/counter may be exposed as telemetry but is not sufficient by itself to change host state.

## Game-side supervisor bridge

The host should not require Tokio solely for supervisor IPC. Prefer a small blocking/background transport thread or another mechanism that keeps Bevy-facing integration simple and independent of the supervisor runtime implementation.

Supervised mode must be explicitly enabled by the game integration. Environment variables provide connection parameters; their presence alone should not silently grant new OS/build authority.

The bridge translates authenticated wire commands into the same core command path used by embedded mode and returns correlated responses over IPC.

## Supervisor process model

### Ownership

Process status distinguishes:

```text
managed  - launched and owned by this supervisor
external - connected manually/test harness; not lifecycle-owned
```

Process stop/restart tools must not kill an external process merely because it connected.

### Public lifecycle tools

```text
process_status
process_launch
process_stop
process_restart
process_logs
```

Existing game-time controls remain separate:

```text
runtime_pause
runtime_resume
runtime_step
runtime_time_scale
```

Existing `runtime_launch`, `runtime_stop`, and `runtime_restart` become deprecated lifecycle aliases in supervised mode rather than continuing to blur Bevy-time control with OS process control.

### Child stdio

Managed game process:

```text
stdin  = null
stdout = pipe
stderr = pipe
```

The supervisor keeps bounded stdout/stderr ring buffers. Game output must never reach supervisor MCP stdout.

### Graceful stop escalation

`process_stop` follows:

```text
send authenticated wire Shutdown
 -> wait configurable grace period
 -> terminate owned process tree
 -> wait
 -> force-kill owned process tree if necessary
```

### Process-tree ownership

Unix:

- place managed game/build operations in dedicated process groups;
- cancellation/stop targets the group, not only the direct child.

Windows:

- place managed game/build processes in Job Objects;
- use kill-on-job-close semantics or equivalent robust ownership;
- no orphaned descendant should survive normal supervisor cleanup.

The implementation must test descendants, not just direct child termination.

## Cargo executor

Cargo operations live entirely in `bevy-mcp-supervisor`.

### Discovery

At project initialization run:

```text
cargo metadata --format-version=1
```

Build an internal project model containing at least:

```text
workspace root
packages/package IDs
manifest paths
binary targets
known Cargo features
target directory
```

If exactly one viable game binary exists, zero-config selection is allowed. Otherwise return `TARGET_AMBIGUOUS` until package/bin configuration is supplied.

### Invocation

Never invoke a shell.

Use direct argument construction:

```rust
Command::new("cargo").arg("check")
```

No public generic `extra_args: Vec<String>` in v1.

Allowed v1 parameters are intentionally narrow:

```text
package
bin
profile: dev|release
features validated against metadata
test filter
```

Do not expand the allowlist merely to mirror every Cargo flag.

### Security note

`cargo check` is still trusted code execution because Cargo can compile/run build scripts and procedural macros. `check`, `build`, and `test` all require supervisor build permission.

### Diagnostics

Use Cargo machine-readable JSON output for compiler messages/artifacts.

Structured result should expose at least:

```text
success
exit_code
duration_ms
compiler diagnostics
warning/error counts
selected package/target/profile
emitted executable path when Cargo reports one
bounded raw-output tail
```

Do not guess artifact paths when Cargo emits an executable path.

For tests, preserve compiler diagnostics structurally but do not claim perfect per-test structure if the test harness output remains textual.

### Supervisor permissions

Define a supervisor-local permission model, for example:

```text
cargo_check
cargo_build
cargo_test
process_launch
process_stop
process_restart
```

It must not be stored in `bevy-mcp-host` or controlled by game ECS resources.

No arbitrary shell/process command is added.

## Long-running supervisor operations

Cargo operations must not occupy one synchronous MCP request for minutes.

Use a supervisor operation registry with opaque namespaced IDs, e.g.:

```text
supervisor:build:<uuid>
supervisor:test:<uuid>
```

Public flow:

```text
build/check/test start
 -> return operation_id
 -> operation_status
 -> operation_cancel
```

Where practical, the existing `operation_status` / `operation_cancel` MCP names should route supervisor-prefixed IDs locally and game-prefixed IDs through `GameCommandBackend`, avoiding duplicate public tools.

Initial v1 concurrency rule: one Cargo operation at a time per supervisor/project. Return a clear conflict/busy error rather than relying only on Cargo's own target lock.

Cancellation must terminate the owned Cargo process tree.

Recommended configurable defaults:

```text
check: 120 s
build: 300 s
test: 300 s
```

These are supervisor operation limits, independent of ordinary game-command timeouts.

## Capabilities

Embedded mode continues to report process/build operations unavailable.

Supervised mode merges supervisor state with live game capabilities.

Example while game is ready:

```json
{
  "mode": "supervised",
  "process": {
    "launch": true,
    "stop": true,
    "restart": true
  },
  "build": {
    "check": true,
    "build": true,
    "test": true
  },
  "game": {
    "connected": true,
    "instance_id": "run-abc",
    "connection_id": "conn-def",
    "host": "ready"
  }
}
```

Example with no game connected:

```json
{
  "mode": "supervised",
  "build": {
    "check": true,
    "build": true,
    "test": true
  },
  "process": {
    "launch": true
  },
  "game": {
    "connected": false
  }
}
```

Game/ECS capabilities remain implemented but operationally unavailable while disconnected.

## Rebuild/restart semantics

v1 uses the conservative cross-platform flow:

```text
cargo check while old game remains running
 -> if check fails: return diagnostics, old game untouched
 -> stop old game
 -> cargo build
 -> if build fails: report failure; game may remain stopped
 -> launch new executable
 -> authenticate IPC
 -> host probe through Bevy ingress
 -> mark ready
 -> return new instance_id/connection_id
```

Do not make build-while-running the default v1 strategy. A running Windows executable/PDB can conflict with Cargo/linker output replacement.

A future optimization may build immutable/staged per-generation artifacts, allowing the old generation to remain running until the new artifact is complete.

A last-known-good launch artifact/rollback is desirable after the basic lifecycle is proven, but is not required to unblock Stages 1-4.

## Optional project configuration

Stage 4 may add `.bevy-mcp.toml`.

Conceptual minimum:

```toml
[project]
package = "my-game"
bin = "my-game"
profile = "dev"

[launch]
args = []
working_dir = "."
ready_timeout_secs = 20

[permissions]
check = true
build = true
test = true
process = true
```

Configuration is optional when Cargo metadata yields one unambiguous game target.

## Error taxonomy

The implementation should use stable machine-actionable codes. Initial set:

### Transport/protocol

```text
AUTH_FAILED
PROTOCOL_MISMATCH
MALFORMED_FRAME
FRAME_TOO_LARGE
GAME_UNAVAILABLE
GAME_DISCONNECTED
GAME_UNRESPONSIVE
CONNECTION_REPLACED
REQUEST_TIMEOUT
INSTANCE_ALREADY_CONNECTED
```

### Entity/world

```text
STALE_INSTANCE
INVALID_WORLD
INVALID_ENTITY_HANDLE
ENTITY_NOT_FOUND
```

### Process

```text
PROCESS_NOT_MANAGED
PROCESS_ALREADY_RUNNING
PROCESS_NOT_RUNNING
PROCESS_START_TIMEOUT
PROCESS_EXITED_DURING_STARTUP
PROCESS_STOP_TIMEOUT
PROCESS_CRASHED
```

### Cargo/project

```text
CARGO_NOT_AVAILABLE
PROJECT_METADATA_FAILED
TARGET_AMBIGUOUS
TARGET_NOT_FOUND
FEATURE_UNKNOWN
CARGO_OPERATION_IN_PROGRESS
BUILD_FAILED
BUILD_TIMEOUT
BUILD_CANCELLED
TEST_FAILED
TEST_TIMEOUT
TEST_CANCELLED
```

Error payloads should include relevant IDs/state but never return the authentication token.

# Implementation stages

## Stage 1 - Wire protocol and supervisor foundation

### Scope

- Add `bevy-mcp-supervisor` crate and persistent `bevy-mcp` binary skeleton.
- Define explicit serde-tagged wire messages and bounded length-prefixed framing.
- Add serialization for shared core command/result types required on the wire.
- Introduce `GameCommandBackend` and move base, advanced, and debugger surfaces onto it.
- Implement `EmbeddedBackend` preserving current queue/dispatcher behavior.
- Implement supervised IPC backend and game-side bridge.
- Add `instance_id` resource/context to the host.
- Replace hardcoded `default` entity instance handling with current instance validation.
- Add typed `STALE_INSTANCE` behavior.
- Implement token/protocol/instance handshake and connection generations.
- Implement internal frame-aware host probe.
- Preserve embedded mode.

### Stage 1 gates

A. **Backend unification**

All MCP surfaces use `GameCommandBackend`; no debugger/advanced path owns a separate direct dispatcher.

B. **Disconnect with requests in flight**

All pending requests for the lost `connection_id` complete with `GAME_DISCONNECTED` promptly.

C. **Old response isolation**

After reconnect, a response associated with the old connection cannot resolve a request on the new connection even if request IDs collide.

D. **Stale entity isolation**

An entity handle from `run-old` used against `run-new` returns `STALE_INSTANCE`, never `ENTITY_NOT_FOUND` and never a newly recycled entity.

E. **Handshake rejection**

Wrong token, protocol version, instance ID, malformed frame, and oversized frame are rejected deterministically without poisoning future connections.

F. **True readiness**

`Hello` alone does not mark ready. A Bevy-processed host probe is required.

G. **Blocked-main-thread detection**

A fixture that keeps transport ping/pong alive while blocking Bevy command processing yields:

```text
process = running
transport = connected
host = unresponsive
```

H. **Reconnect generation**

Same game instance reconnect after transport loss keeps `instance_id`, receives a new `connection_id`, and old pending requests stay failed.

I. **Single-active-game rule**

A simultaneous second game connection is rejected.

J. **Embedded compatibility**

Existing workspace tests pass and embedded MCP behavior remains available.

## Stage 2 - Process manager

### Scope

- `process_status`, `process_launch`, `process_stop`, `process_restart`, `process_logs`.
- Managed vs external ownership.
- Process/transport/host state model.
- Launch environment injection for address/token/instance.
- stdout/stderr ring buffers; null child stdin.
- startup readiness gate through authenticated host probe.
- crash/exit state and evidence.
- graceful wire shutdown, escalation, and forced cleanup.
- Unix process groups.
- Windows Job Objects.

### Stage 2 gates

- Launch does not report success before host readiness.
- Process exiting before readiness returns `PROCESS_EXITED_DURING_STARTUP` with exit code and stderr tail.
- Crash after ready is reflected without killing the supervisor MCP session.
- Game fixture spawns a long-lived descendant; stopping/killing the managed game removes the descendant too.
- Supervisor shutdown removes owned process-tree descendants.
- External/manual connection is never killed by lifecycle tools and returns `PROCESS_NOT_MANAGED` where appropriate.
- Host hang does not masquerade as transport failure.
- Restart produces a new `instance_id`.

## Stage 3 - Cargo executor

### Scope

- Cargo metadata project discovery.
- Typed/allowlisted package/bin/profile/features/test-filter parameters.
- Supervisor-local permissions.
- Direct Cargo process invocation only.
- JSON compiler diagnostic/artifact parsing.
- Bounded text output retention.
- Supervisor operation registry.
- status/cancel routing.
- timeouts and process-tree cancellation.
- one active Cargo operation per project.

### Stage 3 gates

- Deliberately broken fixture returns structured compiler error location/code/message.
- Successful build returns Cargo-reported executable artifact.
- Unknown feature/target is rejected before launching arbitrary commands.
- Ambiguous target returns `TARGET_AMBIGUOUS`.
- Cancellation tears down Cargo and descendants without orphan processes.
- Timeout tears down Cargo and descendants.
- MCP request returns operation ID promptly rather than waiting for the full build.
- A second Cargo operation receives a deterministic busy/conflict result.
- No shell execution path exists.

## Stage 4 - Autonomous dev-cycle integration

### Scope

- `rebuild_restart` composite orchestration.
- capability merging.
- lifecycle alias deprecation.
- startup/crash evidence attached to failures.
- optional `.bevy-mcp.toml`.
- Quick Start/client-guide updates.
- full autonomous-loop integration fixture.

### Stage 4 gates

1. Break source code.
2. Run `rebuild_restart`.
3. `cargo check` fails with structured diagnostics.
4. Existing game remains running and retains its instance.
5. Fix source.
6. Run `rebuild_restart`.
7. Check succeeds.
8. Old game shuts down.
9. Build succeeds.
10. New process launches and authenticates.
11. Host probe succeeds.
12. New `instance_id` and `connection_id` are returned.
13. Old entity handle returns `STALE_INSTANCE`.
14. Agent can immediately run world inspection/playtest against the new instance.

# Testing strategy

## Unit tests

- wire encode/decode and explicit tags;
- frame length bounds;
- handshake validator;
- backend error mapping;
- instance/entity resolution;
- connection generation routing;
- process state transitions;
- Cargo metadata target resolution;
- compiler message parsing;
- permission checks.

## Integration fixtures

Maintain small purpose-built fixtures rather than using the full example game for every failure mode:

```text
healthy_game
slow_or_blocked_game
crashing_game
child_spawning_game
broken_compile_project
ambiguous_target_project
long_build_project/test double
```

Stage tests may spawn fixtures internally even before lifecycle tools are publicly available. Product-visible process control is still Stage 2.

## Platform matrix

Process-tree lifecycle behavior must be exercised on Windows and at least one Unix CI platform. Windows is not an optional follow-up because executable locking and Job Object behavior directly affect the architecture.

# Documentation/migration requirements

Until Stage 4 lands, existing docs continue to describe embedded mode as the supported user path.

When supervised mode becomes usable:

- document embedded vs supervised modes explicitly;
- configure MCP clients to launch the persistent `bevy-mcp` supervisor, not the game binary;
- explain game-side bridge integration;
- explain Cargo/process trust boundary;
- describe build/process operations as unavailable in embedded mode;
- mark lifecycle `runtime_launch/stop/restart` aliases deprecated in supervised mode;
- never imply that a connected socket alone means the game is ready.

# Definition of done for the supervisor program

The supervisor architecture is considered complete enough for transactional playtest work when an agent can perform the following without losing its MCP session:

```text
inspect current game
 -> edit source externally
 -> start cargo check
 -> poll operation
 -> receive structured compile diagnostics
 -> fix source externally
 -> rebuild/restart
 -> survive process replacement
 -> receive new instance identity
 -> reject stale handles
 -> inspect the new world
 -> execute debugger/playtest tools
```

At every point, failures must be classified as build, process, transport, host-liveness, or game-command failures rather than collapsing into a generic timeout/disconnect.

# Deferred follow-ups

After Stages 1-4 are stable:

- staged immutable build artifacts for low-downtime restart;
- last-known-good artifact rollback;
- multi-instance supervision;
- richer game-scoped IDs for checkpoints/recordings/evidence;
- event-driven response wakeups if still useful after transport refactor;
- release/version packaging for the `bevy-mcp` supervisor binary.
