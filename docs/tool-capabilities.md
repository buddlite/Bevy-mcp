# MCP capability contract

`capabilities` is the live execution contract for the current MCP mode. Agents should query it instead of assuming a tool is usable because the tool name exists.

Each capability reports four independent fields:

- `implemented`: this MCP mode has an implementation for the operation.
- `available`: the current host/project/process state provides what the implementation needs.
- `allowed`: the relevant permission policy allows the operation.
- `operational`: all three conditions are true.

Agents should use `operational` as the immediate execution gate and inspect the other fields to explain why an operation is unavailable.

## Embedded mode

In embedded mode, `capabilities` is a live Bevy-host query. Runtime availability is derived from the actual app: for example, viewport capture can be implemented but unavailable without the renderer/primary window, and key input can be installed but disallowed by read-only `McpPermissions`.

Cargo build/check/test and OS process lifecycle are deliberately external in embedded mode, so their embedded capability entries remain unavailable/unimplemented rather than pretending the game can rebuild itself.

## Supervised mode

In supervised mode, the persistent `bevy-mcp` process requests the live host capability contract when a game is connected and ready, then merges supervisor-owned functionality into that response.

The merged contract adds or overrides the supervisor surfaces for:

- Cargo `build_check`, `build`, and `test`
- managed process `launch`, `stop`, and `restart`
- conservative `rebuild_restart`
- supervisor/project/process availability and permission context

Host permissions and supervisor permissions are separate trust boundaries. A game may expose read-only runtime access while the supervisor separately allows or denies Cargo/process operations.

If the Bevy host is disconnected or not ready, supervisor-owned build/lifecycle capabilities can still be reported from the persistent control plane; host-only runtime capabilities must not be fabricated as available.

`development_status` complements rather than replaces `capabilities`: it condenses current process/build state, active operations, recent failure evidence, and a recommended next action, while `capabilities` answers whether a proposed operation is currently implemented, available, and allowed.

## Runtime-specific capability notes

The response includes a `deprecations` array. Legacy `capture_game` and `capture_camera` remain functional aliases for `capture_viewport`, while the old `playtest_run` surface is explicitly unavailable and points agents to the frame-driven `playtest_start` / `playtest_status` debugger API.

Native pointer motion/picking, UI click/type, and camera framing/transform/look-at are implemented by the Agent Interaction layer; their availability and permissions still reflect the live app. Path-targeted asset inspection, status, and reload are implemented when `AssetServer` is present; global asset enumeration remains unavailable because Bevy's public `AssetServer` API does not expose an all-path iterator.

`resource_writers` and `component_writers` use the selected API kind to choose the exact registered access list. Resource-writer discovery therefore continues to work when a registered resource type currently has no live resource instance.

Host capability discovery remains available even when the runtime permission level is `none`; it reports `allowed: false` rather than denying the discovery request. Capture availability is renderer-aware: a window or camera target alone is not considered operational unless Bevy's `RenderDevice` is present.
