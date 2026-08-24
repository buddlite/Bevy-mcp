use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cargo_executor::{
    CargoError, CargoExecutor, CargoInvocation, CargoOperationSnapshot, CargoOperationState,
};
use crate::permissions::SupervisorPermissions;
use crate::process_manager::{ProcessError, ProcessManager, ProcessOwnership, ProcessSnapshot, ProcessState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RebuildRestartState {
    Queued,
    Checking,
    Stopping,
    Building,
    Launching,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl RebuildRestartState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RebuildRestartFailure {
    pub code: String,
    pub message: String,
    pub stage: String,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebuildRestartEvidence {
    pub initial_process: Option<ProcessSnapshot>,
    pub check: Option<CargoOperationSnapshot>,
    pub stopped_process: Option<ProcessSnapshot>,
    pub build: Option<CargoOperationSnapshot>,
    pub executable: Option<String>,
    pub launched_process: Option<ProcessSnapshot>,
}

impl Default for RebuildRestartEvidence {
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

#[derive(Debug, Clone, Serialize)]
pub struct RebuildRestartSnapshot {
    pub operation_id: String,
    pub state: RebuildRestartState,
    pub created_unix_ms: u128,
    pub started_unix_ms: Option<u128>,
    pub finished_unix_ms: Option<u128>,
    pub invocation: CargoInvocation,
    pub evidence: RebuildRestartEvidence,
    pub failure: Option<RebuildRestartFailure>,
}

#[derive(Debug, Clone)]
pub struct RebuildRestartError {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

impl RebuildRestartError {
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

#[derive(Debug, Clone)]
struct RebuildRestartRecord {
    snapshot: RebuildRestartSnapshot,
    cancel_requested: bool,
    current_cargo_operation_id: Option<String>,
}

struct Inner {
    manager: ProcessManager,
    cargo: CargoExecutor,
    permissions: SupervisorPermissions,
    operations: Mutex<HashMap<String, RebuildRestartRecord>>,
    active_operation: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct RebuildRestartCoordinator {
    inner: Arc<Inner>,
}

impl RebuildRestartCoordinator {
    pub fn new(
        manager: ProcessManager,
        cargo: CargoExecutor,
        permissions: SupervisorPermissions,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                manager,
                cargo,
                permissions,
                operations: Mutex::new(HashMap::new()),
                active_operation: Mutex::new(None),
            }),
        }
    }

    pub fn start(
        &self,
        invocation: CargoInvocation,
    ) -> Result<RebuildRestartSnapshot, RebuildRestartError> {
        self.check_permissions()?;
        if self
            .inner
            .cargo
            .status(None)
            .map_err(RebuildRestartError::from_cargo)?
            .iter()
            .any(|snapshot| !cargo_terminal(snapshot.state))
        {
            return Err(RebuildRestartError::new(
                "CARGO_OPERATION_IN_PROGRESS",
                "A Cargo operation is already active; rebuild_restart requires exclusive Cargo ownership",
            ));
        }

        let operation_id = format!("supervisor:rebuild_restart:{}", Uuid::new_v4());
        {
            let mut active = self.inner.active_operation.lock().unwrap();
            if let Some(active_id) = active.as_ref() {
                return Err(RebuildRestartError::new(
                    "REBUILD_RESTART_IN_PROGRESS",
                    "Another rebuild_restart operation is already active",
                )
                .with_details(json!({ "active_operation_id": active_id })));
            }
            *active = Some(operation_id.clone());
        }

        let snapshot = RebuildRestartSnapshot {
            operation_id: operation_id.clone(),
            state: RebuildRestartState::Queued,
            created_unix_ms: now_ms(),
            started_unix_ms: None,
            finished_unix_ms: None,
            invocation: invocation.clone(),
            evidence: RebuildRestartEvidence::default(),
            failure: None,
        };
        self.inner.operations.lock().unwrap().insert(
            operation_id.clone(),
            RebuildRestartRecord {
                snapshot: snapshot.clone(),
                cancel_requested: false,
                current_cargo_operation_id: None,
            },
        );

        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.run(operation_id, invocation).await;
        });
        Ok(snapshot)
    }

    pub fn status(
        &self,
        operation_id: Option<&str>,
    ) -> Result<Vec<RebuildRestartSnapshot>, RebuildRestartError> {
        let operations = self.inner.operations.lock().unwrap();
        if let Some(operation_id) = operation_id {
            let record = operations.get(operation_id).ok_or_else(|| {
                RebuildRestartError::new(
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

    pub async fn cancel(
        &self,
        operation_id: &str,
    ) -> Result<RebuildRestartSnapshot, RebuildRestartError> {
        let current_cargo = {
            let mut operations = self.inner.operations.lock().unwrap();
            let record = operations.get_mut(operation_id).ok_or_else(|| {
                RebuildRestartError::new(
                    "OPERATION_NOT_FOUND",
                    format!("Unknown supervisor operation '{operation_id}'"),
                )
            })?;
            if record.snapshot.state.is_terminal() {
                return Ok(record.snapshot.clone());
            }
            record.cancel_requested = true;
            record.snapshot.state = RebuildRestartState::Cancelling;
            record.current_cargo_operation_id.clone()
        };

        if let Some(cargo_id) = current_cargo {
            let _ = self.inner.cargo.cancel(&cargo_id).await;
        }
        Ok(self
            .status(Some(operation_id))?
            .into_iter()
            .next()
            .expect("specific operation status returns one snapshot"))
    }

    async fn run(&self, operation_id: String, invocation: CargoInvocation) {
        self.update(&operation_id, |record| {
            record.snapshot.started_unix_ms = Some(now_ms());
            record.snapshot.state = RebuildRestartState::Checking;
        });

        let initial_process = self.inner.manager.status().await;
        self.update(&operation_id, |record| {
            record.snapshot.evidence.initial_process = Some(initial_process.clone());
        });
        if initial_process.ownership == ProcessOwnership::External {
            self.finish_failure(
                &operation_id,
                "PROCESS_NOT_MANAGED",
                "The connected game is externally owned and rebuild_restart will not stop it",
                "preflight",
                json!({ "process": initial_process }),
            );
            return;
        }

        let check = match self.start_and_wait_cargo(&operation_id, false, invocation.clone()).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.finish_error(&operation_id, "check", error);
                return;
            }
        };
        self.update(&operation_id, |record| {
            record.snapshot.evidence.check = Some(check.clone());
        });
        if self.cancel_requested(&operation_id) || check.state == CargoOperationState::Cancelled {
            self.finish_cancelled(&operation_id, "Cancelled while checking the project");
            return;
        }
        if check.state != CargoOperationState::Succeeded {
            self.finish_cargo_failure(&operation_id, "check", &check);
            return;
        }

        if self.cancel_requested(&operation_id) {
            self.finish_cancelled(&operation_id, "Cancelled before stopping the current game");
            return;
        }

        let before_stop = self.inner.manager.status().await;
        if before_stop.ownership == ProcessOwnership::External {
            self.finish_failure(
                &operation_id,
                "PROCESS_NOT_MANAGED",
                "Game ownership changed to external during rebuild_restart",
                "stop",
                json!({ "process": before_stop }),
            );
            return;
        }
        if before_stop.ownership == ProcessOwnership::Managed
            && matches!(
                before_stop.state,
                ProcessState::Running | ProcessState::Starting | ProcessState::Stopping
            )
        {
            self.update(&operation_id, |record| {
                record.snapshot.state = RebuildRestartState::Stopping;
            });
            match self.inner.manager.stop().await {
                Ok(stopped) => self.update(&operation_id, |record| {
                    record.snapshot.evidence.stopped_process = Some(stopped);
                }),
                Err(error) => {
                    self.finish_process_failure(&operation_id, "stop", error);
                    return;
                }
            }
        }

        if self.cancel_requested(&operation_id) {
            self.finish_cancelled(
                &operation_id,
                "Cancelled after the current game stopped; no build was started",
            );
            return;
        }

        self.update(&operation_id, |record| {
            record.snapshot.state = RebuildRestartState::Building;
        });
        let build = match self.start_and_wait_cargo(&operation_id, true, invocation).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.finish_error(&operation_id, "build", error);
                return;
            }
        };
        self.update(&operation_id, |record| {
            record.snapshot.evidence.build = Some(build.clone());
        });
        if self.cancel_requested(&operation_id) || build.state == CargoOperationState::Cancelled {
            self.finish_cancelled(&operation_id, "Cancelled while building the replacement game");
            return;
        }
        if build.state != CargoOperationState::Succeeded {
            self.finish_cargo_failure(&operation_id, "build", &build);
            return;
        }

        let executable = match build
            .result
            .as_ref()
            .and_then(|result| result.executable.clone())
        {
            Some(executable) => executable,
            None => {
                self.finish_failure(
                    &operation_id,
                    "BUILD_ARTIFACT_MISSING",
                    "Cargo completed successfully but did not report an executable artifact",
                    "build",
                    json!({ "build": build }),
                );
                return;
            }
        };
        if !PathBuf::from(&executable).is_file() {
            self.finish_failure(
                &operation_id,
                "BUILD_ARTIFACT_MISSING",
                "Cargo-reported executable artifact does not exist",
                "build",
                json!({ "executable": executable, "build": build }),
            );
            return;
        }
        self.update(&operation_id, |record| {
            record.snapshot.evidence.executable = Some(executable.clone());
        });

        if self.cancel_requested(&operation_id) {
            self.finish_cancelled(
                &operation_id,
                "Cancelled after build; replacement game was not launched",
            );
            return;
        }

        self.update(&operation_id, |record| {
            record.snapshot.state = RebuildRestartState::Launching;
        });
        let launched = match self
            .inner
            .manager
            .launch_artifact(PathBuf::from(&executable))
            .await
        {
            Ok(process) => process,
            Err(error) => {
                self.finish_process_failure(&operation_id, "launch", error);
                return;
            }
        };
        self.update(&operation_id, |record| {
            record.snapshot.evidence.launched_process = Some(launched.clone());
        });

        if self.cancel_requested(&operation_id) {
            let _ = self.inner.manager.stop().await;
            self.finish_cancelled(
                &operation_id,
                "Cancelled during replacement startup; replacement game was stopped",
            );
            return;
        }

        if let Some(initial) = self
            .status(Some(&operation_id))
            .ok()
            .and_then(|mut snapshots| snapshots.pop())
            .and_then(|snapshot| snapshot.evidence.initial_process)
        {
            if initial.instance_id.is_some() && initial.instance_id == launched.instance_id {
                self.finish_failure(
                    &operation_id,
                    "REBUILD_IDENTITY_NOT_ROTATED",
                    "Replacement process reused the previous instance_id",
                    "launch",
                    json!({ "initial": initial, "launched": launched }),
                );
                return;
            }
            if initial.connection_id.is_some() && initial.connection_id == launched.connection_id {
                self.finish_failure(
                    &operation_id,
                    "REBUILD_IDENTITY_NOT_ROTATED",
                    "Replacement process reused the previous connection_id",
                    "launch",
                    json!({ "initial": initial, "launched": launched }),
                );
                return;
            }
        }

        self.update(&operation_id, |record| {
            record.snapshot.state = RebuildRestartState::Succeeded;
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.failure = None;
        });
        self.clear_active(&operation_id);
    }

    async fn start_and_wait_cargo(
        &self,
        operation_id: &str,
        build: bool,
        invocation: CargoInvocation,
    ) -> Result<CargoOperationSnapshot, RebuildRestartError> {
        let started = if build {
            self.inner.cargo.start_build(invocation)
        } else {
            self.inner.cargo.start_check(invocation)
        }
        .map_err(RebuildRestartError::from_cargo)?;
        self.update(operation_id, |record| {
            record.current_cargo_operation_id = Some(started.operation_id.clone());
        });

        loop {
            if self.cancel_requested(operation_id) {
                let _ = self.inner.cargo.cancel(&started.operation_id).await;
            }
            let snapshot = self
                .inner
                .cargo
                .status(Some(&started.operation_id))
                .map_err(RebuildRestartError::from_cargo)?
                .into_iter()
                .next()
                .expect("specific Cargo operation status returns one snapshot");
            if cargo_terminal(snapshot.state) {
                self.update(operation_id, |record| {
                    record.current_cargo_operation_id = None;
                });
                return Ok(snapshot);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn check_permissions(&self) -> Result<(), RebuildRestartError> {
        let p = self.inner.permissions;
        if p.cargo_check && p.cargo_build && p.process_launch && p.process_stop {
            Ok(())
        } else {
            Err(RebuildRestartError::new(
                "SUPERVISOR_PERMISSION_DENIED",
                "rebuild_restart requires cargo_check, cargo_build, process_stop, and process_launch permissions",
            )
            .with_details(json!({
                "cargo_check": p.cargo_check,
                "cargo_build": p.cargo_build,
                "process_stop": p.process_stop,
                "process_launch": p.process_launch,
            })))
        }
    }

    fn finish_cargo_failure(
        &self,
        operation_id: &str,
        stage: &str,
        snapshot: &CargoOperationSnapshot,
    ) {
        let (code, message) = snapshot
            .failure
            .as_ref()
            .map(|failure| (failure.code.clone(), failure.message.clone()))
            .unwrap_or_else(|| (
                "BUILD_FAILED".to_string(),
                format!("Cargo {stage} did not succeed"),
            ));
        self.finish_failure(
            operation_id,
            &code,
            &message,
            stage,
            json!({ "cargo_operation": snapshot }),
        );
    }

    fn finish_process_failure(&self, operation_id: &str, stage: &str, error: ProcessError) {
        let mut details = error.details.clone();
        if !details.is_object() {
            details = json!({ "process_error_details": details });
        }
        if let Some(object) = details.as_object_mut() {
            object.insert(
                "process".to_string(),
                serde_json::to_value(
                    tokio::runtime::Handle::current().block_on(self.inner.manager.status()),
                )
                .unwrap_or(Value::Null),
            );
            object.insert(
                "stderr_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stderr"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "stdout_tail".to_string(),
                serde_json::to_value(self.inner.manager.logs(Some("stdout"), 50).unwrap_or_default())
                    .unwrap_or(Value::Null),
            );
        }
        self.finish_failure(operation_id, error.code, &error.message, stage, details);
    }

    fn finish_error(&self, operation_id: &str, stage: &str, error: RebuildRestartError) {
        self.finish_failure(operation_id, error.code, &error.message, stage, error.details);
    }

    fn finish_failure(
        &self,
        operation_id: &str,
        code: &str,
        message: &str,
        stage: &str,
        details: Value,
    ) {
        self.update(operation_id, |record| {
            record.snapshot.state = RebuildRestartState::Failed;
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.failure = Some(RebuildRestartFailure {
                code: code.to_string(),
                message: message.to_string(),
                stage: stage.to_string(),
                details,
            });
            record.current_cargo_operation_id = None;
        });
        self.clear_active(operation_id);
    }

    fn finish_cancelled(&self, operation_id: &str, message: &str) {
        self.update(operation_id, |record| {
            record.snapshot.state = RebuildRestartState::Cancelled;
            record.snapshot.finished_unix_ms = Some(now_ms());
            record.snapshot.failure = Some(RebuildRestartFailure {
                code: "REBUILD_RESTART_CANCELLED".to_string(),
                message: message.to_string(),
                stage: "cancel".to_string(),
                details: json!({}),
            });
            record.current_cargo_operation_id = None;
        });
        self.clear_active(operation_id);
    }

    fn update(&self, operation_id: &str, update: impl FnOnce(&mut RebuildRestartRecord)) {
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

impl RebuildRestartError {
    fn from_cargo(error: CargoError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            details: error.details,
        }
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
