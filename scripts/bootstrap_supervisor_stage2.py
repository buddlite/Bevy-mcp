from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return (ROOT / path).read_text()


def write(path, text):
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)


# --- supervisor Cargo dependencies ---
path = "crates/bevy-mcp-supervisor/Cargo.toml"
text = read(path)
text = replace_once(
    text,
    'anyhow = "1"\n',
    'anyhow = "1"\nclap = { version = "4", features = ["derive"] }\ncommand-group = { version = "5.0.1", features = ["with-tokio"] }\n',
    "supervisor dependencies",
)
write(path, text)


# --- backend: mutable expected instance, shutdown frame, peer PID ---
path = "crates/bevy-mcp-supervisor/src/backend.rs"
text = read(path)
text = text.replace(
    "    DEFAULT_MAX_FRAME_SIZE, HelloAccepted, SUPERVISOR_PROTOCOL_VERSION, WireEnvelope, WireError,\n    WireMessage, WireResponse,\n",
    "    DEFAULT_MAX_FRAME_SIZE, HelloAccepted, SUPERVISOR_PROTOCOL_VERSION, ShutdownRequest,\n    WireEnvelope, WireError, WireMessage, WireResponse,\n",
)
text = replace_once(
    text,
    "struct ActiveConnection {\n    connection_id: String,\n    writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,\n}",
    "struct ActiveConnection {\n    connection_id: String,\n    writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,\n    pid: Option<u32>,\n}",
    "active connection pid",
)
text = replace_once(
    text,
    "    pub connection_id: Option<String>,\n}",
    "    pub connection_id: Option<String>,\n    pub pid: Option<u32>,\n}",
    "snapshot pid",
)
text = replace_once(
    text,
    "    expected_instance_id: String,\n",
    "    expected_instance_id: Mutex<String>,\n",
    "mutable expected instance",
)
text = replace_once(
    text,
    "                expected_instance_id,\n",
    "                expected_instance_id: Mutex::new(expected_instance_id),\n",
    "expected instance init",
)
old_snapshot = '''    pub fn snapshot(&self) -> SupervisorSnapshot {
        let active = self.inner.active.lock().unwrap();
        SupervisorSnapshot {
            process: *self.inner.process.lock().unwrap(),
            transport: *self.inner.transport.lock().unwrap(),
            host: *self.inner.host.lock().unwrap(),
            instance_id: self.inner.expected_instance_id.clone(),
            connection_id: active.as_ref().map(|active| active.connection_id.clone()),
        }
    }
'''
new_snapshot = '''    pub fn snapshot(&self) -> SupervisorSnapshot {
        let active = self.inner.active.lock().unwrap();
        SupervisorSnapshot {
            process: *self.inner.process.lock().unwrap(),
            transport: *self.inner.transport.lock().unwrap(),
            host: *self.inner.host.lock().unwrap(),
            instance_id: self.expected_instance_id(),
            connection_id: active.as_ref().map(|active| active.connection_id.clone()),
            pid: active.as_ref().and_then(|active| active.pid),
        }
    }

    pub fn expected_instance_id(&self) -> String {
        self.inner.expected_instance_id.lock().unwrap().clone()
    }

    /// Prepare the transport to accept a new process incarnation.
    /// This is only legal while no game connection is active.
    pub fn prepare_instance(&self, instance_id: impl Into<String>) -> Result<(), GameCallError> {
        if self.inner.active.lock().unwrap().is_some() {
            return Err(GameCallError::new(
                "INSTANCE_ALREADY_CONNECTED",
                "Cannot rotate the expected instance while a game is connected",
            ));
        }
        *self.inner.expected_instance_id.lock().unwrap() = instance_id.into();
        *self.inner.process.lock().unwrap() = ProcessObservation::Unknown;
        *self.inner.transport.lock().unwrap() = TransportState::Disconnected;
        *self.inner.host.lock().unwrap() = HostState::Waiting;
        Ok(())
    }

    /// Send a lifecycle shutdown request over the authenticated active connection.
    /// The game-side bridge turns this into a Bevy AppExit request.
    pub async fn send_shutdown(&self, reason: impl Into<String>) -> Result<(), GameCallError> {
        let active = self
            .inner
            .active
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| GameCallError::new("GAME_UNAVAILABLE", "No game is connected"))?;
        let envelope = WireEnvelope::on_connection(
            active.connection_id.clone(),
            WireMessage::Shutdown(ShutdownRequest {
                reason: Some(reason.into()),
            }),
        );
        let mut writer = active.writer.lock().await;
        write_envelope(&mut *writer, &envelope, self.inner.maximum_frame_size)
            .await
            .map_err(|error| {
                GameCallError::new(
                    "GAME_DISCONNECTED",
                    format!("Failed to send lifecycle shutdown request: {error}"),
                )
            })
    }
'''
text = replace_once(text, old_snapshot, new_snapshot, "backend snapshot and lifecycle methods")

