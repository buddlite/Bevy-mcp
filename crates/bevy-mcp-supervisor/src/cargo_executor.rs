use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use command_group::{AsyncCommandGroup, AsyncGroupChild};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::permissions::SupervisorPermissions;

#[derive(Debug, Clone)]
pub struct CargoExecutorConfig {
    pub project_dir: PathBuf,
    pub check_timeout: Duration,
    pub build_timeout: Duration,
    pub test_timeout: Duration,
    pub poll_interval: Duration,
    pub output_tail_lines: usize,
    pub max_diagnostics: usize,
    pub permissions: SupervisorPermissions,
}

impl CargoExecutorConfig {
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
            ..Default::default()
        }
    }
}

impl Default for CargoExecutorConfig {
    fn default() -> Self {
        Self {
            project_dir: PathBuf::from("."),
            check_timeout: Duration::from_secs(120),
            build_timeout: Duration::from_secs(300),
            test_timeout: Duration::from_secs(300),
            poll_interval: Duration::from_millis(20),
            output_tail_lines: 200,
            max_diagnostics: 200,
            permissions: SupervisorPermissions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoOperationKind {
    Check,
    Build,
    Test,
}

impl CargoOperationKind {
    fn id_segment(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
        }
    }

    fn failure_code(self) -> &'static str {
        match self {
            Self::Check | Self::Build => "BUILD_FAILED",
            Self::Test => "TEST_FAILED",
        }
    }

    fn timeout_code(self) -> &'static str {
        match self {
            Self::Check | Self::Build => "BUILD_TIMEOUT",
            Self::Test => "TEST_TIMEOUT",
        }
    }

