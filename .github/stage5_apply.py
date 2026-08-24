from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected patch anchor missing in {path}: {old[:100]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


# Wire the agent-oriented development_status tool into the supervisor router.
path = Path("crates/bevy-mcp-supervisor/src/process_tools.rs")
replace_once(
    path,
    "use crate::cargo_executor::{CargoError, CargoExecutor, CargoInvocation};\n",
    "use crate::cargo_executor::{CargoError, CargoExecutor, CargoInvocation};\nuse crate::development_status::DevelopmentStatus;\n",
)
replace_once(
    path,
    '''#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessEvidenceParams {
    #[schemars(description = "Maximum newest stdout/stderr lines per stream (default 50).")]
    pub limit: Option<u32>,
}

''',
    '''#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessEvidenceParams {
    #[schemars(description = "Maximum newest stdout/stderr lines per stream (default 50).")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DevelopmentStatusParams {
    #[schemars(description = "Maximum newest stdout/stderr lines retained in failure evidence (default 50).")]
    pub evidence_limit: Option<u32>,
}

''',
)
replace_once(
    path,
    '''    #[tool(
        description = "Return managed/external process ownership plus process, transport, and Bevy-host readiness state."
    )]
    async fn process_status(&self) -> String {
''',
    '''    #[tool(
        description = "Return one agent-oriented development diagnosis: current state, active operation, runtime/build identity, most recent failure with compiler/process evidence, and the recommended next tool/action."
    )]
    async fn development_status(
        &self,
        Parameters(params): Parameters<DevelopmentStatusParams>,
    ) -> String {
        let limit = params.evidence_limit.unwrap_or(50).clamp(1, 1000) as usize;
        let status = DevelopmentStatus::collect(
            &self.manager,
            &self.cargo,
            &self.rebuild,
            self.permissions,
            limit,
        )
        .await;
        serde_json::to_string(&status).unwrap()
    }

    #[tool(
        description = "Return managed/external process ownership plus process, transport, and Bevy-host readiness state."
    )]
    async fn process_status(&self) -> String {
''',
)

# Extend the Stage 4 real Cargo/process fixtures to validate development_status
# against actual compile and startup failures, not only synthetic snapshots.
path = Path("crates/bevy-mcp-supervisor/src/stage4_acceptance.rs")
replace_once(
    path,
    '''    CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationSnapshot,
    CargoOperationState, ProcessManager, ProcessManagerConfig, ProcessOwnership, ProcessSnapshot,
    ProcessState, RebuildRestartCoordinator, RebuildRestartSnapshot, RebuildRestartState,
    SupervisorPermissions, SupervisorTransport,
''',
    '''    CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationSnapshot,
    CargoOperationState, DevelopmentState, DevelopmentStatus, ProcessManager, ProcessManagerConfig,
    ProcessOwnership, ProcessSnapshot, ProcessState, RebuildRestartCoordinator,
    RebuildRestartSnapshot, RebuildRestartState, SupervisorPermissions, SupervisorTransport,
''',
)
replace_once(
    path,
    '''    assert_eq!(current.ownership, ProcessOwnership::Managed);
    assert_eq!(current.instance_id, initial_instance);
    assert_eq!(current.connection_id, initial_connection);
    manager.stop().await.unwrap();
''',
    '''    assert_eq!(current.ownership, ProcessOwnership::Managed);
    assert_eq!(current.instance_id, initial_instance);
    assert_eq!(current.connection_id, initial_connection);

    let development = DevelopmentStatus::collect(
        &manager,
        &executor,
        &coordinator,
        SupervisorPermissions::full(),
        50,
    )
    .await;
    assert_eq!(development.state, DevelopmentState::CompileFailed);
    let diagnostic_failure = development.last_failure.as_ref().unwrap();
    assert_eq!(diagnostic_failure.source, "rebuild_restart");
    assert_eq!(diagnostic_failure.stage.as_deref(), Some("check"));
    assert!(!diagnostic_failure.diagnostics.is_empty());
    assert_eq!(development.recovery.action, "fix_compile_errors");
    assert_eq!(development.recovery.tool.as_deref(), Some("rebuild_restart"));

    manager.stop().await.unwrap();
''',
)
replace_once(
    path,
    '''    assert!(
        failure
            .details
            .to_string()
            .contains("stage4 startup failure marker")
    );
}

#[test]
''',
    '''    assert!(
        failure
            .details
            .to_string()
            .contains("stage4 startup failure marker")
    );

    let development = DevelopmentStatus::collect(
        &manager,
        &executor,
        &coordinator,
        SupervisorPermissions::full(),
        50,
    )
    .await;
    assert_eq!(development.state, DevelopmentState::StartupFailed);
    let startup_failure = development.last_failure.as_ref().unwrap();
    assert_eq!(startup_failure.stage.as_deref(), Some("launch"));
    assert!(
        startup_failure
            .stderr_tail
            .iter()
            .any(|entry| entry.text.contains("stage4 startup failure marker"))
    );
    assert_eq!(development.recovery.tool.as_deref(), Some("process_evidence"));
}

#[test]
''',
)

# Documentation: development_status becomes the normal first diagnosis call,
# while capabilities remains the detailed contract and process_evidence remains
# the deeper log surface.
path = Path("docs/supervised-mode.md")
replace_once(
    path,
    '''## Recommended agent loop

Start by calling `capabilities`. In supervised mode this is a merged contract containing both the current Bevy-host capabilities and supervisor-owned build/lifecycle capabilities.

For a source-code change, the normal autonomous loop is:
''',
    '''## Recommended agent loop

Start by calling `development_status`. It is the compact agent-facing diagnosis surface and returns the current development state, any active Cargo/rebuild operation, current instance/connection/build identity, the most recent failure with structured compiler or process evidence, and one recommended recovery action/tool.

Use `capabilities` when the agent needs the full permission/availability contract. Use `process_evidence` when it needs a larger or explicit stdout/stderr view.

For a source-code change, the normal autonomous loop is:
''',
)
replace_once(
    path,
    '''1. edit source
2. rebuild_restart
3. operation_status until terminal
4. inspect returned check/build/startup evidence
5. process_evidence when additional stdout/stderr context is useful
6. inspect the new live world
7. interact / assert / playtest / diagnose
8. repeat
''',
    '''1. development_status
2. edit source
3. rebuild_restart
4. operation_status until terminal
5. development_status to classify success/failure and choose the next action
6. process_evidence when additional stdout/stderr context is useful
7. inspect the new live world
8. interact / assert / playtest / diagnose
9. repeat
''',
)
replace_once(
    path,
    '''## Process evidence

`process_evidence` returns:
''',
    '''## Agent-oriented development diagnosis

`development_status` is intended to answer the first question an autonomous coding agent normally has after any edit, build, restart, or crash: **what state am I in and what should I do next?**

Its response includes:

- a single normalized state such as `ready`, `rebuild_in_progress`, `compile_failed`, `startup_failed`, `game_crashed`, or `host_unresponsive`
- the active supervisor operation, if any
- current `instance_id`, `connection_id`, executable, and the most recent successful build/rebuild operation identities
- the latest relevant failure across Cargo, `rebuild_restart`, and the managed game process
- structured Rust compiler diagnostics for check/build/test failures
- bounded stdout/stderr evidence for process/startup failures
- a deterministic recovery action and the MCP tool that should normally be called next

The status is advisory orchestration metadata: it does not mutate the game or automatically execute the recovery action.

## Process evidence

`process_evidence` returns:
''',
)

path = Path("README.md")
replace_once(
    path,
    '''`rebuild_restart` is asynchronous and returns a `supervisor:rebuild_restart:*` operation ID. Poll it with `operation_status`; use `process_evidence` for bounded stdout/stderr plus process state when startup or runtime failures need diagnosis. A failed preflight check leaves the old managed game untouched. A build failure after the stop phase deliberately leaves the game stopped rather than relaunching stale code.
''',
    '''`development_status` is the normal agent-facing entry point for supervised diagnosis: it condenses the current process/build state, active operation, latest compiler or crash evidence, and recommended next action into one response. `rebuild_restart` is asynchronous and returns a `supervisor:rebuild_restart:*` operation ID. Poll it with `operation_status`; use `process_evidence` when deeper stdout/stderr context is useful. A failed preflight check leaves the old managed game untouched. A build failure after the stop phase deliberately leaves the game stopped rather than relaunching stale code.
''',
)

path = Path("CHANGELOG.md")
replace_once(
    path,
    '''- Stage 4 supervised development-cycle tooling: asynchronous `rebuild_restart`, conservative check-before-stop sequencing, Cargo-artifact launch, new instance/connection validation, merged host/supervisor capabilities, and bounded startup/crash evidence.
''',
    '''- Stage 4 supervised development-cycle tooling: asynchronous `rebuild_restart`, conservative check-before-stop sequencing, Cargo-artifact launch, new instance/connection validation, merged host/supervisor capabilities, and bounded startup/crash evidence.
- Agent-oriented `development_status` diagnostics that collapse process/Cargo/rebuild state into one normalized development state, current generation identity, latest structured failure evidence, and a deterministic recommended recovery action.
''',
)