# Handshake snapshots the expected instance once, preventing mixed-generation comparisons.
needle = "async fn accept_connection(mut stream: TcpStream, backend: SupervisorBackend) -> io::Result<()> {\n    *backend.inner.transport.lock().unwrap() = TransportState::Connecting;\n    let envelope = read_envelope(&mut stream, backend.inner.maximum_frame_size).await?;\n"
replacement = needle + "    let expected_instance_id = backend.expected_instance_id();\n    let peer_pid = match &envelope.message {\n        WireMessage::Hello(hello) => hello.pid,\n        _ => None,\n    };\n"
text = replace_once(text, needle, replacement, "handshake expected instance snapshot")
text = text.replace("backend.inner.expected_instance_id", "expected_instance_id")
# The broad replacement also touched methods above; repair the intended Mutex accesses.
text = text.replace("self.inner.expected_instance_id.lock()", "self.inner.expected_instance_id.lock()")
text = text.replace("*expected_instance_id.lock().unwrap()", "*self.inner.expected_instance_id.lock().unwrap()")
# Repair helper if broad replacement changed it.
text = text.replace("self.expected_instance_id.lock().unwrap().clone()", "self.inner.expected_instance_id.lock().unwrap().clone()")
# HelloAccepted should clone the handshake snapshot.
text = text.replace("instance_id: expected_instance_id.clone().clone(),", "instance_id: expected_instance_id.clone(),")
text = text.replace("instance_id: expected_instance_id.clone(),", "instance_id: expected_instance_id.clone(),")
text = replace_once(
    text,
    "        writer,\n    });",
    "        writer,\n        pid: peer_pid,\n    });",
    "store peer pid",
)
# Probe validation must compare with current immutable launch generation captured for this connection.
old_probe = '''                    && value.get("instance_id").and_then(|value| value.as_str())
                        == Some(expected_instance_id.as_str()) =>
'''
if old_probe not in text:
    old_probe = '''                    && value.get("instance_id").and_then(|value| value.as_str())
                        == Some(probe_backend.inner.expected_instance_id.as_str()) =>
'''
new_probe = '''                    && value.get("instance_id").and_then(|value| value.as_str())
                        == Some(probe_backend.expected_instance_id().as_str()) =>
'''
text = replace_once(text, old_probe, new_probe, "probe expected instance")
write(path, text)


