use serde::Serialize;
use serde_json::Value;

use crate::cargo_executor::{
    CargoDiagnostic, CargoExecutor, CargoOperationKind, CargoOperationSnapshot, CargoOperationState,
};
use crate::permissions::SupervisorPermissions;
use crate::process_manager::{
    ProcessLogEntry, ProcessManager, ProcessOwnership, ProcessSnapshot, ProcessState,
};
use crate::rebuild_restart::{
    RebuildRestartCoordinator, RebuildRestartSnapshot, RebuildRestartState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DevelopmentState {
    Ready,
    RebuildInProgress,
    CargoInProgress,
    CompileFailed,
    TestFailed,
    StartupFailed,
    GameCrashed,
    HostUnresponsive,
    Starting,
    GameExited,
    Stopped,
    ExternalGame,
    ProjectUnavailable,
    PermissionBlocked,
    Idle,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevelopmentOperationRef {
    pub kind: String,
    pub operation_id: String,
    pub state: String,
    pub stage: Option<String>,
    pub created_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevelopmentGeneration {
    pub instance_id: Option<String>,
    pub connection_id: Option<String>,
    pub executable: Option<String>,
    pub process_started_unix_ms: Option<u128>,
    pub last_successful_build_operation_id: Option<String>,
    pub last_successful_build_finished_unix_ms: Option<u128>,
    pub last_successful_rebuild_operation_id: Option<String>,
    pub last_successful_rebuild_finished_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevelopmentFailure {
    pub source: String,
    pub operation_id: Option<String>,
    pub stage: Option<String>,
    pub code: String,
    pub message: String,
    pub occurred_unix_ms: Option<u128>,
    pub diagnostics: Vec<CargoDiagnostic>,
    pub stdout_tail: Vec<ProcessLogEntry>,
    pub stderr_tail: Vec<ProcessLogEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryAction {
    pub action: String,
    pub reason: String,
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevelopmentProjectStatus {
    pub cargo_available: bool,
    pub initialization_error: Option<Value>,
    pub configured_launch_target: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevelopmentStatus {
    pub schema_version: u32,
    pub state: DevelopmentState,
    pub summary: String,
    pub process: ProcessSnapshot,
    pub project: DevelopmentProjectStatus,
    pub active_operation: Option<DevelopmentOperationRef>,
    pub generation: DevelopmentGeneration,
    pub last_failure: Option<DevelopmentFailure>,
    pub recovery: RecoveryAction,
}

impl DevelopmentStatus {
    pub async fn collect(
        manager: &ProcessManager,
        cargo: &CargoExecutor,
        rebuild: &RebuildRestartCoordinator,
        permissions: SupervisorPermissions,
        log_limit: usize,
    ) -> Self {
        let process = manager.status().await;
        let cargo_operations = cargo.status(None).unwrap_or_default();
        let rebuild_operations = rebuild.status(None).unwrap_or_default();
        let stdout_tail = manager
            .logs(Some("stdout"), log_limit.max(1))
            .unwrap_or_default();
        let stderr_tail = manager
            .logs(Some("stderr"), log_limit.max(1))
            .unwrap_or_default();

        compose_status(
            process,
            cargo.available(),
            cargo.initialization_error().map(|error| error.to_json()),
            manager.has_configured_launch_target(),
            permissions,
            cargo_operations,
            rebuild_operations,
            stdout_tail,
            stderr_tail,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn compose_status(
    process: ProcessSnapshot,
    cargo_available: bool,
    initialization_error: Option<Value>,
    configured_launch_target: bool,
    permissions: SupervisorPermissions,
    cargo_operations: Vec<CargoOperationSnapshot>,
    rebuild_operations: Vec<RebuildRestartSnapshot>,
    stdout_tail: Vec<ProcessLogEntry>,
    stderr_tail: Vec<ProcessLogEntry>,
) -> DevelopmentStatus {
    let active_operation = active_operation(&cargo_operations, &rebuild_operations);
    let last_failure = latest_failure(
        &process,
        &cargo_operations,
        &rebuild_operations,
        &stdout_tail,
        &stderr_tail,
    );
    let generation = generation(&process, &cargo_operations, &rebuild_operations);
    let current_failure = last_failure.as_ref().filter(|failure| {
        failure_is_current(failure, &process, &cargo_operations, &rebuild_operations)
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

    DevelopmentStatus {
        schema_version: 1,
        state,
        summary,
        process,
        project: DevelopmentProjectStatus {
            cargo_available,
            initialization_error,
            configured_launch_target,
        },
        active_operation,
        generation,
        last_failure,
        recovery,
    }
}

fn active_operation(
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> Option<DevelopmentOperationRef> {
    let active_rebuild = rebuild
        .iter()
        .filter(|operation| !rebuild_terminal(operation.state))
        .max_by_key(|operation| operation.created_unix_ms)
        .map(|operation| DevelopmentOperationRef {
            kind: "rebuild_restart".to_string(),
            operation_id: operation.operation_id.clone(),
            state: rebuild_state_name(operation.state).to_string(),
            stage: Some(rebuild_state_name(operation.state).to_string()),
            created_unix_ms: operation.created_unix_ms,
        });
    if active_rebuild.is_some() {
        return active_rebuild;
    }

    cargo
        .iter()
        .filter(|operation| !cargo_terminal(operation.state))
        .max_by_key(|operation| operation.created_unix_ms)
        .map(|operation| DevelopmentOperationRef {
            kind: cargo_kind_name(operation.kind).to_string(),
            operation_id: operation.operation_id.clone(),
            state: cargo_state_name(operation.state).to_string(),
            stage: None,
            created_unix_ms: operation.created_unix_ms,
        })
}

fn latest_failure(
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
    stdout_tail: &[ProcessLogEntry],
    stderr_tail: &[ProcessLogEntry],
) -> Option<DevelopmentFailure> {
    let cargo_failure = cargo
        .iter()
        .filter(|operation| {
            matches!(
                operation.state,
                CargoOperationState::Failed | CargoOperationState::TimedOut
            )
        })
        .max_by_key(|operation| {
            operation
                .finished_unix_ms
                .unwrap_or(operation.created_unix_ms)
        })
        .map(failure_from_cargo);

    let rebuild_failure = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Failed)
        .max_by_key(|operation| {
            operation
                .finished_unix_ms
                .unwrap_or(operation.created_unix_ms)
        })
        .map(|operation| failure_from_rebuild(operation, stdout_tail, stderr_tail));

    let process_failure = (process.state == ProcessState::Crashed).then(|| DevelopmentFailure {
        source: "process".to_string(),
        operation_id: None,
        stage: Some("runtime".to_string()),
        code: "PROCESS_CRASHED".to_string(),
        message: match process.exit_code {
            Some(code) => format!("Managed game process crashed with exit code {code}"),
            None => "Managed game process crashed".to_string(),
        },
        occurred_unix_ms: process.exited_unix_ms,
        diagnostics: Vec::new(),
        stdout_tail: stdout_tail.to_vec(),
        stderr_tail: stderr_tail.to_vec(),
    });

    [cargo_failure, rebuild_failure, process_failure]
        .into_iter()
        .flatten()
        .max_by_key(|failure| {
            (
                failure.occurred_unix_ms.unwrap_or_default(),
                failure_source_priority(&failure.source),
            )
        })
}

fn failure_from_cargo(operation: &CargoOperationSnapshot) -> DevelopmentFailure {
    let (code, message) = operation
        .failure
        .as_ref()
        .map(|failure| (failure.code.clone(), failure.message.clone()))
        .unwrap_or_else(|| {
            (
                match operation.kind {
                    CargoOperationKind::Test => "TEST_FAILED",
                    _ => "BUILD_FAILED",
                }
                .to_string(),
                format!("Cargo {} failed", cargo_kind_name(operation.kind)),
            )
        });
    let diagnostics = operation
        .result
        .as_ref()
        .map(|result| result.diagnostics.clone())
        .unwrap_or_default();

    DevelopmentFailure {
        source: "cargo".to_string(),
        operation_id: Some(operation.operation_id.clone()),
        stage: Some(cargo_kind_name(operation.kind).to_string()),
        code,
        message,
        occurred_unix_ms: operation.finished_unix_ms,
        diagnostics,
        stdout_tail: Vec::new(),
        stderr_tail: Vec::new(),
    }
}

fn failure_from_rebuild(
    operation: &RebuildRestartSnapshot,
    stdout_tail: &[ProcessLogEntry],
    stderr_tail: &[ProcessLogEntry],
) -> DevelopmentFailure {
    let failure = operation.failure.as_ref();
    let stage = failure.map(|failure| failure.stage.clone());
    let diagnostics = match stage.as_deref() {
        Some("check") => operation
            .evidence
            .check
            .as_ref()
            .and_then(|snapshot| snapshot.result.as_ref())
            .map(|result| result.diagnostics.clone())
            .unwrap_or_default(),
        Some("build") => operation
            .evidence
            .build
            .as_ref()
            .and_then(|snapshot| snapshot.result.as_ref())
            .map(|result| result.diagnostics.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let process_evidence = matches!(stage.as_deref(), Some("launch" | "stop"));

    DevelopmentFailure {
        source: "rebuild_restart".to_string(),
        operation_id: Some(operation.operation_id.clone()),
        stage,
        code: failure
            .map(|failure| failure.code.clone())
            .unwrap_or_else(|| "REBUILD_RESTART_FAILED".to_string()),
        message: failure
            .map(|failure| failure.message.clone())
            .unwrap_or_else(|| "rebuild_restart failed".to_string()),
        occurred_unix_ms: operation.finished_unix_ms,
        diagnostics,
        stdout_tail: if process_evidence {
            stdout_tail.to_vec()
        } else {
            Vec::new()
        },
        stderr_tail: if process_evidence {
            stderr_tail.to_vec()
        } else {
            Vec::new()
        },
    }
}

fn failure_source_priority(source: &str) -> u8 {
    match source {
        "rebuild_restart" => 2,
        "cargo" => 1,
        _ => 0,
    }
}

fn failure_is_current(
    failure: &DevelopmentFailure,
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> bool {
    let occurred = failure.occurred_unix_ms.unwrap_or_default();
    let latest_rebuild_success = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Succeeded)
        .filter_map(|operation| operation.finished_unix_ms)
        .max()
        .unwrap_or_default();

    if failure.code.starts_with("TEST_") || failure.stage.as_deref() == Some("test") {
        let latest_test_success = cargo
            .iter()
            .filter(|operation| {
                operation.kind == CargoOperationKind::Test
                    && operation.state == CargoOperationState::Succeeded
            })
            .filter_map(|operation| operation.finished_unix_ms)
            .max()
            .unwrap_or_default();
        return occurred > latest_test_success;
    }

    if failure.code.starts_with("BUILD_")
        || matches!(failure.stage.as_deref(), Some("check" | "build"))
    {
        let latest_compile_success = cargo
            .iter()
            .filter(|operation| {
                matches!(
                    operation.kind,
                    CargoOperationKind::Check | CargoOperationKind::Build
                ) && operation.state == CargoOperationState::Succeeded
            })
            .filter_map(|operation| operation.finished_unix_ms)
            .max()
            .unwrap_or_default()
            .max(latest_rebuild_success);
        return occurred > latest_compile_success;
    }

    let latest_ready_process = if process.state == ProcessState::Running && process.host == "ready"
    {
        process.started_unix_ms.unwrap_or_default()
    } else {
        0
    };
    occurred > latest_ready_process.max(latest_rebuild_success)
}

fn generation(
    process: &ProcessSnapshot,
    cargo: &[CargoOperationSnapshot],
    rebuild: &[RebuildRestartSnapshot],
) -> DevelopmentGeneration {
    let last_build = cargo
        .iter()
        .filter(|operation| {
            operation.kind == CargoOperationKind::Build
                && operation.state == CargoOperationState::Succeeded
        })
        .max_by_key(|operation| {
            operation
                .finished_unix_ms
                .unwrap_or(operation.created_unix_ms)
        });
    let last_rebuild = rebuild
        .iter()
        .filter(|operation| operation.state == RebuildRestartState::Succeeded)
        .max_by_key(|operation| {
            operation
                .finished_unix_ms
                .unwrap_or(operation.created_unix_ms)
        });

    DevelopmentGeneration {
        instance_id: process.instance_id.clone(),
        connection_id: process.connection_id.clone(),
        executable: process.executable.clone(),
        process_started_unix_ms: process.started_unix_ms,
        last_successful_build_operation_id: last_build
            .map(|operation| operation.operation_id.clone()),
        last_successful_build_finished_unix_ms: last_build
            .and_then(|operation| operation.finished_unix_ms),
        last_successful_rebuild_operation_id: last_rebuild
            .map(|operation| operation.operation_id.clone()),
        last_successful_rebuild_finished_unix_ms: last_rebuild
            .and_then(|operation| operation.finished_unix_ms),
    }
}

fn classify_state(
    process: &ProcessSnapshot,
    cargo_available: bool,
    permissions: SupervisorPermissions,
    active: Option<&DevelopmentOperationRef>,
    failure: Option<&DevelopmentFailure>,
) -> DevelopmentState {
    if let Some(active) = active {
        return if active.kind == "rebuild_restart" {
            DevelopmentState::RebuildInProgress
        } else {
            DevelopmentState::CargoInProgress
        };
    }

    if process.ownership == ProcessOwnership::External {
        return DevelopmentState::ExternalGame;
    }
    if process.host == "unresponsive" {
        return DevelopmentState::HostUnresponsive;
    }
    if process.state == ProcessState::Starting {
        return DevelopmentState::Starting;
    }

    if let Some(failure) = failure {
        if failure.code == "SUPERVISOR_PERMISSION_DENIED" {
            return DevelopmentState::PermissionBlocked;
        }
        if failure.code.starts_with("TEST_") {
            return DevelopmentState::TestFailed;
        }
        if matches!(failure.stage.as_deref(), Some("check" | "build"))
            || failure.code.starts_with("BUILD_")
        {
            return DevelopmentState::CompileFailed;
        }
        if matches!(failure.stage.as_deref(), Some("launch"))
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
        return DevelopmentState::Ready;
    }
    if process.state == ProcessState::Exited {
        return DevelopmentState::GameExited;
    }
    if matches!(
        process.state,
        ProcessState::Stopped | ProcessState::Stopping
    ) {
        return DevelopmentState::Stopped;
    }
    if !cargo_available {
        return DevelopmentState::ProjectUnavailable;
    }
    if !(permissions.cargo_check || permissions.cargo_build || permissions.cargo_test) {
        return DevelopmentState::PermissionBlocked;
    }
    DevelopmentState::Idle
}

fn recovery_action(
    state: DevelopmentState,
    cargo_available: bool,
    configured_launch_target: bool,
    permissions: SupervisorPermissions,
    active: Option<&DevelopmentOperationRef>,
    failure: Option<&DevelopmentFailure>,
) -> RecoveryAction {
    match state {
        DevelopmentState::RebuildInProgress | DevelopmentState::CargoInProgress => RecoveryAction {
            action: "wait_for_operation".to_string(),
            reason: active
                .map(|operation| format!("{} is still {}", operation.operation_id, operation.state))
                .unwrap_or_else(|| "A supervisor operation is still running".to_string()),
            tool: Some("operation_status".to_string()),
        },
        DevelopmentState::CompileFailed => RecoveryAction {
            action: "fix_compile_errors".to_string(),
            reason: failure
                .map(|failure| failure.message.clone())
                .unwrap_or_else(|| "The most recent Cargo check/build failed".to_string()),
            tool: Some("rebuild_restart".to_string()),
        },
        DevelopmentState::TestFailed => RecoveryAction {
            action: "fix_failing_tests".to_string(),
            reason: failure
                .map(|failure| failure.message.clone())
                .unwrap_or_else(|| "The most recent Cargo test failed".to_string()),
            tool: Some("test".to_string()),
        },
        DevelopmentState::StartupFailed
        | DevelopmentState::GameCrashed
        | DevelopmentState::HostUnresponsive => RecoveryAction {
            action: "inspect_process_evidence".to_string(),
            reason: failure
                .map(|failure| failure.message.clone())
                .unwrap_or_else(|| "The game process or host is unhealthy".to_string()),
            tool: Some("process_evidence".to_string()),
        },
        DevelopmentState::Stopped | DevelopmentState::GameExited | DevelopmentState::Idle => {
            if cargo_available
                && permissions.cargo_check
                && permissions.cargo_build
                && permissions.process_launch
                && permissions.process_stop
            {
                RecoveryAction {
                    action: "rebuild_and_launch".to_string(),
                    reason: "No ready managed game is running and the supervised rebuild path is available".to_string(),
                    tool: Some("rebuild_restart".to_string()),
                }
            } else if configured_launch_target && permissions.process_launch {
                RecoveryAction {
                    action: "launch_configured_game".to_string(),
                    reason: "A managed launch target is configured".to_string(),
                    tool: Some("process_launch".to_string()),
                }
            } else {
                RecoveryAction {
                    action: "fix_project_or_permissions".to_string(),
                    reason: "No currently permitted launch path is available".to_string(),
                    tool: Some("capabilities".to_string()),
                }
            }
        }
        DevelopmentState::ProjectUnavailable => RecoveryAction {
            action: "fix_project_configuration".to_string(),
            reason: "Cargo project metadata is unavailable".to_string(),
            tool: Some("capabilities".to_string()),
        },
        DevelopmentState::PermissionBlocked => RecoveryAction {
            action: "enable_supervisor_permissions".to_string(),
            reason: "The required supervisor operation is disabled by policy".to_string(),
            tool: Some("capabilities".to_string()),
        },
        DevelopmentState::ExternalGame => RecoveryAction {
            action: "use_external_lifecycle".to_string(),
            reason:
                "The connected game is externally owned; the supervisor will not stop or rebuild it"
                    .to_string(),
            tool: Some("process_status".to_string()),
        },
        DevelopmentState::Starting => RecoveryAction {
            action: "wait_for_readiness".to_string(),
            reason: "The managed game is still starting".to_string(),
            tool: Some("development_status".to_string()),
        },
        DevelopmentState::Ready => RecoveryAction {
            action: "continue_agent_loop".to_string(),
            reason: "The managed game is connected and host-ready".to_string(),
            tool: None,
        },
    }
}

fn summary(
    state: DevelopmentState,
    active: Option<&DevelopmentOperationRef>,
    failure: Option<&DevelopmentFailure>,
) -> String {
    match state {
        DevelopmentState::Ready => {
            "Managed game is connected and ready for agent interaction".to_string()
        }
        DevelopmentState::RebuildInProgress | DevelopmentState::CargoInProgress => active
            .map(|operation| format!("{} is {}", operation.kind, operation.state))
            .unwrap_or_else(|| "A supervisor operation is in progress".to_string()),
        DevelopmentState::CompileFailed
        | DevelopmentState::TestFailed
        | DevelopmentState::StartupFailed
        | DevelopmentState::GameCrashed => failure
            .map(|failure| format!("{}: {}", failure.code, failure.message))
            .unwrap_or_else(|| "The most recent development operation failed".to_string()),
        DevelopmentState::HostUnresponsive => {
            "Game transport is connected but the Bevy host is unresponsive".to_string()
        }
        DevelopmentState::Starting => {
            "Managed game is starting and has not reached host readiness".to_string()
        }
        DevelopmentState::GameExited => {
            "Managed game exited without an active replacement".to_string()
        }
        DevelopmentState::Stopped => "No managed game is currently running".to_string(),
        DevelopmentState::ExternalGame => {
            "A game is connected but lifecycle ownership is external".to_string()
        }
        DevelopmentState::ProjectUnavailable => {
            "Cargo project discovery is unavailable".to_string()
        }
        DevelopmentState::PermissionBlocked => {
            "Supervisor permissions block the required development action".to_string()
        }
        DevelopmentState::Idle => "Supervisor is idle and no game is ready".to_string(),
    }
}

fn cargo_terminal(state: CargoOperationState) -> bool {
    matches!(
        state,
        CargoOperationState::Succeeded
            | CargoOperationState::Failed
            | CargoOperationState::Cancelled
            | CargoOperationState::TimedOut
    )
}

fn rebuild_terminal(state: RebuildRestartState) -> bool {
    matches!(
        state,
        RebuildRestartState::Succeeded
            | RebuildRestartState::Failed
            | RebuildRestartState::Cancelled
    )
}

fn cargo_kind_name(kind: CargoOperationKind) -> &'static str {
    match kind {
        CargoOperationKind::Check => "check",
        CargoOperationKind::Build => "build",
        CargoOperationKind::Test => "test",
    }
}

fn cargo_state_name(state: CargoOperationState) -> &'static str {
    match state {
        CargoOperationState::Queued => "queued",
        CargoOperationState::Running => "running",
        CargoOperationState::Cancelling => "cancelling",
        CargoOperationState::Succeeded => "succeeded",
        CargoOperationState::Failed => "failed",
        CargoOperationState::Cancelled => "cancelled",
        CargoOperationState::TimedOut => "timed_out",
    }
}

fn rebuild_state_name(state: RebuildRestartState) -> &'static str {
    match state {
        RebuildRestartState::Queued => "queued",
        RebuildRestartState::Checking => "checking",
        RebuildRestartState::Stopping => "stopping",
        RebuildRestartState::Building => "building",
        RebuildRestartState::Launching => "launching",
        RebuildRestartState::Cancelling => "cancelling",
        RebuildRestartState::Succeeded => "succeeded",
        RebuildRestartState::Failed => "failed",
        RebuildRestartState::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cargo_executor::{
        CargoInvocation, CargoOperationFailure, CargoRunResult, CargoSpan,
    };
    use crate::rebuild_restart::{RebuildRestartEvidence, RebuildRestartFailure};

    fn process(state: ProcessState, host: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            state,
            ownership: ProcessOwnership::Managed,
            pid: Some(123),
            instance_id: Some("instance-2".to_string()),
            connection_id: Some("connection-2".to_string()),
            transport: "connected".to_string(),
            host: host.to_string(),
            exit_code: None,
            executable: Some("game".to_string()),
            started_unix_ms: Some(100),
            exited_unix_ms: None,
            last_error: None,
        }
    }

    fn failed_check() -> CargoOperationSnapshot {
        CargoOperationSnapshot {
            operation_id: "supervisor:check:1".to_string(),
            kind: CargoOperationKind::Check,
            state: CargoOperationState::Failed,
            created_unix_ms: 200,
            started_unix_ms: Some(201),
            finished_unix_ms: Some(220),
            invocation: CargoInvocation::new(None, None, None, None, None),
            result: Some(CargoRunResult {
                success: false,
                exit_code: Some(101),
                duration_ms: 19,
                warning_count: 0,
                error_count: 1,
                diagnostics: vec![CargoDiagnostic {
                    level: "error".to_string(),
                    message: "cannot find value `missing`".to_string(),
                    code: Some("E0425".to_string()),
                    rendered: None,
                    spans: vec![CargoSpan {
                        file_name: "src/main.rs".to_string(),
                        line_start: 4,
                        line_end: 4,
                        column_start: 9,
                        column_end: 16,
                        is_primary: true,
                    }],
                }],
                executable: None,
                package: "game".to_string(),
                bin: "game".to_string(),
                profile: "dev".to_string(),
                features: Vec::new(),
                raw_output_tail: Vec::new(),
            }),
            failure: Some(CargoOperationFailure {
                code: "BUILD_FAILED".to_string(),
                message: "Cargo check exited unsuccessfully".to_string(),
            }),
        }
    }

    #[test]
    fn compiler_failure_becomes_one_actionable_status() {
        let status = compose_status(
            process(ProcessState::Running, "ready"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            vec![failed_check()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(status.state, DevelopmentState::CompileFailed);
        assert_eq!(status.recovery.action, "fix_compile_errors");
        assert_eq!(status.recovery.tool.as_deref(), Some("rebuild_restart"));
        let failure = status.last_failure.unwrap();
        assert_eq!(failure.code, "BUILD_FAILED");
        assert_eq!(failure.diagnostics.len(), 1);
        assert_eq!(failure.diagnostics[0].code.as_deref(), Some("E0425"));
    }

    #[test]
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
    fn successful_check_does_not_hide_unresolved_test_failure() {
        let mut failed_test = failed_check();
        failed_test.operation_id = "supervisor:test:1".to_string();
        failed_test.kind = CargoOperationKind::Test;
        failed_test.failure.as_mut().unwrap().code = "TEST_FAILED".to_string();
        failed_test.failure.as_mut().unwrap().message =
            "Cargo test exited unsuccessfully".to_string();

        let mut successful_check = failed_check();
        successful_check.operation_id = "supervisor:check:2".to_string();
        successful_check.state = CargoOperationState::Succeeded;
        successful_check.created_unix_ms = 230;
        successful_check.started_unix_ms = Some(231);
        successful_check.finished_unix_ms = Some(240);
        successful_check.failure = None;
        successful_check.result.as_mut().unwrap().success = true;
        successful_check.result.as_mut().unwrap().error_count = 0;
        successful_check
            .result
            .as_mut()
            .unwrap()
            .diagnostics
            .clear();

        let status = compose_status(
            process(ProcessState::Running, "ready"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            vec![failed_test, successful_check],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(status.state, DevelopmentState::TestFailed);
        assert_eq!(status.recovery.action, "fix_failing_tests");
    }

    #[test]
    fn active_rebuild_takes_precedence_over_old_failure() {
        let rebuild = RebuildRestartSnapshot {
            operation_id: "supervisor:rebuild_restart:2".to_string(),
            state: RebuildRestartState::Building,
            created_unix_ms: 300,
            started_unix_ms: Some(301),
            finished_unix_ms: None,
            invocation: CargoInvocation::new(None, None, None, None, None),
            evidence: RebuildRestartEvidence::default(),
            failure: None,
        };
        let status = compose_status(
            process(ProcessState::Stopped, "waiting"),
            true,
            None,
            false,
            SupervisorPermissions::full(),
            vec![failed_check()],
            vec![rebuild],
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(status.state, DevelopmentState::RebuildInProgress);
        assert_eq!(status.recovery.tool.as_deref(), Some("operation_status"));
        assert_eq!(
            status.active_operation.unwrap().stage.as_deref(),
            Some("building")
        );
    }

    #[test]
    fn rebuild_launch_failure_surfaces_process_evidence() {
        let rebuild = RebuildRestartSnapshot {
            operation_id: "supervisor:rebuild_restart:3".to_string(),
            state: RebuildRestartState::Failed,
            created_unix_ms: 400,
            started_unix_ms: Some(401),
            finished_unix_ms: Some(450),
            invocation: CargoInvocation::new(None, None, None, None, None),
            evidence: RebuildRestartEvidence::default(),
            failure: Some(RebuildRestartFailure {
                code: "PROCESS_EXITED_DURING_STARTUP".to_string(),
                message: "replacement exited before host readiness".to_string(),
                stage: "launch".to_string(),
                details: Value::Null,
            }),
        };
        let stderr = vec![ProcessLogEntry {
            sequence: 1,
            stream: "stderr".to_string(),
            text: "panic in startup".to_string(),
        }];
        let mut crashed = process(ProcessState::Crashed, "waiting");
        crashed.exited_unix_ms = Some(450);
        let status = compose_status(
            crashed,
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
        assert_eq!(status.recovery.tool.as_deref(), Some("process_evidence"));
        assert_eq!(
            status.last_failure.unwrap().stderr_tail[0].text,
            "panic in startup"
        );
    }
}
