use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use command_group::{AsyncCommandGroup, AsyncGroupChild};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;

use crate::backend::{HostState, SupervisorBackend, TransportState, generate_instance_id};

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl LaunchSpec {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            args: Vec::new(),
            current_dir: None,
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.args.push(value.into());
        self
    }

    pub fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct ProcessManagerConfig {
    pub launch: Option<LaunchSpec>,
    pub ready_timeout: Duration,
    pub graceful_stop_timeout: Duration,
    pub force_stop_timeout: Duration,
    pub poll_interval: Duration,
    pub log_capacity: usize,
}

impl Default for ProcessManagerConfig {
    fn default() -> Self {
        Self {
            launch: None,
            ready_timeout: Duration::from_secs(20),
            graceful_stop_timeout: Duration::from_secs(3),
            force_stop_timeout: Duration::from_secs(3),
            poll_interval: Duration::from_millis(20),
            log_capacity: 1000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Exited,
    Crashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOwnership {
    None,
    Managed,
    External,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessSnapshot {
    pub state: ProcessState,
    pub ownership: ProcessOwnership,
    pub pid: Option<u32>,
    pub instance_id: Option<String>,
    pub connection_id: Option<String>,
    pub transport: String,
    pub host: String,
    pub exit_code: Option<i32>,
    pub executable: Option<String>,
    pub started_unix_ms: Option<u128>,
    pub exited_unix_ms: Option<u128>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessLogEntry {
    pub sequence: u64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ProcessError {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

impl ProcessError {
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
struct ProcessRecord {
    state: ProcessState,
    ownership: ProcessOwnership,
    pid: Option<u32>,
    instance_id: Option<String>,
    exit_code: Option<i32>,
    executable: Option<String>,
    started_unix_ms: Option<u128>,
    exited_unix_ms: Option<u128>,
    last_error: Option<String>,
}

impl Default for ProcessRecord {
    fn default() -> Self {
        Self {
            state: ProcessState::Stopped,
            ownership: ProcessOwnership::None,
            pid: None,
            instance_id: None,
            exit_code: None,
            executable: None,
            started_unix_ms: None,
            exited_unix_ms: None,
            last_error: None,
        }
    }
}

struct LogBuffer {
    capacity: usize,
    next_sequence: u64,
    entries: VecDeque<ProcessLogEntry>,
}

impl LogBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_sequence: 1,
            entries: VecDeque::new(),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.next_sequence = 1;
    }

    fn push(&mut self, stream: &'static str, text: String) {
        let entry = ProcessLogEntry {
            sequence: self.next_sequence,
            stream: stream.to_string(),
            text,
        };
        self.next_sequence += 1;
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    fn snapshot(&self, stream: Option<&str>, limit: usize) -> Vec<ProcessLogEntry> {
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| stream.is_none_or(|stream| entry.stream == stream))
            .cloned()
            .collect();
        let keep = limit.min(entries.len());
        entries.drain(0..entries.len().saturating_sub(keep));
        entries
    }
}

struct ManagedChild {
    instance_id: String,
    child: AsyncGroupChild,
}

struct Inner {
    backend: SupervisorBackend,
    address: std::net::SocketAddr,
    token: String,
    config: ProcessManagerConfig,
    lifecycle: AsyncMutex<()>,
    child: AsyncMutex<Option<ManagedChild>>,
    record: Mutex<ProcessRecord>,
    logs: Mutex<LogBuffer>,
}

#[derive(Clone)]
pub struct ProcessManager {
    inner: Arc<Inner>,
}

impl ProcessManager {
    pub fn new(
        backend: SupervisorBackend,
        address: std::net::SocketAddr,
        token: impl Into<String>,
        config: ProcessManagerConfig,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                backend,
                address,
                token: token.into(),
                logs: Mutex::new(LogBuffer::new(config.log_capacity)),
                config,
                lifecycle: AsyncMutex::new(()),
                child: AsyncMutex::new(None),
                record: Mutex::new(ProcessRecord::default()),
            }),
        }
    }

    pub fn backend(&self) -> SupervisorBackend {
        self.inner.backend.clone()
    }

    pub async fn status(&self) -> ProcessSnapshot {
        let backend = self.inner.backend.snapshot();
        let child_present = self.inner.child.lock().await.is_some();
        let record = self.inner.record.lock().unwrap().clone();

        if !child_present
            && backend.transport == TransportState::Connected
            && !matches!(
                record.state,
                ProcessState::Starting | ProcessState::Stopping
            )
        {
            return ProcessSnapshot {
                state: ProcessState::Running,
                ownership: ProcessOwnership::External,
                pid: backend.pid,
                instance_id: Some(backend.instance_id),
                connection_id: backend.connection_id,
                transport: transport_name(backend.transport).into(),
                host: host_name(backend.host).into(),
                exit_code: None,
                executable: None,
                started_unix_ms: None,
                exited_unix_ms: None,
                last_error: None,
            };
        }

        ProcessSnapshot {
            state: record.state,
            ownership: record.ownership,
            pid: record.pid,
            instance_id: record.instance_id,
            connection_id: backend.connection_id,
            transport: transport_name(backend.transport).into(),
            host: host_name(backend.host).into(),
            exit_code: record.exit_code,
            executable: record.executable,
            started_unix_ms: record.started_unix_ms,
            exited_unix_ms: record.exited_unix_ms,
            last_error: record.last_error,
        }
    }

    pub fn logs(
        &self,
        stream: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ProcessLogEntry>, ProcessError> {
        if let Some(stream) = stream {
            if stream != "stdout" && stream != "stderr" {
                return Err(ProcessError::new(
                    "INVALID_PROCESS_LOG_STREAM",
                    "stream must be 'stdout', 'stderr', or omitted",
                ));
            }
        }
        Ok(self
            .inner
            .logs
            .lock()
            .unwrap()
            .snapshot(stream, limit.max(1)))
    }

    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.launch_inner().await
    }

    /// Launch the exact executable path reported by Cargo while preserving any
    /// configured game args/current-dir/environment as a launch template.
    /// When no explicit launch target was configured, the artifact is launched
    /// directly with the supervisor's current working directory.
    pub async fn launch_artifact(
        &self,
        executable: impl Into<PathBuf>,
    ) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        let executable = executable.into();
        let mut launch = self
            .inner
            .config
            .launch
            .clone()
            .unwrap_or_else(|| LaunchSpec::new(executable.clone()));
        launch.executable = executable;
        self.launch_spec_inner(launch).await
    }

    pub fn has_configured_launch_target(&self) -> bool {
        self.inner.config.launch.is_some()
    }

    async fn launch_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self.inner.config.launch.clone().ok_or_else(|| {
            ProcessError::new(
                "PROCESS_TARGET_NOT_CONFIGURED",
                "No managed game executable was configured when the supervisor started",
            )
        })?;
        self.launch_spec_inner(launch).await
    }

    async fn launch_spec_inner(&self, launch: LaunchSpec) -> Result<ProcessSnapshot, ProcessError> {
        if self.inner.child.lock().await.is_some() {
            return Err(ProcessError::new(
                "PROCESS_ALREADY_RUNNING",
                "A managed game process is already running",
            ));
        }
        if self.inner.backend.snapshot().transport == TransportState::Connected {
            return Err(ProcessError::new(
                "PROCESS_ALREADY_RUNNING",
                "An external game is already connected; v1 supports only one active game",
            ));
        }

        let instance_id = generate_instance_id();
        self.inner
            .backend
            .prepare_instance(instance_id.clone())
            .map_err(|error| ProcessError::new(error.code, error.message))?;
        self.inner.logs.lock().unwrap().clear();

        let mut command = Command::new(&launch.executable);
        command
            .args(&launch.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("BEVY_MCP_SUPERVISOR_ADDR", self.inner.address.to_string())
            .env("BEVY_MCP_SUPERVISOR_TOKEN", &self.inner.token)
            .env("BEVY_MCP_INSTANCE_ID", &instance_id);
        if let Some(current_dir) = &launch.current_dir {
            command.current_dir(current_dir);
        }
        for (key, value) in &launch.env {
            command.env(key, value);
        }

        let mut group = command.group();
        group.kill_on_drop(true);
        let mut child = match group.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("Failed to launch {}: {error}", launch.executable.display());
                let mut record = self.inner.record.lock().unwrap();
                record.state = ProcessState::Exited;
                record.ownership = ProcessOwnership::Managed;
                record.instance_id = Some(instance_id.clone());
                record.executable = Some(launch.executable.display().to_string());
                record.last_error = Some(message.clone());
                record.exited_unix_ms = Some(now_ms());
                return Err(ProcessError::new("PROCESS_LAUNCH_FAILED", message));
            }
        };

        let pid = child.id();
        let stdout = child.inner().stdout.take();
        let stderr = child.inner().stderr.take();
        {
            let mut record = self.inner.record.lock().unwrap();
            *record = ProcessRecord {
                state: ProcessState::Starting,
                ownership: ProcessOwnership::Managed,
                pid,
                instance_id: Some(instance_id.clone()),
                exit_code: None,
                executable: Some(launch.executable.display().to_string()),
                started_unix_ms: Some(now_ms()),
                exited_unix_ms: None,
                last_error: None,
            };
        }
        *self.inner.child.lock().await = Some(ManagedChild {
            instance_id: instance_id.clone(),
            child,
        });

        if let Some(stdout) = stdout {
            let inner = self.inner.clone();
            tokio::spawn(async move { capture_pipe(stdout, "stdout", inner).await });
        }
        if let Some(stderr) = stderr {
            let inner = self.inner.clone();
            tokio::spawn(async move { capture_pipe(stderr, "stderr", inner).await });
        }

        let monitor = self.clone();
        let monitor_instance = instance_id.clone();
        tokio::spawn(async move { monitor.monitor(monitor_instance).await });

        self.wait_for_startup(&instance_id).await
    }