# --- game-side bridge: authenticated Shutdown -> Bevy AppExit; no reconnect after shutdown ---
write("crates/bevy-mcp-host/src/supervisor_bridge.rs", r'''use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_core::wire::{
    DEFAULT_MAX_FRAME_SIZE, Hello, WireEnvelope, WireMessage, WireResponse, read_frame, write_frame,
};

#[derive(Debug, Clone)]
pub struct SupervisorBridgeConfig {
    pub address: SocketAddr,
    pub token: String,
    pub instance_id: String,
    pub host_version: String,
    pub bevy_version: Option<String>,
    pub reconnect_delay: Duration,
    pub maximum_frame_size: usize,
}

impl SupervisorBridgeConfig {
    pub fn from_env() -> Result<Self, String> {
        let address = std::env::var("BEVY_MCP_SUPERVISOR_ADDR")
            .map_err(|_| "BEVY_MCP_SUPERVISOR_ADDR is not set".to_string())?
            .parse()
            .map_err(|error| format!("invalid BEVY_MCP_SUPERVISOR_ADDR: {error}"))?;
        let token = std::env::var("BEVY_MCP_SUPERVISOR_TOKEN")
            .map_err(|_| "BEVY_MCP_SUPERVISOR_TOKEN is not set".to_string())?;
        let instance_id = std::env::var("BEVY_MCP_INSTANCE_ID")
            .map_err(|_| "BEVY_MCP_INSTANCE_ID is not set".to_string())?;
        Ok(Self {
            address,
            token,
            instance_id,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            bevy_version: None,
            reconnect_delay: Duration::from_millis(250),
            maximum_frame_size: DEFAULT_MAX_FRAME_SIZE,
        })
    }
}

/// Cross-thread lifecycle signal set only by an authenticated supervisor Shutdown frame.
#[derive(Resource, Clone, Default)]
pub struct SupervisorShutdownSignal(Arc<AtomicBool>);

impl SupervisorShutdownSignal {
    fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

pub(crate) fn supervisor_shutdown_system(
    signal: Res<SupervisorShutdownSignal>,
    mut exit: MessageWriter<AppExit>,
) {
    if signal.take() {
        exit.write(AppExit::Success);
    }
}

enum ConnectionEnd {
    Reconnect,
    Shutdown,
}

pub fn spawn_supervisor_bridge(
    config: SupervisorBridgeConfig,
    ingress: McpIngressQueue,
    results: McpResultQueue,
    shutdown: SupervisorShutdownSignal,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("bevy-mcp-supervisor-bridge".into())
        .spawn(move || loop {
            match run_connection(&config, &ingress, &results, &shutdown) {
                Ok(ConnectionEnd::Shutdown) => break,
                Ok(ConnectionEnd::Reconnect) => {}
                Err(error) => tracing::debug!(%error, "supervisor bridge connection ended"),
            }
            thread::sleep(config.reconnect_delay);
        })
}

fn run_connection(
    config: &SupervisorBridgeConfig,
    ingress: &McpIngressQueue,
    results: &McpResultQueue,
    shutdown: &SupervisorShutdownSignal,
) -> io::Result<ConnectionEnd> {
    let mut stream = TcpStream::connect(config.address)?;
    stream.set_nodelay(true)?;
    write_frame(
        &mut stream,
        &WireEnvelope::new(WireMessage::Hello(Hello {
            token: config.token.clone(),
            instance_id: config.instance_id.clone(),
            host_version: config.host_version.clone(),
            bevy_version: config.bevy_version.clone(),
            pid: Some(std::process::id()),
        })),
        config.maximum_frame_size,
    )
    .map_err(protocol_io)?;

    let accepted = read_frame(&mut stream, config.maximum_frame_size).map_err(protocol_io)?;
    let connection_id = match accepted.message {
        WireMessage::HelloAccepted(accepted) if accepted.instance_id == config.instance_id => {
            accepted.connection_id
        }
        WireMessage::HelloRejected(error) => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{}: {}", error.code, error.message),
            ));
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "supervisor did not accept the bridge handshake",
            ));
        }
    };

    let reader = stream.try_clone()?;
    let writer = Arc::new(Mutex::new(stream));
    let connected = Arc::new(AtomicBool::new(true));

    let response_writer = {
        let writer = writer.clone();
        let connected = connected.clone();
        let results = results.clone();
        let connection_id = connection_id.clone();
        let maximum = config.maximum_frame_size;
        thread::Builder::new()
            .name("bevy-mcp-supervisor-responses".into())
            .spawn(move || {
                while connected.load(Ordering::Acquire) {
                    for response in results.drain() {
                        let envelope = WireEnvelope::on_connection(
                            connection_id.clone(),
                            WireMessage::Response(WireResponse {
                                request_id: response.request_id,
                                result: response.result,
                            }),
                        );
                        let result = writer
                            .lock()
                            .map_err(|_| io::Error::other("bridge writer lock poisoned"))
                            .and_then(|mut writer| {
                                write_frame(&mut *writer, &envelope, maximum).map_err(protocol_io)
                            });
                        if result.is_err() {
                            connected.store(false, Ordering::Release);
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            })?
    };

    let mut reader = reader;
    let mut disposition = ConnectionEnd::Reconnect;
    while connected.load(Ordering::Acquire) {
        let envelope = match read_frame(&mut reader, config.maximum_frame_size) {
            Ok(envelope) => envelope,
            Err(_) => break,
        };
        if envelope.protocol_version != bevy_mcp_core::wire::SUPERVISOR_PROTOCOL_VERSION {
            break;
        }
        if envelope.connection_id.as_deref() != Some(connection_id.as_str()) {
            continue;
        }
        match envelope.message {
            WireMessage::Command(command) => {
                ingress.push(command.request_id, command.command);
            }
            WireMessage::TransportPing { nonce } => {
                let pong = WireEnvelope::on_connection(
                    connection_id.clone(),
                    WireMessage::TransportPong { nonce },
                );
                let result = writer
                    .lock()
                    .map_err(|_| io::Error::other("bridge writer lock poisoned"))
                    .and_then(|mut writer| {
                        write_frame(&mut *writer, &pong, config.maximum_frame_size)
                            .map_err(protocol_io)
                    });
                if result.is_err() {
                    break;
                }
            }
            WireMessage::Shutdown(_) => {
                shutdown.request();
                disposition = ConnectionEnd::Shutdown;
                break;
            }
            _ => {}
        }
    }

    connected.store(false, Ordering::Release);
    let _ = response_writer.join();
    Ok(disposition)
}

fn protocol_io(error: bevy_mcp_core::wire::WireProtocolError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
''')