    fn cancelled_code(self) -> &'static str {
        match self {
            Self::Check | Self::Build => "BUILD_CANCELLED",
            Self::Test => "TEST_CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoOperationState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl CargoOperationState {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoInvocation {
    pub package: Option<String>,
    pub bin: Option<String>,
    pub profile: String,
    pub features: Vec<String>,
    pub filter: Option<String>,
}

impl CargoInvocation {
    pub fn new(
        package: Option<String>,
        bin: Option<String>,
        profile: Option<String>,
        features: Option<Vec<String>>,
        filter: Option<String>,
    ) -> Self {
        Self {
            package,
            bin,
            profile: profile.unwrap_or_else(|| "dev".to_string()),
            features: features.unwrap_or_default(),
            filter,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoSpan {
    pub file_name: String,
    pub line_start: u64,
    pub line_end: u64,
    pub column_start: u64,
    pub column_end: u64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoDiagnostic {
    pub level: String,
    pub message: String,
    pub code: Option<String>,
    pub rendered: Option<String>,
    pub spans: Vec<CargoSpan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoRunResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub warning_count: u64,
    pub error_count: u64,
    pub diagnostics: Vec<CargoDiagnostic>,
    pub executable: Option<String>,
    pub package: String,
    pub bin: String,
    pub profile: String,
    pub features: Vec<String>,
    pub raw_output_tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoOperationFailure {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoOperationSnapshot {
    pub operation_id: String,
    pub kind: CargoOperationKind,
    pub state: CargoOperationState,
    pub created_unix_ms: u128,
    pub started_unix_ms: Option<u128>,
    pub finished_unix_ms: Option<u128>,
    pub invocation: CargoInvocation,
    pub result: Option<CargoRunResult>,
    pub failure: Option<CargoOperationFailure>,
}

#[derive(Debug, Clone)]
pub struct CargoError {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

impl CargoError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: json!({}),
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "error": self.code,
            "message": self.message,
        });
        if let (Some(object), Some(details)) = (value.as_object_mut(), self.details.as_object()) {
            for (key, value) in details {
                object.insert(key.clone(), value.clone());
            }
        }
        value
    }
}

#[derive(Debug, Deserialize)]
struct MetadataDocument {
    workspace_root: String,
    packages: Vec<MetadataPackage>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    features: HashMap<String, Vec<String>>,
    targets: Vec<MetadataTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
}

impl MetadataTarget {
    fn is_binary(&self) -> bool {
        self.kind.iter().any(|kind| kind == "bin")
    }
}

#[derive(Debug, Clone)]
struct ProjectModel {
    manifest_path: PathBuf,
    workspace_root: PathBuf,
    packages: Vec<MetadataPackage>,
}

#[derive(Debug, Clone)]
struct SelectedTarget {
    package: String,
    bin: String,
    profile: String,
    features: Vec<String>,
}

#[derive(Debug, Clone)]
struct PreparedOperation {
    kind: CargoOperationKind,
    invocation: CargoInvocation,
    selected: SelectedTarget,
    args: Vec<String>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
struct CargoOperationRecord {
    snapshot: CargoOperationSnapshot,
    cancel_requested: bool,
}

struct ManagedCargoChild {
    operation_id: String,
    child: AsyncGroupChild,
}

struct OutputAccumulator {
    tail_capacity: usize,
    max_diagnostics: usize,
    raw_tail: VecDeque<String>,
    diagnostics: Vec<CargoDiagnostic>,
    warning_count: u64,
    error_count: u64,
    executable: Option<String>,
}

impl OutputAccumulator {
    fn new(tail_capacity: usize, max_diagnostics: usize) -> Self {
        Self {
            tail_capacity: tail_capacity.max(1),
            max_diagnostics: max_diagnostics.max(1),
            raw_tail: VecDeque::new(),
            diagnostics: Vec::new(),
            warning_count: 0,
            error_count: 0,
            executable: None,
        }
    }

    fn push_raw(&mut self, stream: &str, line: &str) {
        self.raw_tail.push_back(format!("[{stream}] {line}"));
        while self.raw_tail.len() > self.tail_capacity {
            self.raw_tail.pop_front();
        }
    }

    fn observe_stdout(&mut self, line: &str) {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            match value.get("reason").and_then(Value::as_str) {
                Some("compiler-message") => self.observe_compiler_message(&value),
                Some("compiler-artifact") => {
                    if let Some(executable) = value.get("executable").and_then(Value::as_str) {
                        self.executable = Some(executable.to_string());
                    }
                }
                _ => {}
            }
        }
        self.push_raw("stdout", line);
    }

    fn observe_compiler_message(&mut self, value: &Value) {
        let Some(message) = value.get("message") else {
            return;
        };
        let level = message
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        match level.as_str() {
            "warning" => self.warning_count += 1,
            "error" => self.error_count += 1,
            _ => {}
        }
        if self.diagnostics.len() >= self.max_diagnostics {
            return;
        }
        let code = message
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let spans = message
            .get("spans")
            .and_then(Value::as_array)
            .map(|spans| {
                spans
                    .iter()
                    .filter_map(|span| {
                        Some(CargoSpan {
                            file_name: span.get("file_name")?.as_str()?.to_string(),
                            line_start: span.get("line_start")?.as_u64()?,
                            line_end: span.get("line_end")?.as_u64()?,
                            column_start: span.get("column_start")?.as_u64()?,
                            column_end: span.get("column_end")?.as_u64()?,
                            is_primary: span
                                .get("is_primary")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.diagnostics.push(CargoDiagnostic {
            level,
            message: message
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            code,
            rendered: message
                .get("rendered")
                .and_then(Value::as_str)
                .map(str::to_owned),
            spans,
        });
    }

    fn result(
        &self,
        success: bool,
        status: Option<ExitStatus>,
        duration: Duration,
        selected: &SelectedTarget,
    ) -> CargoRunResult {
        CargoRunResult {
            success,
            exit_code: status.as_ref().and_then(|status| status.code()),
            duration_ms: duration.as_millis(),
            warning_count: self.warning_count,
            error_count: self.error_count,
            diagnostics: self.diagnostics.clone(),
            executable: self.executable.clone(),
            package: selected.package.clone(),
            bin: selected.bin.clone(),
            profile: selected.profile.clone(),
            features: selected.features.clone(),
            raw_output_tail: self.raw_tail.iter().cloned().collect(),
        }
    }
}

struct Inner {
    config: CargoExecutorConfig,
    project: Option<ProjectModel>,
    init_error: Option<CargoError>,
    operations: Mutex<HashMap<String, CargoOperationRecord>>,
    active_operation: Mutex<Option<String>>,
    child: AsyncMutex<Option<ManagedCargoChild>>,
}

#[derive(Clone)]
pub struct CargoExecutor {
    inner: Arc<Inner>,
}

impl CargoExecutor {
    pub async fn initialize(config: CargoExecutorConfig) -> Self {
        let discovery = discover_project(&config.project_dir).await;
        let (project, init_error) = match discovery {
            Ok(project) => (Some(project), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            inner: Arc::new(Inner {
                config,
                project,
                init_error,
                operations: Mutex::new(HashMap::new()),
                active_operation: Mutex::new(None),
                child: AsyncMutex::new(None),
            }),
        }
    }

    pub fn available(&self) -> bool {
        self.inner.project.is_some()
    }

    pub fn initialization_error(&self) -> Option<CargoError> {
        self.inner.init_error.clone()
    }

    pub fn permissions(&self) -> SupervisorPermissions {
        self.inner.config.permissions
    }

    pub fn start_check(
        &self,
        invocation: CargoInvocation,
    ) -> Result<CargoOperationSnapshot, CargoError> {
        self.start(CargoOperationKind::Check, invocation)
    }

    pub fn start_build(
        &self,
        invocation: CargoInvocation,
    ) -> Result<CargoOperationSnapshot, CargoError> {
        self.start(CargoOperationKind::Build, invocation)
    }

    pub fn start_test(
        &self,
        invocation: CargoInvocation,
    ) -> Result<CargoOperationSnapshot, CargoError> {
        self.start(CargoOperationKind::Test, invocation)
    }

    fn start(
        &self,
        kind: CargoOperationKind,
        invocation: CargoInvocation,
    ) -> Result<CargoOperationSnapshot, CargoError> {
        self.check_permission(kind)?;
        let prepared = self.prepare(kind, invocation)?;
        let operation_id = format!("supervisor:{}:{}", kind.id_segment(), Uuid::new_v4());
        {
            let mut active = self.inner.active_operation.lock().unwrap();
            if let Some(active_id) = active.as_ref() {
                return Err(CargoError::new(
                    "CARGO_OPERATION_IN_PROGRESS",
                    "Another Cargo operation is already running for this supervisor",
                )
                .with_details(json!({ "active_operation_id": active_id })));
            }
            *active = Some(operation_id.clone());
        }

        let snapshot = CargoOperationSnapshot {
            operation_id: operation_id.clone(),
            kind,
            state: CargoOperationState::Queued,
            created_unix_ms: now_ms(),
            started_unix_ms: None,
            finished_unix_ms: None,
            invocation: prepared.invocation.clone(),
            result: None,
            failure: None,
        };
        self.inner.operations.lock().unwrap().insert(
            operation_id.clone(),
            CargoOperationRecord {
                snapshot: snapshot.clone(),
                cancel_requested: false,
            },
        );

        let executor = self.clone();
        tokio::spawn(async move {
            executor.run_operation(operation_id, prepared).await;
        });
        Ok(snapshot)
    }

    pub fn status(
        &self,
        operation_id: Option<&str>,
    ) -> Result<Vec<CargoOperationSnapshot>, CargoError> {
        let operations = self.inner.operations.lock().unwrap();
        if let Some(operation_id) = operation_id {
            let record = operations.get(operation_id).ok_or_else(|| {
                CargoError::new(
                    "OPERATION_NOT_FOUND",
                    format!("Unknown supervisor operation '{operation_id}'"),
                )
            })?;
            return Ok(vec![record.snapshot.clone()]);
        }
        let mut snapshots: Vec<_> = operations
            .values()
            .map(|record| record.snapshot.clone())
            .collect();
        snapshots.sort_by_key(|snapshot| snapshot.created_unix_ms);
        Ok(snapshots)
    }

    pub async fn cancel(&self, operation_id: &str) -> Result<CargoOperationSnapshot, CargoError> {
        {
            let mut operations = self.inner.operations.lock().unwrap();
            let record = operations.get_mut(operation_id).ok_or_else(|| {
                CargoError::new(
                    "OPERATION_NOT_FOUND",
                    format!("Unknown supervisor operation '{operation_id}'"),
                )
            })?;
            if record.snapshot.state.is_terminal() {
                return Ok(record.snapshot.clone());
            }
            record.cancel_requested = true;
            record.snapshot.state = CargoOperationState::Cancelling;
        }

        let mut child = self.inner.child.lock().await;
        if let Some(managed) = child.as_mut() {
            if managed.operation_id == operation_id {
                managed.child.start_kill().map_err(|error| {
                    CargoError::new(
                        "BUILD_CANCEL_FAILED",
                        format!("Failed to terminate Cargo process tree: {error}"),
                    )
                })?;
            }
        }
        drop(child);
        Ok(self
            .status(Some(operation_id))?
            .into_iter()
            .next()
            .expect("specific operation status must return one record"))
    }

    fn check_permission(&self, kind: CargoOperationKind) -> Result<(), CargoError> {
        let permissions = self.inner.config.permissions;
        let allowed = match kind {
            CargoOperationKind::Check => permissions.cargo_check,
            CargoOperationKind::Build => permissions.cargo_build,
            CargoOperationKind::Test => permissions.cargo_test,
        };
        if allowed {
            Ok(())
        } else {
            Err(CargoError::new(
                "SUPERVISOR_PERMISSION_DENIED",
                format!("Cargo {} permission is disabled", kind.command()),
            ))
        }
    }

    fn project(&self) -> Result<&ProjectModel, CargoError> {
        self.inner.project.as_ref().ok_or_else(|| {
            self.inner.init_error.clone().unwrap_or_else(|| {
                CargoError::new(
                    "PROJECT_METADATA_FAILED",
                    "Cargo project metadata is unavailable",
                )
            })
        })
    }

    fn prepare(
        &self,
        kind: CargoOperationKind,
        invocation: CargoInvocation,
    ) -> Result<PreparedOperation, CargoError> {
        let project = self.project()?;
        let profile = match invocation.profile.as_str() {
            "dev" | "release" => invocation.profile.clone(),
            other => {
                return Err(CargoError::new(
                    "INVALID_PROFILE",
                    format!("Unsupported Cargo profile '{other}'; expected 'dev' or 'release'"),
                ));
            }
        };
        let (package, bin) = resolve_target(
            project,
            invocation.package.as_deref(),
            invocation.bin.as_deref(),
        )?;
        for feature in &invocation.features {
            if !package.features.contains_key(feature) {
                return Err(CargoError::new(
                    "FEATURE_UNKNOWN",
                    format!("Unknown Cargo feature '{feature}' for package '{}'; refusing to launch Cargo", package.name),
                )
                .with_details(json!({ "package": package.name, "feature": feature })));
            }
        }

        let selected = SelectedTarget {
            package: package.name.clone(),
            bin: bin.name.clone(),
            profile: profile.clone(),
            features: invocation.features.clone(),
        };
        let mut args = vec![
            kind.command().to_string(),
            "--manifest-path".to_string(),
            project.manifest_path.display().to_string(),
            "--message-format=json-render-diagnostics".to_string(),
            "-p".to_string(),
            package.name.clone(),
            "--bin".to_string(),
            bin.name.clone(),
        ];
        if profile == "release" {
            args.push("--release".to_string());
        }
        if !invocation.features.is_empty() {
            args.push("--features".to_string());
            args.push(invocation.features.join(","));
        }
        if kind == CargoOperationKind::Test {
            if let Some(filter) = invocation.filter.as_ref() {
                if filter.starts_with('-') {
                    return Err(CargoError::new(
                        "INVALID_TEST_FILTER",
                        "test filter must be a test-name filter, not a test-harness flag",
                    ));
                }
                args.push("--".to_string());
                args.push(filter.clone());
            }
        }
        let timeout = match kind {
            CargoOperationKind::Check => self.inner.config.check_timeout,
            CargoOperationKind::Build => self.inner.config.build_timeout,
            CargoOperationKind::Test => self.inner.config.test_timeout,
        };
        Ok(PreparedOperation {
            kind,
            invocation,
            selected,
            args,
            timeout,
        })
    }

    async fn run_operation(&self, operation_id: String, prepared: PreparedOperation) {
        let started = Instant::now();
        self.update_operation(&operation_id, |record| {
            record.snapshot.state = CargoOperationState::Running;
            record.snapshot.started_unix_ms = Some(now_ms());
        });
        if self.cancel_requested(&operation_id) {
            self.finish_cancelled(
                &operation_id,
                &prepared,
                started.elapsed(),
                None,
                Arc::new(Mutex::new(OutputAccumulator::new(
                    self.inner.config.output_tail_lines,
                    self.inner.config.max_diagnostics,
                ))),
            );
            return;
        }

        let output = Arc::new(Mutex::new(OutputAccumulator::new(
            self.inner.config.output_tail_lines,
            self.inner.config.max_diagnostics,
        )));
        let mut command = Command::new("cargo");
        command
            .args(&prepared.args)
            .current_dir(
                &self
                    .project()
                    .map(|project| project.workspace_root.clone())
                    .unwrap_or_else(|_| self.inner.config.project_dir.clone()),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut group = command.group();
        group.kill_on_drop(true);
        let mut child = match group.spawn() {
            Ok(child) => child,
            Err(error) => {
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    "CARGO_NOT_AVAILABLE"
                } else {
                    prepared.kind.failure_code()
                };
                self.finish_failure(
                    &operation_id,
                    &prepared,
                    started.elapsed(),
                    None,
                    output,
                    code,
                    format!("Failed to start Cargo: {error}"),
                );
                return;
            }
        };

        let stdout = child.inner().stdout.take();
        let stderr = child.inner().stderr.take();
        *self.inner.child.lock().await = Some(ManagedCargoChild {
            operation_id: operation_id.clone(),
            child,
        });

        let stdout_task = stdout.map(|stdout| {
            let output = output.clone();
            tokio::spawn(async move { capture_cargo_pipe(stdout, true, output).await })
        });
        let stderr_task = stderr.map(|stderr| {
            let output = output.clone();
            tokio::spawn(async move { capture_cargo_pipe(stderr, false, output).await })
        });

        let mut timed_out = false;
        let status = loop {
            if self.cancel_requested(&operation_id) {
                let _ = self.kill_active_child(&operation_id).await;
            } else if started.elapsed() >= prepared.timeout {
                timed_out = true;
                let _ = self.kill_active_child(&operation_id).await;
            }

            match self.poll_child(&operation_id).await {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(error) => {
                    self.finish_failure(
                        &operation_id,
                        &prepared,
                        started.elapsed(),
                        None,
                        output.clone(),
                        prepared.kind.failure_code(),
                        error,
                    );
                    self.clear_active(&operation_id);
                    return;
                }
            }
            tokio::time::sleep(self.inner.config.poll_interval).await;
        };

        if let Some(task) = stdout_task {
            let _ = task.await;
        }
        if let Some(task) = stderr_task {
            let _ = task.await;
        }

        if timed_out {
            self.finish_failure(
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
        } else if self.cancel_requested(&operation_id) {
            self.finish_cancelled(&operation_id, &prepared, started.elapsed(), status, output);
        } else if status.as_ref().is_some_and(ExitStatus::success) {
            self.finish_success(&operation_id, &prepared, started.elapsed(), status, output);
        } else {
            self.finish_failure(
                &operation_id,
                &prepared,
                started.elapsed(),
                status,
                output,
                prepared.kind.failure_code(),
                format!("Cargo {} exited unsuccessfully", prepared.kind.command()),
            );
        }
    }

    async fn kill_active_child(&self, operation_id: &str) -> Result<(), String> {
        let mut child = self.inner.child.lock().await;
        if let Some(managed) = child.as_mut() {
            if managed.operation_id == operation_id {
                managed
                    .child
                    .start_kill()
                    .map_err(|error| format!("Failed to kill Cargo process tree: {error}"))?;
            }
        }
        Ok(())
    }

    async fn poll_child(&self, operation_id: &str) -> Result<Option<ExitStatus>, String> {
        let mut child = self.inner.child.lock().await;
        let Some(managed) = child.as_mut() else {
            return Ok(None);
        };
        if managed.operation_id != operation_id {
            return Err("Active Cargo child belongs to a different operation".to_string());
        }
        let status = match managed.child.inner().try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return Ok(None),
            Err(error) => return Err(format!("Failed to poll Cargo process: {error}")),
        };
        let _ = managed.child.start_kill();
        managed
            .child
            .wait()
            .await
            .map_err(|error| format!("Failed to drain Cargo process tree: {error}"))?;
        child.take();
        Ok(Some(status))
    }

    fn finish_success(
        &self,
        operation_id: &str,
        prepared: &PreparedOperation,
        duration: Duration,
        status: Option<ExitStatus>,
        output: Arc<Mutex<OutputAccumulator>>,
    ) {
        let result = output
            .lock()
            .unwrap()
            .result(true, status, duration, &prepared.selected);
        self.update_operation(operation_id, |record| {
            record.snapshot.state = CargoOperationState::Succeeded;
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.result = Some(result);
            record.snapshot.failure = None;
        });
        self.clear_active(operation_id);
    }

    fn finish_failure(
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

    fn finish_cancelled(
        &self,
        operation_id: &str,
        prepared: &PreparedOperation,
        duration: Duration,
        status: Option<ExitStatus>,
        output: Arc<Mutex<OutputAccumulator>>,
    ) {
        let result = output
            .lock()
            .unwrap()
            .result(false, status, duration, &prepared.selected);
        self.update_operation(operation_id, |record| {
            record.snapshot.state = CargoOperationState::Cancelled;
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.result = Some(result);
            record.snapshot.failure = Some(CargoOperationFailure {
                code: prepared.kind.cancelled_code().to_string(),
                message: format!("Cargo {} was cancelled", prepared.kind.command()),
            });
        });
        self.clear_active(operation_id);
    }

    fn update_operation(&self, operation_id: &str, update: impl FnOnce(&mut CargoOperationRecord)) {
        if let Some(record) = self.inner.operations.lock().unwrap().get_mut(operation_id) {
            update(record);
        }
    }

    fn cancel_requested(&self, operation_id: &str) -> bool {
        self.inner
            .operations
            .lock()
            .unwrap()
            .get(operation_id)
            .is_some_and(|record| record.cancel_requested)
    }

    fn clear_active(&self, operation_id: &str) {
        let mut active = self.inner.active_operation.lock().unwrap();
        if active.as_deref() == Some(operation_id) {
            *active = None;
        }
    }
}

async fn discover_project(project_dir: &Path) -> Result<ProjectModel, CargoError> {
    let project_dir = if project_dir.is_absolute() {
        project_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| CargoError::new("PROJECT_METADATA_FAILED", error.to_string()))?
            .join(project_dir)
    };
    let manifest_path = project_dir.join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .current_dir(&project_dir)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| {
            CargoError::new(
                "PROJECT_METADATA_FAILED",
                "cargo metadata exceeded the 30 second initialization timeout",
            )
        })?
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                CargoError::new("CARGO_NOT_AVAILABLE", "Cargo executable was not found")
            } else {
                CargoError::new(
                    "PROJECT_METADATA_FAILED",
                    format!("Failed to execute cargo metadata: {error}"),
                )
            }
        })?;
    if !output.status.success() {
        return Err(CargoError::new(
            "PROJECT_METADATA_FAILED",
            "cargo metadata failed for the configured project",
        )
        .with_details(json!({
            "exit_code": output.status.code(),
            "stderr_tail": bounded_text_tail(&output.stderr, 50),
        })));
    }
    let metadata: MetadataDocument = serde_json::from_slice(&output.stdout).map_err(|error| {
        CargoError::new(
            "PROJECT_METADATA_FAILED",
            format!("Failed to parse cargo metadata JSON: {error}"),
        )
    })?;
    Ok(ProjectModel {
        manifest_path,
        workspace_root: PathBuf::from(metadata.workspace_root),
        packages: metadata.packages,
    })
}

fn resolve_target<'a>(
    project: &'a ProjectModel,
    package_name: Option<&str>,
    bin_name: Option<&str>,
) -> Result<(&'a MetadataPackage, &'a MetadataTarget), CargoError> {
    let packages: Vec<&MetadataPackage> = match package_name {
        Some(name) => vec![
            project
                .packages
                .iter()
                .find(|package| package.name == name || package.id == name)
                .ok_or_else(|| {
                    CargoError::new(
                        "TARGET_NOT_FOUND",
                        format!("Cargo package '{name}' was not found in project metadata"),
                    )
                })?,
        ],
        None => project.packages.iter().collect(),
    };
    let mut candidates = Vec::new();
    for package in packages {
        for target in package.targets.iter().filter(|target| target.is_binary()) {
            if bin_name.is_none_or(|name| target.name == name) {
                candidates.push((package, target));
            }
        }
    }
    match candidates.as_slice() {
        [(package, target)] => Ok((*package, *target)),
        [] => Err(CargoError::new(
            "TARGET_NOT_FOUND",
            match bin_name {
                Some(bin) => format!("Cargo binary target '{bin}' was not found"),
                None => {
                    "No Cargo binary target was found for the selected package/project".to_string()
                }
            },
        )),
        _ => Err(CargoError::new(
            "TARGET_AMBIGUOUS",
            "Multiple Cargo binary targets are available; specify package and/or bin explicitly",
        )
        .with_details(json!({
            "candidates": candidates
                .iter()
                .map(|(package, target)| json!({ "package": package.name, "bin": target.name }))
                .collect::<Vec<_>>()
        }))),
    }
}

async fn capture_cargo_pipe<R: AsyncRead + Unpin>(
    reader: R,
    stdout: bool,
    output: Arc<Mutex<OutputAccumulator>>,
) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) => break,
            Ok(_) => {
                while matches!(buffer.last(), Some(b'\n' | b'\r')) {
                    buffer.pop();
                }
                let line = String::from_utf8_lossy(&buffer).into_owned();
                let mut output = output.lock().unwrap();
                if stdout {
                    output.observe_stdout(&line);
                } else {
                    output.push_raw("stderr", &line);
                }
            }
            Err(error) => {
                output
                    .lock()
                    .unwrap()
                    .push_raw("capture", &format!("failed to read Cargo output: {error}"));
                break;
            }
        }
    }
}

fn bounded_text_tail(bytes: &[u8], limit: usize) -> Vec<String> {
    let mut result: Vec<_> = String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_owned)
        .collect();
    let keep = limit.max(1).min(result.len());
    let start = result.len().saturating_sub(keep);
    result.drain(0..start);
    result
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(packages: Vec<MetadataPackage>) -> ProjectModel {
        ProjectModel {
            manifest_path: PathBuf::from("Cargo.toml"),
            workspace_root: PathBuf::from("."),
            packages,
        }
    }