    pub async fn stop(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.stop_inner().await
    }

    async fn stop_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let child_present = self.inner.child.lock().await.is_some();
        if !child_present {
            if self.inner.backend.snapshot().transport == TransportState::Connected {
                return Err(ProcessError::new(
                    "PROCESS_NOT_MANAGED",
                    "The connected game was not launched by this supervisor and will not be killed",
                ));
            }
            return Err(ProcessError::new(
                "PROCESS_NOT_RUNNING",
                "No managed game process is running",
            ));
        }

        {
            let mut record = self.inner.record.lock().unwrap();
            record.state = ProcessState::Stopping;
        }

        let _ = self
            .inner
            .backend
            .send_shutdown("process_stop requested by MCP supervisor")
            .await;

        if self
            .wait_for_exit(self.inner.config.graceful_stop_timeout)
            .await?
        {
            return Ok(self.status().await);
        }

        {
            let mut guard = self.inner.child.lock().await;
            if let Some(managed) = guard.as_mut() {
                managed.child.start_kill().map_err(|error| {
                    ProcessError::new(
                        "PROCESS_STOP_FAILED",
                        format!("Failed to force-kill managed process tree: {error}"),
                    )
                })?;
            }
        }

        if !self
            .wait_for_exit(self.inner.config.force_stop_timeout)
            .await?
        {
            return Err(ProcessError::new(
                "PROCESS_STOP_TIMEOUT",
                "Managed process tree did not exit after graceful and forced stop windows",
            ));
        }