# Plugin wires the shutdown signal into Bevy's normal frame schedule.
path = "crates/bevy-mcp-host/src/plugin.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::supervisor_bridge::{SupervisorBridgeConfig, spawn_supervisor_bridge};",
    "use crate::supervisor_bridge::{\n    SupervisorBridgeConfig, SupervisorShutdownSignal, spawn_supervisor_bridge,\n    supervisor_shutdown_system,\n};",
    "plugin bridge imports",
)
old_bridge = '''        if let Some(config) = self.supervisor_bridge.clone() {
            if let Err(error) =
                spawn_supervisor_bridge(config, ingress.inner().clone(), results.inner().clone())
            {
                tracing::error!(%error, "failed to start bevy-mcp supervisor bridge");
            }
        }
'''
new_bridge = '''        let supervisor_shutdown = SupervisorShutdownSignal::default();
        app.insert_resource(supervisor_shutdown.clone());
        if let Some(config) = self.supervisor_bridge.clone() {
            if let Err(error) = spawn_supervisor_bridge(
                config,
                ingress.inner().clone(),
                results.inner().clone(),
                supervisor_shutdown,
            ) {
                tracing::error!(%error, "failed to start bevy-mcp supervisor bridge");
            }
        }
'''
text = replace_once(text, old_bridge, new_bridge, "plugin bridge setup")
old_pre = '''                debugger::debug_ingress_system
                    .before(advanced::advanced_ingress_system)
'''
new_pre = '''                supervisor_shutdown_system.before(debugger::debug_ingress_system),
                debugger::debug_ingress_system
                    .before(advanced::advanced_ingress_system)
'''
text = replace_once(text, old_pre, new_pre, "shutdown system schedule")
write(path, text)


# --- process manager ---
write("crates/bevy-mcp-supervisor/src/process_manager.rs", r'''use std::collections::VecDeque;
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
            && !matches!(record.state, ProcessState::Starting | ProcessState::Stopping)
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

    pub fn logs(&self, stream: Option<&str>, limit: usize) -> Result<Vec<ProcessLogEntry>, ProcessError> {
        if let Some(stream) = stream {
            if stream != "stdout" && stream != "stderr" {
                return Err(ProcessError::new(
                    "INVALID_PROCESS_LOG_STREAM",
                    "stream must be 'stdout', 'stderr', or omitted",
                ));
            }
        }
        Ok(self.inner.logs.lock().unwrap().snapshot(stream, limit.max(1)))
    }

    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self
            .inner
            .config
            .launch
            .clone()
            .ok_or_else(|| ProcessError::new(
                "PROCESS_TARGET_NOT_CONFIGURED",
                "No managed game executable was configured when the supervisor started",
            ))?;

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
            self.stop().await?;
        }
        self.launch().await
    }

    pub async fn shutdown_owned(&self) -> Result<(), ProcessError> {
        if self.inner.child.lock().await.is_none() {
            return Ok(());
        }
        self.stop().await.map(|_| ())
    }

    async fn wait_for_startup(&self, instance_id: &str) -> Result<ProcessSnapshot, ProcessError> {
        let deadline = tokio::time::Instant::now() + self.inner.config.ready_timeout;
        loop {
            let backend = self.inner.backend.snapshot();
            if backend.instance_id == instance_id && backend.host == HostState::Ready {
                let mut record = self.inner.record.lock().unwrap();
                if record.instance_id.as_deref() == Some(instance_id) {
                    record.state = ProcessState::Running;
                }
                drop(record);
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
        let _ = self.wait_for_exit(self.inner.config.force_stop_timeout).await;
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
        match managed.child.try_wait() {
            Ok(Some(status)) => {
                guard.take();
                drop(guard);
                self.record_exit(&instance_id, status);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => Err(ProcessError::new(
                "PROCESS_STATUS_FAILED",
                format!("Failed to poll managed process state: {error}"),
            )),
        }
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

    async fn fixture_manager(mode: &str, ready_timeout: Duration) -> (crate::SupervisorTransport, ProcessManager) {
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
        loop {
            let envelope = match read_frame(&mut stream, DEFAULT_MAX_FRAME_SIZE) {
                Ok(envelope) => envelope,
                Err(_) => break,
            };
            match envelope.message {
                WireMessage::Command(command) => {
                    if let McpCommand::HostProbe { probe_id } = command.command {
                        if mode == "slow_ready" {
                            std::thread::sleep(Duration::from_millis(180));
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

    async fn wait_for_state(manager: &ProcessManager, expected: ProcessState, timeout: Duration) -> ProcessSnapshot {
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
        assert!(started.elapsed() >= Duration::from_millis(150));
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
        assert!(rendered.contains("fixture startup failure marker"), "{rendered}");
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
        let mut stream = tokio::net::TcpStream::connect(transport.address()).await.unwrap();
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
        let (_transport, manager) = fixture_manager("hang", Duration::from_secs(2)).await;
        let launch = manager.launch().await.unwrap_err();
        assert_eq!(launch.code, "PROCESS_START_TIMEOUT");
        // The launch timeout cleans the managed process, while the backend's probe semantics
        // are independently covered by Stage 1. This test ensures a hung host is classified
        // by host readiness rather than a spurious protocol/transport error.
        assert!(launch.message.contains("host-ready"));
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
''')


