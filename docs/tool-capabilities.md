# MCP capability contract

`capabilities` is a live host query. It no longer returns a hard-coded server-side feature list.

Each capability reports four independent fields:

- `implemented`: the MCP has an implementation for this operation.
- `available`: the current Bevy app has the runtime resource/target needed by that implementation.
- `allowed`: current `McpPermissions` allow the operation.
- `operational`: all three conditions are true.

This distinction prevents misleading results. For example, viewport capture is implemented but can be unavailable in a `MinimalPlugins` app without a primary window; key input can be implemented and installed but disallowed under read-only permissions.

The response also includes a `deprecations` array. Legacy `capture_game` and `capture_camera` remain functional aliases for `capture_viewport`, while the old `playtest_run` surface is explicitly unavailable and points agents to the frame-driven `playtest_start`/`playtest_status` debugger API.

Known interaction surfaces reserved for the next Agent Interaction work—mouse motion, UI click/type, camera framing/transform/look-at—report `implemented: false` instead of being advertised as working. Asset inspection/reload and embedded cargo build/test surfaces likewise report false.

`resource_writers` and `component_writers` use the selected API kind to choose the exact registered access list. Resource-writer discovery therefore continues to work when a registered resource type currently has no live resource instance.