    fn package(name: &str, bins: &[&str], features: &[&str]) -> MetadataPackage {
        MetadataPackage {
            id: name.to_string(),
            name: name.to_string(),
            features: features
                .iter()
                .map(|feature| ((*feature).to_string(), Vec::new()))
                .collect(),
            targets: bins
                .iter()
                .map(|bin| MetadataTarget {
                    name: (*bin).to_string(),
                    kind: vec!["bin".to_string()],
                })
                .collect(),
        }
    }

    #[test]
    fn target_resolution_requires_disambiguation() {
        let project = project(vec![package("game", &["client", "server"], &[])]);
        let error = resolve_target(&project, Some("game"), None).unwrap_err();
        assert_eq!(error.code, "TARGET_AMBIGUOUS");
    }

    #[test]
    fn target_resolution_accepts_explicit_binary() {
        let project = project(vec![package("game", &["client", "server"], &[])]);
        let (package, target) = resolve_target(&project, Some("game"), Some("client")).unwrap();
        assert_eq!(package.name, "game");
        assert_eq!(target.name, "client");
    }

    #[test]
    fn compiler_messages_are_normalized() {
        let mut output = OutputAccumulator::new(20, 20);
        output.observe_stdout(r#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value","code":{"code":"E0425"},"rendered":"error[E0425]","spans":[{"file_name":"src/main.rs","line_start":3,"line_end":3,"column_start":5,"column_end":9,"is_primary":true}]}}"#);
        assert_eq!(output.error_count, 1);
        assert_eq!(output.diagnostics[0].code.as_deref(), Some("E0425"));
        assert_eq!(output.diagnostics[0].spans[0].line_start, 3);
    }
}
