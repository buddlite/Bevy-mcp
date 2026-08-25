from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected clippy-cleanup anchor missing in {path}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# End Bevy resource borrows with lexical scopes rather than explicit drop(Mut<T>).
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''            let mut debugger = world.resource_mut::<McpDebugger>();
            let id = debugger.allocate_id("watch");
            debugger.watchpoints.insert(
                id.clone(),
                WatchpointRuntime {
                    id: id.clone(),
                    spec: spec.clone(),
                    enabled: true,
                    trigger_count: 0,
                    was_matched: false,
                    last_trigger_frame: None,
                    last_evaluation: None,
                    last_error: None,
                    evidence: None,
                },
            );
            drop(debugger);
''',
    '''            let id = {
                let mut debugger = world.resource_mut::<McpDebugger>();
                let id = debugger.allocate_id("watch");
                debugger.watchpoints.insert(
                    id.clone(),
                    WatchpointRuntime {
                        id: id.clone(),
                        spec: spec.clone(),
                        enabled: true,
                        trigger_count: 0,
                        was_matched: false,
                        last_trigger_frame: None,
                        last_evaluation: None,
                        last_error: None,
                        evidence: None,
                    },
                );
                id
            };
''',
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    ".map(|watchpoint| watchpoint_json(watchpoint, &debugger))",
    ".map(|watchpoint| watchpoint_json(watchpoint, debugger))",
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    "McpResult::success(playtest_json(session, &debugger))",
    "McpResult::success(playtest_json(session, debugger))",
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    ".map(|session| playtest_json(session, &debugger))",
    ".map(|session| playtest_json(session, debugger))",
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''                    let mut store = world.resource_mut::<McpCheckpointStore>();
                    let id = store.next_id();
                    store.insert(StoredCheckpoint {
                        id: id.clone(),
                        name: name.clone(),
                        frame,
                        values,
                    });
                    drop(store);
''',
    '''                    let id = {
                        let mut store = world.resource_mut::<McpCheckpointStore>();
                        let id = store.next_id();
                        store.insert(StoredCheckpoint {
                            id: id.clone(),
                            name: name.clone(),
                            frame,
                            values,
                        });
                        id
                    };
''',
)
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''    let frame = current_frame(world);
    let mut debugger = world.resource_mut::<McpDebugger>();
    if let Some(active) = debugger
        .playtests
        .values()
        .find(|session| session.status == PlaytestStatus::Running)
    {
        let active_id = active.id.clone();
        drop(debugger);
        push_error(
            world,
            request_id,
            "PLAYTEST_ALREADY_RUNNING",
            format!("Playtest '{active_id}' is already running"),
        );
        return;
    }

    let id = debugger.allocate_id("playtest");
    let step_count = plan.steps.len();
    debugger.playtests.insert(
        id.clone(),
        PlaytestRuntime {
            id: id.clone(),
            plan: plan.clone(),
            status: PlaytestStatus::Running,
            step_index: 0,
            step_started_frame: None,
            started_frame: frame,
            finished_frame: None,
            step_results: Vec::new(),
            failure: None,
            evidence: None,
            captures: Vec::new(),
        },
    );
    drop(debugger);
''',
    '''    let frame = current_frame(world);
    let active_id = world
        .resource::<McpDebugger>()
        .playtests
        .values()
        .find(|session| session.status == PlaytestStatus::Running)
        .map(|session| session.id.clone());
    if let Some(active_id) = active_id {
        push_error(
            world,
            request_id,
            "PLAYTEST_ALREADY_RUNNING",
            format!("Playtest '{active_id}' is already running"),
        );
        return;
    }

    let step_count = plan.steps.len();
    let id = {
        let mut debugger = world.resource_mut::<McpDebugger>();
        let id = debugger.allocate_id("playtest");
        debugger.playtests.insert(
            id.clone(),
            PlaytestRuntime {
                id: id.clone(),
                plan: plan.clone(),
                status: PlaytestStatus::Running,
                step_index: 0,
                step_started_frame: None,
                started_frame: frame,
                finished_frame: None,
                step_results: Vec::new(),
                failure: None,
                evidence: None,
                captures: Vec::new(),
            },
        );
        id
    };
''',
)

# Keep Result<()> construction explicit when the queued operation itself returns unit.
replace_once(
    "crates/bevy-mcp-host/src/interaction.rs",
    '''            Ok(queue(
                world,
                PendingInteraction {
                    request_id,
                    location: from.clone(),
                    kind: InteractionKind::Drag {
                        button,
                        from,
                        to,
                        steps,
                        step: 0,
                    },
                    phase: 0,
                },
            ))
''',
    '''            queue(
                world,
                PendingInteraction {
                    request_id,
                    location: from.clone(),
                    kind: InteractionKind::Drag {
                        button,
                        from,
                        to,
                        steps,
                        step: 0,
                    },
                    phase: 0,
                },
            );
            Ok(())
''',
)
replace_once(
    "crates/bevy-mcp-host/src/interaction.rs",
    '''        let mut computed = bevy::ui::ComputedNode::default();
        computed.inverse_scale_factor = 0.5;
''',
    '''        let computed = bevy::ui::ComputedNode {
            inverse_scale_factor: 0.5,
            ..Default::default()
        };
''',
)

# These resource wrappers have exactly the same Default as their inner queue.
replace_once(
    "crates/bevy-mcp-host/src/queue.rs",
    '''#[derive(bevy::prelude::Resource, Clone)]
pub struct McpIngressQueue(bevy_mcp_core::queue::McpIngressQueue);

impl Default for McpIngressQueue {
    fn default() -> Self {
        Self(bevy_mcp_core::queue::McpIngressQueue::default())
    }
}
''',
    '''#[derive(bevy::prelude::Resource, Clone, Default)]
pub struct McpIngressQueue(bevy_mcp_core::queue::McpIngressQueue);
''',
)
replace_once(
    "crates/bevy-mcp-host/src/queue.rs",
    '''#[derive(bevy::prelude::Resource, Clone)]
pub struct McpResultQueue(bevy_mcp_core::queue::McpResultQueue);

impl Default for McpResultQueue {
    fn default() -> Self {
        Self(bevy_mcp_core::queue::McpResultQueue::default())
    }
}
''',
    '''#[derive(bevy::prelude::Resource, Clone, Default)]
pub struct McpResultQueue(bevy_mcp_core::queue::McpResultQueue);
''',
)

# Move the pending vector out directly instead of draining into a new allocation.
replace_once(
    "crates/bevy-mcp-host/src/systems/dispatch.rs",
    "        deferred.pending.drain(..).collect::<Vec<_>>()",
    "        std::mem::take(&mut deferred.pending)",
)
