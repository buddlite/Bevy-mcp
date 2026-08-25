from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected strict-clippy anchor missing in {path}: {old[:140]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# Cargo executor: avoid needless generic borrowing and package failure metadata
# into the existing typed failure value so finish_failure stays below Clippy's
# argument-count threshold.
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''            .current_dir(
                &self
                    .project()
                    .map(|project| project.workspace_root.clone())
                    .unwrap_or_else(|_| self.inner.config.project_dir.clone()),
            )
''',
    '''            .current_dir(
                self.project()
                    .map(|project| project.workspace_root.clone())
                    .unwrap_or_else(|_| self.inner.config.project_dir.clone()),
            )
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''                self.finish_failure(
                    &operation_id,
                    &prepared,
                    started.elapsed(),
                    None,
                    output,
                    code,
                    format!("Failed to start Cargo: {error}"),
                );
''',
    '''                self.finish_failure(
                    &operation_id,
                    &prepared,
                    started.elapsed(),
                    None,
                    output,
                    CargoOperationFailure {
                        code: code.to_string(),
                        message: format!("Failed to start Cargo: {error}"),
                    },
                );
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''                    self.finish_failure(
                        &operation_id,
                        &prepared,
                        started.elapsed(),
                        None,
                        output.clone(),
                        prepared.kind.failure_code(),
                        error,
                    );
''',
    '''                    self.finish_failure(
                        &operation_id,
                        &prepared,
                        started.elapsed(),
                        None,
                        output.clone(),
                        CargoOperationFailure {
                            code: prepared.kind.failure_code().to_string(),
                            message: error,
                        },
                    );
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''            self.finish_failure(
                &operation_id,
                &prepared,
                started.elapsed(),
                status,
                output,
                prepared.kind.timeout_code(),
                format!(
                    "Cargo {} exceeded the {:.1}s operation timeout",
                    prepared.kind.command(),
                    prepared.timeout.as_secs_f64()
                ),
            );
''',
    '''            self.finish_failure(
                &operation_id,
                &prepared,
                started.elapsed(),
                status,
                output,
                CargoOperationFailure {
                    code: prepared.kind.timeout_code().to_string(),
                    message: format!(
                        "Cargo {} exceeded the {:.1}s operation timeout",
                        prepared.kind.command(),
                        prepared.timeout.as_secs_f64()
                    ),
                },
            );
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''            self.finish_failure(
                &operation_id,
                &prepared,
                started.elapsed(),
                status,
                output,
                prepared.kind.failure_code(),
                format!("Cargo {} exited unsuccessfully", prepared.kind.command()),
            );
''',
    '''            self.finish_failure(
                &operation_id,
                &prepared,
                started.elapsed(),
                status,
                output,
                CargoOperationFailure {
                    code: prepared.kind.failure_code().to_string(),
                    message: format!("Cargo {} exited unsuccessfully", prepared.kind.command()),
                },
            );
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/cargo_executor.rs",
    '''    fn finish_failure(
        &self,
        operation_id: &str,
        prepared: &PreparedOperation,
        duration: Duration,
        status: Option<ExitStatus>,
        output: Arc<Mutex<OutputAccumulator>>,
        code: &'static str,
        message: String,
    ) {
        let timed_out = code == prepared.kind.timeout_code();
        let result = output
            .lock()
            .unwrap()
            .result(false, status, duration, &prepared.selected);
        self.update_operation(operation_id, |record| {
            record.snapshot.state = if timed_out {
                CargoOperationState::TimedOut
            } else {
                CargoOperationState::Failed
            };
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.result = Some(result);
            record.snapshot.failure = Some(CargoOperationFailure {
                code: code.to_string(),
                message,
            });
        });
        self.clear_active(operation_id);
    }
''',
    '''    fn finish_failure(
        &self,
        operation_id: &str,
        prepared: &PreparedOperation,
        duration: Duration,
        status: Option<ExitStatus>,
        output: Arc<Mutex<OutputAccumulator>>,
        failure: CargoOperationFailure,
    ) {
        let timed_out = failure.code == prepared.kind.timeout_code();
        let result = output
            .lock()
            .unwrap()
            .result(false, status, duration, &prepared.selected);
        self.update_operation(operation_id, |record| {
            record.snapshot.state = if timed_out {
                CargoOperationState::TimedOut
            } else {
                CargoOperationState::Failed
            };
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.result = Some(result);
            record.snapshot.failure = Some(failure);
        });
        self.clear_active(operation_id);
    }
''',
)

