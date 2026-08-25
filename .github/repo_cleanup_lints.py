from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected lint-cleanup anchor missing in {path}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# Reduce the in-memory size of the tagged debugger request enum without changing
# its serde JSON representation. Box<T> serializes/deserializes transparently.
replace_once(
    "crates/bevy-mcp-core/src/debug.rs",
    "    WatchpointAdd {\n        spec: WatchpointSpec,\n    },",
    "    WatchpointAdd {\n        spec: Box<WatchpointSpec>,\n    },",
)
replace_once(
    "crates/bevy-mcp-server/src/debug_tools.rs",
    '''            .call(DebugRequest::WatchpointAdd {
                spec: WatchpointSpec {
                    name: params.name,
                    condition,
                    pause_on_trigger: params.pause_on_trigger.unwrap_or(false),
                    once: params.once.unwrap_or(true),
                    evidence: evidence_from_params(params.evidence),
                },
            })
''',
    '''            .call(DebugRequest::WatchpointAdd {
                spec: Box::new(WatchpointSpec {
                    name: params.name,
                    condition,
                    pause_on_trigger: params.pause_on_trigger.unwrap_or(false),
                    once: params.once.unwrap_or(true),
                    evidence: evidence_from_params(params.evidence),
                }),
            })
''',
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''        DebugRequest::WatchpointAdd { spec } => {
            let mut debugger = world.resource_mut::<McpDebugger>();
''',
    '''        DebugRequest::WatchpointAdd { spec } => {
            let spec = *spec;
            let mut debugger = world.resource_mut::<McpDebugger>();
''',
)
replace_once(
    "crates/bevy-mcp-host/tests/agent_debugger.rs",
    '''        DebugRequest::WatchpointAdd {
            spec: WatchpointSpec {
                name: "frame-zero".into(),
                condition: DebugCondition::FrameAtLeast { frame: 0 },
                pause_on_trigger: false,
                once: true,
                evidence: no_screenshot_evidence(),
            },
        },
''',
    '''        DebugRequest::WatchpointAdd {
            spec: Box::new(WatchpointSpec {
                name: "frame-zero".into(),
                condition: DebugCondition::FrameAtLeast { frame: 0 },
                pause_on_trigger: false,
                once: true,
                evidence: no_screenshot_evidence(),
            }),
        },
''',
)

# aggregate_world_bounds is an implementation detail used only by the camera module;
# its previous pub(crate) visibility exposed a private return type and produced a warning.
replace_once(
    "crates/bevy-mcp-host/src/systems/camera.rs",
    "pub(crate) fn aggregate_world_bounds(",
    "fn aggregate_world_bounds(",
)

# The added tick was stored but never consulted: added/removed classification comes
# from snapshot membership, while change detection only needs the changed tick.
replace_once(
    "crates/bevy-mcp-host/src/change_tracking.rs",
    '''struct TickSnapshot {
    added: u32,
    changed: u32,
}
''',
    '''struct TickSnapshot {
    changed: u32,
}
''',
)
replace_once(
    "crates/bevy-mcp-host/src/change_tracking.rs",
    '''                TickSnapshot {
                    added: ticks.added.get(),
                    changed: ticks.changed.get(),
                },
''',
    '''                TickSnapshot {
                    changed: ticks.changed.get(),
                },
''',
)
replace_once(
    "crates/bevy-mcp-host/src/change_tracking.rs",
    '''        let snapshot = TickSnapshot {
            added: ticks.added.get(),
            changed: ticks.changed.get(),
        };
''',
    '''        let snapshot = TickSnapshot {
            changed: ticks.changed.get(),
        };
''',
)

# Avoid duplicate push+pull_request CI on every feature branch, and move checkout
# off the deprecated Node 20 runtime used by checkout v4.
replace_once(
    ".github/workflows/ci.yml",
    '''on:
  push:
  pull_request:
''',
    '''on:
  push:
    branches: [v.01]
  pull_request:
    branches: [v.01]
''',
)
path = Path(".github/workflows/ci.yml")
path.write_text(path.read_text(encoding="utf-8").replace("actions/checkout@v4", "actions/checkout@v7"), encoding="utf-8")

# Bounded operation retention is observable: old terminal IDs may eventually be
# evicted, so document that contract next to operation_status guidance.
replace_once(
    "docs/supervised-mode.md",
    "Poll it using `operation_status`. `operation_cancel` can cancel the active Cargo child immediately; lifecycle-stage cancellation is observed at safe boundaries so the supervisor does not intentionally leave a half-transitioned process tree.\n",
    "Poll it using `operation_status`. `operation_cancel` can cancel the active Cargo child immediately; lifecycle-stage cancellation is observed at safe boundaries so the supervisor does not intentionally leave a half-transitioned process tree. Supervisor operation history is bounded: the oldest terminal Cargo and `rebuild_restart` records may be evicted after sustained use, while active operations are never pruned.\n",
)

# Reconcile the capability reference with the merged supervisor architecture.
Path("docs/tool-capabilities.md").write_text(r'''# MCP capability contract

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
''', encoding="utf-8")