# --- process MCP tool surface and top-level supervisor wrapper ---
write("crates/bevy-mcp-supervisor/src/process_tools.rs", r'''use bevy_mcp_server::AgentBevyMcpServer;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerInfo,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

use crate::process_manager::{ProcessError, ProcessManager};

fn format_error(error: ProcessError) -> String {
    error.to_json().to_string()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProcessLogsParams {
    #[schemars(description = "Optional stream filter: stdout or stderr.")]
    pub stream: Option<String>,
    #[schemars(description = "Maximum newest log lines to return (default 200).")]
    pub limit: Option<u32>,
}

#[derive(Clone)]
pub struct ProcessToolServer {
    manager: ProcessManager,
}

impl ProcessToolServer {
    fn new(manager: ProcessManager) -> Self {
        Self { manager }
    }
}

#[tool_router(server_handler)]
impl ProcessToolServer {
    #[tool(description = "Return managed/external process ownership plus process, transport, and Bevy-host readiness state.")]
    async fn process_status(&self) -> String {
        serde_json::to_string(&self.manager.status().await).unwrap()
    }

    #[tool(description = "Launch the game executable preconfigured when the supervisor started. Success is returned only after authenticated Bevy host readiness.")]
    async fn process_launch(&self) -> String {
        match self.manager.launch().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(description = "Gracefully stop the supervisor-owned game, then escalate to whole-process-tree termination if necessary. External games are never killed.")]
    async fn process_stop(&self) -> String {
        match self.manager.stop().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(description = "Restart the supervisor-owned game without rebuilding it. Every restart receives a new instance_id and must pass host readiness again.")]
    async fn process_restart(&self) -> String {
        match self.manager.restart().await {
            Ok(status) => serde_json::to_string(&status).unwrap(),
            Err(error) => format_error(error),
        }
    }

    #[tool(description = "Return bounded captured stdout/stderr from the managed game. Game output never shares MCP stdout.")]
    async fn process_logs(&self, Parameters(params): Parameters<ProcessLogsParams>) -> String {
        match self
            .manager
            .logs(params.stream.as_deref(), params.limit.unwrap_or(200) as usize)
        {
            Ok(logs) => serde_json::json!({ "logs": logs, "count": logs.len() }).to_string(),
            Err(error) => format_error(error),
        }
    }
}

#[derive(Clone)]
pub struct SupervisorMcpServer {
    base: AgentBevyMcpServer,
    process: ProcessToolServer,
}

impl SupervisorMcpServer {
    pub fn new(base: AgentBevyMcpServer, manager: ProcessManager) -> Self {
        Self {
            base,
            process: ProcessToolServer::new(manager),
        }
    }
}

impl ServerHandler for SupervisorMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.base.get_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.process.get_tool(request.name.as_ref()).is_some() {
            self.process.call_tool(request, context).await
        } else {
            self.base.call_tool(request, context).await
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut base = self.base.list_tools(request.clone(), context.clone()).await?;
        let process = self.process.list_tools(request, context).await?;
        base.tools.extend(process.tools);
        Ok(base)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.process.get_tool(name).or_else(|| self.base.get_tool(name))
    }
}
''')

