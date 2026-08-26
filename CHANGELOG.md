# Changelog

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
- Persistent supervisor transport and process lifecycle management, including authenticated game reconnection, instance/connection generations, frame-aware readiness, managed launch/stop/restart, bounded process logs, and cross-platform process-tree ownership.
- Supervisor Cargo execution for `build_check`, `build`, and `test`, with `cargo metadata` target discovery, typed package/bin/profile/features/test-filter parameters, structured compiler diagnostics and executable artifacts, bounded output, asynchronous `supervisor:*` operation IDs, cancellation/timeouts, one-operation-at-a-time locking, and supervisor-local permissions.
- Stage 4 supervised development-cycle tooling: asynchronous `rebuild_restart`, conservative check-before-stop sequencing, Cargo-artifact launch, new instance/connection validation, merged host/supervisor capabilities, and bounded startup/crash evidence.
- Agent-oriented `development_status` diagnostics that collapse process/Cargo/rebuild state into one normalized development state, current generation identity, latest structured failure evidence, and a deterministic recommended recovery action.

### Changed

- The normal autonomous-agent entry point is `AgentBevyMcpServer`; `BevyMcpServer` remains the base/legacy router.
- Front-page documentation now describes the live runtime surface instead of a fixed tool count.
- Agent-facing mutation, interaction, assertion, debugging, and replay flows are documented around the loop `inspect -> mutate -> step/interact -> assert -> diagnose -> replay/checkpoint -> retry`.
- In supervised mode, build/test and OS process lifecycle authority live in the persistent supervisor rather than the Bevy host; embedded mode retains the existing game-local permission boundary and externally owned lifecycle.
- The supervised `capabilities` response now merges the live Bevy-host contract with supervisor Cargo, process, and `rebuild_restart` availability instead of exposing the embedded build/lifecycle contract unchanged.
- Repository CI now enforces rustfmt and Clippy in addition to cross-platform compile/test coverage.
- Supervisor Cargo and rebuild/restart operation histories are bounded by evicting the oldest terminal records.
- Onboarding and contributor documentation now distinguishes supervised and embedded execution modes consistently.
- Stage-numbered supervisor acceptance tests were renamed to reflect their continuing cross-stage role.
- Bevy integration is aligned with Bevy 0.19.1 semantics: the workspace MSRV matches Bevy, synthetic key/mouse/gamepad input follows Bevy's current input lifecycle and per-entity gamepad model, invalid virtual-time scales are rejected safely, and paused frame stepping respects the configured time scale.
- Hierarchy reparenting now validates parents, self-parenting, and cycles before mutation; reflected short-name lookup rejects ambiguous registrations; schedule/system conflict inspection resolves names safely after Bevy has initialized the executable schedule.
- `development_status` no longer recommends `process_launch` or `rebuild_restart` from passive `stopped`, `game_exited`, or `idle` states; lifecycle-changing recovery suggestions are explicitly marked `automatic_safe: false` so polling cannot become an implicit restart policy.

### Current limitations

- Embedded `build_check`, `build`, and `test` tools return `BUILD_NOT_AVAILABLE`; supervisor mode provides the trusted Cargo executor instead.
- `asset_list` is reserved; loaded-asset enumeration is not implemented.
- Atomic batches intentionally exclude entity lifecycle, hierarchy changes, input/runtime operations, semantic actions, and arbitrary side effects. `verify` mode is not implemented.
- Entity duplication remains reserved until safe reflected cloning is implemented.
- Embedded runtime launch/stop/restart remain externally owned.
- Generic `input_action` is not implemented; register semantic actions instead.
- Checkpoint restoration covers only explicitly registered checkpoint state/adapters.
- Supervisor Cargo operations execute project build scripts/proc macros as trusted local development code; keep supervisor build permissions disabled for untrusted projects.

### Release history note

The previous `0.1.0` changelog entry was removed because it described a mixture of planned and unavailable capabilities (including a fixed tool count, embedded Cargo tools, loaded-asset enumeration, broad atomic/verify batching, and entity duplication) rather than a reliable shipped-state record. A tagged release should be added here only when its published crates and documented capability set are reconciled.