        Ok(self.status().await)
    }

    pub async fn restart(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        let ownership = self.inner.record.lock().unwrap().ownership;
        let child_present = self.inner.child.lock().await.is_some();
        if !child_present && self.inner.backend.snapshot().transport == TransportState::Connected {
            return Err(ProcessError::new(
                "PROCESS_NOT_MANAGED",
                "The connected game was not launched by this supervisor and cannot be restarted",
            ));
        }
        if !child_present && ownership != ProcessOwnership::Managed {
            return Err(ProcessError::new(
                "PROCESS_NOT_RUNNING",
                "No managed game process has been launched",
            ));
        }
        if child_present {
            self.stop_inner().await?;
        }
        self.launch_inner().await
    }

    pub async fn shutdown_owned(&self) -> Result<(), ProcessError> {
        // Supervisor teardown waits for an in-flight lifecycle operation instead of racing it.
        let _operation = self.inner.lifecycle.lock().await;
        if self.inner.child.lock().await.is_none() {
            return Ok(());
        }
        self.stop_inner().await.map(|_| ())
    }

    fn try_lifecycle_operation(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, ProcessError> {
        self.inner.lifecycle.try_lock().map_err(|_| {
            ProcessError::new(
                "PROCESS_OPERATION_IN_PROGRESS",
                "Another process lifecycle operation is already in progress",
            )
        })
    }

    async fn wait_for_startup(&self, instance_id: &str) -> Result<ProcessSnapshot, ProcessError> {
        let deadline = tokio::time::Instant::now() + self.inner.config.ready_timeout;
        loop {
            let backend = self.inner.backend.snapshot();
            if backend.instance_id == instance_id && backend.host == HostState::Ready {
                {
                    let mut record = self.inner.record.lock().unwrap();
                    if record.instance_id.as_deref() == Some(instance_id) {
                        record.state = ProcessState::Running;
                    }
                }
                return Ok(self.status().await);
            }

            let record = self.inner.record.lock().unwrap().clone();
            if record.instance_id.as_deref() == Some(instance_id)
                && matches!(record.state, ProcessState::Exited | ProcessState::Crashed)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let logs = self.inner.logs.lock().unwrap().snapshot(Some("stderr"), 50);
                return Err(ProcessError::new(
                    "PROCESS_EXITED_DURING_STARTUP",
                    "Managed game exited before the Bevy host became ready",
                )
                .with_details(json!({
                    "instance_id": instance_id,
                    "exit_code": record.exit_code,
                    "stderr_tail": logs,
                })));
            }

            if tokio::time::Instant::now() >= deadline {
                {
                    let mut record = self.inner.record.lock().unwrap();
                    record.last_error = Some("host readiness timeout".into());
                    record.state = ProcessState::Stopping;
                }
                self.force_stop_after_startup_failure().await;
                return Err(ProcessError::new(
                    "PROCESS_START_TIMEOUT",
                    format!(
                        "Managed game did not become host-ready within {:.1} seconds",
                        self.inner.config.ready_timeout.as_secs_f64()
                    ),
                )
                .with_details(json!({
                    "instance_id": instance_id,
                    "stderr_tail": self.inner.logs.lock().unwrap().snapshot(Some("stderr"), 50),
                })));
            }
            tokio::time::sleep(self.inner.config.poll_interval).await;
        }
    }

    async fn force_stop_after_startup_failure(&self) {
        {
            let mut guard = self.inner.child.lock().await;
            if let Some(managed) = guard.as_mut() {
                let _ = managed.child.start_kill();
            }
        }
        let _ = self
            .wait_for_exit(self.inner.config.force_stop_timeout)
            .await;
    }

    async fn wait_for_exit(&self, timeout: Duration) -> Result<bool, ProcessError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.reap_if_exited().await? {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(self.inner.config.poll_interval).await;
        }
    }

    async fn reap_if_exited(&self) -> Result<bool, ProcessError> {
        let mut guard = self.inner.child.lock().await;
        let Some(managed) = guard.as_mut() else {
            return Ok(true);
        };
        let instance_id = managed.instance_id.clone();
        let status = match managed.child.inner().try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return Ok(false),
            Err(error) => {
                return Err(ProcessError::new(
                    "PROCESS_STATUS_FAILED",
                    format!("Failed to poll managed process leader state: {error}"),
                ));
            }
        };

        // The group leader may exit while owned descendants remain alive. Keep ownership of
        // the process-group / Job Object until every remaining member has been terminated and
        // drained. start_kill may report that the group is already gone; that is already clean.
        let _ = managed.child.start_kill();
        managed.child.wait().await.map_err(|error| {
            ProcessError::new(
                "PROCESS_STATUS_FAILED",
                format!("Failed to drain managed process tree after leader exit: {error}"),
            )
        })?;

        guard.take();
        drop(guard);
        self.record_exit(&instance_id, status);
        Ok(true)
    }

    fn record_exit(&self, instance_id: &str, status: ExitStatus) {
        let mut record = self.inner.record.lock().unwrap();
        if record.instance_id.as_deref() != Some(instance_id) {
            return;
        }
        let stopping = record.state == ProcessState::Stopping;
        record.exit_code = status.code();
        record.exited_unix_ms = Some(now_ms());
        record.state = if stopping {
            ProcessState::Stopped
        } else if status.success() {
            ProcessState::Exited
        } else {
            ProcessState::Crashed
        };
    }

    async fn monitor(&self, instance_id: String) {
        loop {
            match self.reap_if_exited().await {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    let mut record = self.inner.record.lock().unwrap();
                    if record.instance_id.as_deref() == Some(instance_id.as_str()) {
                        record.last_error = Some(error.message);
                    }
                    break;
                }
            }
            tokio::time::sleep(self.inner.config.poll_interval).await;
        }
    }
}