# Library exports.
write("crates/bevy-mcp-supervisor/src/lib.rs", r'''pub mod backend;
pub mod process_manager;
pub mod process_tools;

pub use backend::{
    HostState, ProcessObservation, SupervisorBackend, SupervisorSnapshot, SupervisorTransport,
    TransportState, generate_instance_id, generate_token,
};
pub use process_manager::{
    LaunchSpec, ProcessError, ProcessLogEntry, ProcessManager, ProcessManagerConfig,
    ProcessOwnership, ProcessSnapshot, ProcessState,
};
pub use process_tools::SupervisorMcpServer;
''')

# Persistent binary: optional preconfigured managed executable, lifecycle cleanup on MCP disconnect.
write("crates/bevy-mcp-supervisor/src/main.rs", r'''use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::tools::BevyMcpState;
use bevy_mcp_supervisor::{
    LaunchSpec, ProcessManager, ProcessManagerConfig, SupervisorMcpServer, SupervisorTransport,
    generate_instance_id, generate_token,
};
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "bevy-mcp", about = "Persistent MCP supervisor for Bevy games")]
struct Cli {
    #[arg(long, value_name = "PATH")]
    game_executable: Option<PathBuf>,
    #[arg(long = "game-arg", value_name = "ARG")]
    game_args: Vec<String>,
    #[arg(long, value_name = "DIR")]
    game_cwd: Option<PathBuf>,
    #[arg(long, default_value_t = 20)]
    ready_timeout_secs: u64,
    #[arg(long, default_value_t = 3)]
    stop_grace_secs: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("bevy_mcp_supervisor=debug".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let instance_id = generate_instance_id();
    let token = generate_token();
    let transport = SupervisorTransport::bind(instance_id.clone(), token.clone()).await?;

    let launch = cli.game_executable.map(|executable| {
        let mut spec = LaunchSpec::new(executable).args(cli.game_args);
        if let Some(cwd) = cli.game_cwd {
            spec = spec.current_dir(cwd);
        }
        spec
    });
    let manager = ProcessManager::new(
        transport.backend(),
        transport.address(),
        token.clone(),
        ProcessManagerConfig {
            launch,
            ready_timeout: Duration::from_secs(cli.ready_timeout_secs),
            graceful_stop_timeout: Duration::from_secs(cli.stop_grace_secs),
            ..Default::default()
        },
    );

    eprintln!("bevy-mcp supervisor listening on {}", transport.address());
    if manager.status().await.executable.is_none() {
        eprintln!("No managed executable configured; external Stage-1 bridge mode remains available:");
        eprintln!("  BEVY_MCP_SUPERVISOR_ADDR={}", transport.address());
        eprintln!("  BEVY_MCP_SUPERVISOR_TOKEN={token}");
        eprintln!("  BEVY_MCP_INSTANCE_ID={instance_id}");
    }

    let state = BevyMcpState::from_backend(Arc::new(transport.backend()));
    let base = AgentBevyMcpServer::new(state);
    let service = SupervisorMcpServer::new(base, manager.clone())
        .serve(stdio())
        .await?;
    let service_result = service.waiting().await;
    if let Err(error) = manager.shutdown_owned().await {
        tracing::error!(code = error.code, message = %error.message, "failed to clean up managed game during supervisor shutdown");
    }
    service_result?;
    Ok(())
}
''')

# Permanent CI platform gate: full Ubuntu workspace + focused Windows supervisor lifecycle tests.
path = ".github/workflows/ci.yml"
text = read(path)
if "process-windows:" not in text:
    text += r'''

  process-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Check supervisor on Windows
        run: cargo check -p bevy-mcp-supervisor --all-targets
      - name: Test supervisor process lifecycle on Windows
        run: cargo test -p bevy-mcp-supervisor --lib
'''
write(path, text)

print("Stage 2 process manager sources integrated")
