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
