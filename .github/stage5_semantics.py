from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected semantics anchor missing in {path}: {old[:100]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


path = Path("crates/bevy-mcp-supervisor/src/development_status.rs")
replace_once(
    path,
    '''    let generation = generation(&process, &cargo_operations, &rebuild_operations);
    let state = classify_state(
        &process,
        cargo_available,
        permissions,
        active_operation.as_ref(),
        last_failure.as_ref(),
    );
    let recovery = recovery_action(
        state,
        cargo_available,
        configured_launch_target,
        permissions,
        active_operation.as_ref(),
        last_failure.as_ref(),
    );
    let summary = summary(state, active_operation.as_ref(), last_failure.as_ref());
''',
    '''    let generation = generation(&process, &cargo_operations, &rebuild_operations);
    let latest_success = latest_success_unix_ms(&process, &cargo_operations, &rebuild_operations);
    let current_failure = last_failure.as_ref().filter(|failure| {
        failure.occurred_unix_ms.unwrap_or_default() > latest_success
    });
    let state = classify_state(
        &process,
        cargo_available,
        permissions,
        active_operation.as_ref(),
        current_failure,
    );
    let recovery = recovery_action(
        state,
        cargo_available,
        configured_launch_target,
        permissions,
        active_operation.as_ref(),
        current_failure,
    );
    let summary = summary(state, active_operation.as_ref(), current_failure);
''',
)
replace_once(
    path,
    '''fn generation(
    process: &ProcessSnapshot,
''',
    '''fn latest_success_unix_ms(
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> u128 {
    let cargo_success = cargo
        .iter()
        .filter(|operation| operation.state == CargoOperationState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();
    let rebuild_success = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();
    let ready_process = (process.state == ProcessState::Running && process.host == "ready")
        .then_some(process.started_unix_ms.unwrap_or_default())
        .unwrap_or_default();
    cargo_success.max(rebuild_success).max(ready_process)
}

fn generation(
    process: &ProcessSnapshot,
''',
)

# A stage-specific failure is more useful to an agent than the generic process
# state. In particular, a replacement that exits before host readiness is a
# startup failure even though ProcessManager records the process as crashed.
replace_once(
    path,
    '''    if process.ownership == ProcessOwnership::External {
        return DevelopmentState::ExternalGame;
    }
    if process.state == ProcessState::Crashed {
        return DevelopmentState::GameCrashed;
    }
    if process.host == "unresponsive" {
        return DevelopmentState::HostUnresponsive;
    }
    if process.state == ProcessState::Starting {
        return DevelopmentState::Starting;
    }

    if let Some(failure) = failure {
''',
    '''    if process.ownership == ProcessOwnership::External {
        return DevelopmentState::ExternalGame;
    }
    if process.host == "unresponsive" {
        return DevelopmentState::HostUnresponsive;
    }
    if process.state == ProcessState::Starting {
        return DevelopmentState::Starting;
    }

    if let Some(failure) = failure {
''',
)
replace_once(
    path,
    '''        if matches!(failure.stage.as_deref(), Some("launch"))
            || failure.code.starts_with("PROCESS_START")
            || failure.code == "PROCESS_EXITED_DURING_STARTUP"
        {
            return DevelopmentState::StartupFailed;
        }
    }

    if process.state == ProcessState::Running && process.host == "ready" {
''',
    '''        if matches!(failure.stage.as_deref(), Some("launch"))
            || failure.code.starts_with("PROCESS_START")
            || failure.code == "PROCESS_EXITED_DURING_STARTUP"
        {
            return DevelopmentState::StartupFailed;
        }
    }

    if process.state == ProcessState::Crashed {
        return DevelopmentState::GameCrashed;
    }
    if process.state == ProcessState::Running && process.host == "ready" {
''',
)

replace_once(
    path,
    '''    #[test]
    fn active_rebuild_takes_precedence_over_old_failure() {
''',
    '''    #[test]
    fn newer_success_supersedes_old_failure_without_erasing_history() {
        let mut successful = failed_check();
        successful.operation_id = "supervisor:check:2".to_string();
        successful.state = CargoOperationState::Succeeded;
        successful.created_unix_ms = 230;
        successful.started_unix_ms = Some(231);
        successful.finished_unix_ms = Some(240);
        successful.failure = None;
        successful.result.as_mut().unwrap().success = true;
        successful.result.as_mut().unwrap().error_count = 0;
        successful.result.as_mut().unwrap().diagnostics.clear();

        let status = compose_status(
            process(ProcessState::Running, "ready"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            vec![failed_check(), successful],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(status.state, DevelopmentState::Ready);
        assert_eq!(status.recovery.action, "continue_agent_loop");
        assert_eq!(status.last_failure.as_ref().unwrap().code, "BUILD_FAILED");
    }

    #[test]
    fn active_rebuild_takes_precedence_over_old_failure() {
''',
)
replace_once(
    path,
    '''        let status = compose_status(
            process(ProcessState::Stopped, "waiting"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            Vec::new(),
            vec![rebuild],
            Vec::new(),
            stderr,
        );

        assert_eq!(status.state, DevelopmentState::StartupFailed);
''',
    '''        let status = compose_status(
            process(ProcessState::Crashed, "waiting"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            Vec::new(),
            vec![rebuild],
            Vec::new(),
            stderr,
        );

        assert_eq!(status.state, DevelopmentState::StartupFailed);
''',
)