# Development status: prefer explicit conditionals for optional evidence and the
# process-readiness timestamp introduced by the cleanup's supersession logic.
replace_once(
    "crates/bevy-mcp-supervisor/src/development_status.rs",
    '''        stdout_tail: process_evidence
            .then(|| stdout_tail.to_vec())
            .unwrap_or_default(),
        stderr_tail: process_evidence
            .then(|| stderr_tail.to_vec())
            .unwrap_or_default(),
''',
    '''        stdout_tail: if process_evidence {
            stdout_tail.to_vec()
        } else {
            Vec::new()
        },
        stderr_tail: if process_evidence {
            stderr_tail.to_vec()
        } else {
            Vec::new()
        },
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/development_status.rs",
    '''    let latest_ready_process = (process.state == ProcessState::Running && process.host == "ready")
        .then_some(process.started_unix_ms.unwrap_or_default())
        .unwrap_or_default();
''',
    '''    let latest_ready_process = if process.state == ProcessState::Running && process.host == "ready" {
        process.started_unix_ms.unwrap_or_default()
    } else {
        0
    };
''',
)

# Capability merge has accumulated supervisor state over several stages. Group it
# into one typed context so the helper stays readable and extensible.
replace_once(
    "crates/bevy-mcp-supervisor/src/process_tools.rs",
    '''pub(crate) fn merge_supervisor_capabilities(
    mut host: Value,
    connected: bool,
    ready: bool,
    instance_id: Option<String>,
    connection_id: Option<String>,
    process: &ProcessSnapshot,
    configured_launch_target: bool,
    cargo_available: bool,
    permissions: SupervisorPermissions,
    cargo_error: Option<CargoError>,
    host_error: Option<Value>,
) -> Value {
''',
    '''pub(crate) struct SupervisorCapabilityContext<'a> {
    pub(crate) connected: bool,
    pub(crate) ready: bool,
    pub(crate) instance_id: Option<String>,
    pub(crate) connection_id: Option<String>,
    pub(crate) process: &'a ProcessSnapshot,
    pub(crate) configured_launch_target: bool,
    pub(crate) cargo_available: bool,
    pub(crate) permissions: SupervisorPermissions,
    pub(crate) cargo_error: Option<CargoError>,
    pub(crate) host_error: Option<Value>,
}

pub(crate) fn merge_supervisor_capabilities(
    mut host: Value,
    context: SupervisorCapabilityContext<'_>,
) -> Value {
    let SupervisorCapabilityContext {
        connected,
        ready,
        instance_id,
        connection_id,
        process,
        configured_launch_target,
        cargo_available,
        permissions,
        cargo_error,
        host_error,
    } = context;
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/process_tools.rs",
    '''        merge_supervisor_capabilities(
            host,
            backend_status.connected,
            backend_status.ready,
            backend_status.instance_id,
            backend_status.connection_id,
            &process,
            self.manager.has_configured_launch_target(),
            self.cargo.available(),
            self.permissions,
            self.cargo.initialization_error(),
            host_error,
        )
''',
    '''        merge_supervisor_capabilities(
            host,
            SupervisorCapabilityContext {
                connected: backend_status.connected,
                ready: backend_status.ready,
                instance_id: backend_status.instance_id,
                connection_id: backend_status.connection_id,
                process: &process,
                configured_launch_target: self.manager.has_configured_launch_target(),
                cargo_available: self.cargo.available(),
                permissions: self.permissions,
                cargo_error: self.cargo.initialization_error(),
                host_error,
            },
        )
''',
)
# repo_cleanup_apply.py renames stage4_acceptance.rs before this script runs.
replace_once(
    "crates/bevy-mcp-supervisor/src/supervisor_acceptance.rs",
    "use crate::process_tools::merge_supervisor_capabilities;",
    "use crate::process_tools::{SupervisorCapabilityContext, merge_supervisor_capabilities};",
)
replace_once(
    "crates/bevy-mcp-supervisor/src/supervisor_acceptance.rs",
    '''    let merged = merge_supervisor_capabilities(
        host,
        false,
        false,
        Some("run-supervisor".into()),
        None,
        &process,
        false,
        true,
        SupervisorPermissions::full(),
        None,
        None,
    );
''',
    '''    let merged = merge_supervisor_capabilities(
        host,
        SupervisorCapabilityContext {
            connected: false,
            ready: false,
            instance_id: Some("run-supervisor".into()),
            connection_id: None,
            process: &process,
            configured_launch_target: false,
            cargo_available: true,
            permissions: SupervisorPermissions::full(),
            cargo_error: None,
            host_error: None,
        },
    );
''',
)

# Evidence is an all-optional data holder, so Default is exactly derivable.
replace_once(
    "crates/bevy-mcp-supervisor/src/rebuild_restart.rs",
    '''#[derive(Debug, Clone, Serialize)]
pub struct RebuildRestartEvidence {
''',
    '''#[derive(Debug, Clone, Serialize, Default)]
pub struct RebuildRestartEvidence {
''',
)
replace_once(
    "crates/bevy-mcp-supervisor/src/rebuild_restart.rs",
    '''impl Default for RebuildRestartEvidence {
    fn default() -> Self {
        Self {
            initial_process: None,
            check: None,
            stopped_process: None,
            build: None,
            executable: None,
            launched_process: None,
        }
    }
}

''',
    "",
)

# The managed-process fixture reads until EOF; express that directly.
replace_once(
    "crates/bevy-mcp-supervisor/src/process_manager.rs",
    '''        let mut crash_armed = false;
        loop {
            let envelope = match read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
                Ok(envelope) => envelope,
                Err(_) => break,
            };
            match envelope.message {
''',
    '''        let mut crash_armed = false;
        while let Ok(envelope) = read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
            match envelope.message {
''',
)
