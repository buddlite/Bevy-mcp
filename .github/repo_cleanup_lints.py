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