async fn capture_pipe<R: AsyncRead + Unpin>(reader: R, stream: &'static str, inner: Arc<Inner>) {
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
                inner.logs.lock().unwrap().push(stream, line);
            }
            Err(error) => {
                inner
                    .logs
                    .lock()
                    .unwrap()
                    .push(stream, format!("[bevy-mcp capture error: {error}]"));
                break;
            }
        }
    }
}

fn transport_name(state: TransportState) -> &'static str {
    match state {
        TransportState::Disconnected => "disconnected",
        TransportState::Connecting => "connecting",
        TransportState::Connected => "connected",
    }
}

fn host_name(state: HostState) -> &'static str {
    match state {
        HostState::Waiting => "waiting",
        HostState::Ready => "ready",
        HostState::Unresponsive => "unresponsive",
    }
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
    use bevy_mcp_core::command::{McpCommand, McpResult};
    use bevy_mcp_core::wire::{
        DEFAULT_MAX_FRAME_SIZE, Hello, WireEnvelope, WireMessage, WireResponse, read_frame,
        write_frame,
    };
    use std::io::Write;
    use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};

    fn fixture_spec(mode: &str) -> LaunchSpec {
        LaunchSpec::new(std::env::current_exe().unwrap())
            .arg("managed_fixture_entry")
            .arg("--nocapture")
            .env("BEVY_MCP_FIXTURE_MODE", mode)
    }

    async fn fixture_manager(
        mode: &str,
        ready_timeout: Duration,
    ) -> (crate::SupervisorTransport, ProcessManager) {
        let token = format!("secret-{}", uuid::Uuid::new_v4());
        let transport = crate::SupervisorTransport::bind_with_options(
            "run-bootstrap".into(),
            token.clone(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_millis(150),
        )
        .await
        .unwrap();
        let manager = ProcessManager::new(
            transport.backend(),
            transport.address(),
            token,
            ProcessManagerConfig {
                launch: Some(fixture_spec(mode)),
                ready_timeout,
                graceful_stop_timeout: Duration::from_millis(250),
                force_stop_timeout: Duration::from_secs(2),
                poll_interval: Duration::from_millis(10),
                log_capacity: 200,
            },
        );
        (transport, manager)
    }

    fn fixture_bridge(mode: &str) {
        if std::env::var("BEVY_MCP_FIXTURE_MODE").ok().as_deref() != Some(mode) {
            return;
        }
        if mode == "exit_startup" {
            eprintln!("fixture startup failure marker");
            std::process::exit(42);
        }

        if mode == "child" {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("descendant_fixture_entry")
                .arg("--nocapture")
                .env("BEVY_MCP_DESCENDANT_FIXTURE", "1")
                .spawn()
                .unwrap();
            println!("DESCENDANT_PID={}", child.id());
            std::io::stdout().flush().unwrap();
            std::mem::forget(child);
        }

        let address: std::net::SocketAddr = std::env::var("BEVY_MCP_SUPERVISOR_ADDR")
            .unwrap()
            .parse()
            .unwrap();
        let token = std::env::var("BEVY_MCP_SUPERVISOR_TOKEN").unwrap();
        let instance_id = std::env::var("BEVY_MCP_INSTANCE_ID").unwrap();
        let mut stream = StdTcpStream::connect(address).unwrap();
        stream.set_nodelay(true).unwrap();
        write_frame(
            &mut stream,
            &WireEnvelope::new(WireMessage::Hello(Hello {
                token,
                instance_id: instance_id.clone(),
                host_version: "stage2-fixture".into(),
                bevy_version: None,
                pid: Some(std::process::id()),
            })),
            DEFAULT_MAX_FRAME_SIZE,
        )
        .unwrap();
        let accepted = read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE).unwrap();
        let connection_id = match accepted.message {
            WireMessage::HelloAccepted(accepted) => accepted.connection_id,
            other => panic!("fixture handshake rejected: {other:?}"),
        };

        let mut crash_armed = false;
        while let Ok(envelope) = read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
            match envelope.message {
                WireMessage::Command(command) => {
                    if let McpCommand::HostProbe { probe_id } = command.command {
                        if mode == "slow_ready" {
                            std::thread::sleep(Duration::from_millis(80));
                        }
                        if mode != "hang" {
                            write_frame(
                                &mut stream,
                                &WireEnvelope::on_connection(
                                    connection_id.clone(),
                                    WireMessage::Response(WireResponse {
                                        request_id: command.request_id,
                                        result: McpResult::success(json!({
                                            "probe_id": probe_id,
                                            "instance_id": instance_id,
                                            "frame": 1,
                                        })),
                                    }),
                                ),
                                DEFAULT_MAX_FRAME_SIZE,
                            )
                            .unwrap();
                            if mode == "crash" && !crash_armed {
                                crash_armed = true;
                                std::thread::spawn(|| {
                                    std::thread::sleep(Duration::from_millis(100));
                                    std::process::exit(23);
                                });
                            }
                        }
                    }
                }
                WireMessage::TransportPing { nonce } => {
                    write_frame(
                        &mut stream,
                        &WireEnvelope::on_connection(
                            connection_id.clone(),
                            WireMessage::TransportPong { nonce },
                        ),
                        DEFAULT_MAX_FRAME_SIZE,
                    )
                    .unwrap();
                }
                WireMessage::Shutdown(_) => break,
                _ => {}
            }
        }
    }

    #[test]
    fn managed_fixture_entry() {
        let Ok(mode) = std::env::var("BEVY_MCP_FIXTURE_MODE") else {
            return;
        };
        fixture_bridge(&mode);
    }

    #[test]
    fn descendant_fixture_entry() {
        if std::env::var("BEVY_MCP_DESCENDANT_FIXTURE").ok().as_deref() != Some("1") {
            return;
        }
        let listener = StdTcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        println!("DESCENDANT_PORT={}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    async fn wait_for_state(
        manager: &ProcessManager,
        expected: ProcessState,
        timeout: Duration,
    ) -> ProcessSnapshot {
        tokio::time::timeout(timeout, async {
            loop {
                let status = manager.status().await;
                if status.state == expected {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {expected:?}"))
    }

    async fn wait_for_descendant_port(manager: &ProcessManager) -> u16 {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let logs = manager.logs(Some("stdout"), 100).unwrap();
                for entry in logs {
                    if let Some(value) = entry.text.strip_prefix("DESCENDANT_PORT=") {
                        if let Ok(port) = value.parse() {
                            return port;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("descendant fixture never reported its port")
    }

    async fn wait_for_port_closed(port: u16) {
        tokio::time::timeout(Duration::from_secs(2), async move {
            loop {
                if StdTcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("descendant listener survived process-tree cleanup");
    }

    #[tokio::test]
    async fn launch_waits_for_frame_processed_readiness() {
        let (_transport, manager) = fixture_manager("slow_ready", Duration::from_secs(2)).await;
        let started = tokio::time::Instant::now();
        let status = manager.launch().await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(60));
        assert_eq!(status.state, ProcessState::Running);
        assert_eq!(status.host, "ready");
        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn startup_exit_reports_code_and_stderr_tail() {
        let (_transport, manager) = fixture_manager("exit_startup", Duration::from_secs(1)).await;
        let error = manager.launch().await.unwrap_err();
        assert_eq!(error.code, "PROCESS_EXITED_DURING_STARTUP");
        assert_eq!(error.details["exit_code"], 42);
        let rendered = error.details["stderr_tail"].to_string();
        assert!(
            rendered.contains("fixture startup failure marker"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn crash_after_ready_is_observed_without_supervisor_exit() {
        let (_transport, manager) = fixture_manager("crash", Duration::from_secs(1)).await;
        manager.launch().await.unwrap();
        let crashed = wait_for_state(&manager, ProcessState::Crashed, Duration::from_secs(2)).await;
        assert_eq!(crashed.exit_code, Some(23));
        assert_eq!(manager.status().await.state, ProcessState::Crashed);
    }

    #[tokio::test]
    async fn stopping_managed_game_kills_long_lived_descendant() {
        let (_transport, manager) = fixture_manager("child", Duration::from_secs(1)).await;
        manager.launch().await.unwrap();
        let port = wait_for_descendant_port(&manager).await;
        assert!(StdTcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_ok());
        manager.stop().await.unwrap();
        wait_for_port_closed(port).await;
    }

    #[tokio::test]
    async fn supervisor_shutdown_cleans_owned_descendants() {
        let (_transport, manager) = fixture_manager("child", Duration::from_secs(1)).await;
        manager.launch().await.unwrap();
        let port = wait_for_descendant_port(&manager).await;
        manager.shutdown_owned().await.unwrap();
        wait_for_port_closed(port).await;
    }

    #[tokio::test]
    async fn external_connection_is_never_killed_by_lifecycle_tools() {
        let token = "external-secret";
        let transport = crate::SupervisorTransport::bind_with_options(
            "run-external".into(),
            token.into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let manager = ProcessManager::new(
            transport.backend(),
            transport.address(),
            token,
            ProcessManagerConfig::default(),
        );
        let mut stream = tokio::net::TcpStream::connect(transport.address())
            .await
            .unwrap();
        let hello = WireEnvelope::new(WireMessage::Hello(Hello {
            token: token.into(),
            instance_id: "run-external".into(),
            host_version: "external".into(),
            bevy_version: None,
            pid: None,
        }));
        let payload = serde_json::to_vec(&hello).unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_u32(payload.len() as u32).await.unwrap();
        stream.write_all(&payload).await.unwrap();
        let length = stream.read_u32().await.unwrap() as usize;
        let mut accepted = vec![0; length];
        stream.read_exact(&mut accepted).await.unwrap();
        let _: WireEnvelope = serde_json::from_slice(&accepted).unwrap();

        let status = manager.status().await;
        assert_eq!(status.ownership, ProcessOwnership::External);
        let error = manager.stop().await.unwrap_err();
        assert_eq!(error.code, "PROCESS_NOT_MANAGED");
        let error = manager.restart().await.unwrap_err();
        assert_eq!(error.code, "PROCESS_NOT_MANAGED");
        assert_eq!(manager.status().await.ownership, ProcessOwnership::External);
        drop(stream);
    }

    #[tokio::test]
    async fn host_hang_remains_connected_but_unresponsive() {
        let (_transport, manager) = fixture_manager("hang", Duration::from_millis(500)).await;
        let launch_manager = manager.clone();
        let launch = tokio::spawn(async move { launch_manager.launch().await });

        tokio::time::timeout(Duration::from_millis(400), async {
            loop {
                let backend = manager.backend().snapshot();
                if backend.transport == TransportState::Connected
                    && backend.host == HostState::Unresponsive
                {
                    let status = manager.status().await;
                    assert_eq!(status.state, ProcessState::Starting);
                    assert_eq!(status.transport, "connected");
                    assert_eq!(status.host, "unresponsive");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("hung fixture never reached connected/unresponsive classification");

        let error = launch.await.unwrap().unwrap_err();
        assert_eq!(error.code, "PROCESS_START_TIMEOUT");
    }

    #[tokio::test]
    async fn concurrent_lifecycle_operations_are_rejected_deterministically() {
        let (_transport, manager) = fixture_manager("slow_ready", Duration::from_secs(2)).await;
        let first_manager = manager.clone();
        let first_launch = tokio::spawn(async move { first_manager.launch().await });

        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if manager.status().await.state == ProcessState::Starting {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first lifecycle operation never entered starting state");

        let second_launch = manager.launch().await.unwrap_err();
        assert_eq!(second_launch.code, "PROCESS_OPERATION_IN_PROGRESS");
        let concurrent_stop = manager.stop().await.unwrap_err();
        assert_eq!(concurrent_stop.code, "PROCESS_OPERATION_IN_PROGRESS");

        let started = first_launch.await.unwrap().unwrap();
        assert_eq!(started.state, ProcessState::Running);
        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn restart_rotates_instance_identity() {
        let (_transport, manager) = fixture_manager("healthy", Duration::from_secs(1)).await;
        let first = manager.launch().await.unwrap();
        let first_instance = first.instance_id.clone().unwrap();
        let second = manager.restart().await.unwrap();
        let second_instance = second.instance_id.clone().unwrap();
        assert_ne!(first_instance, second_instance);
        manager.stop().await.unwrap();
    }
}
