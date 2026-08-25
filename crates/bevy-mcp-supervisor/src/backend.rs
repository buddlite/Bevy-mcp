use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::wire::{
    DEFAULT_MAX_FRAME_SIZE, HelloAccepted, SUPERVISOR_PROTOCOL_VERSION, ShutdownRequest,
    WireEnvelope, WireError, WireMessage, WireResponse,
};
use bevy_mcp_server::backend::{
    BackendFuture, BackendMode, GameBackendStatus, GameCallError, GameCommandBackend,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostState {
    Waiting,
    Ready,
    Unresponsive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessObservation {
    Unknown,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSnapshot {
    pub process: ProcessObservation,
    pub transport: TransportState,
    pub host: HostState,
    pub instance_id: String,
    pub connection_id: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Clone)]
struct ActiveConnection {
    connection_id: String,
    writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    pid: Option<u32>,
}

struct PendingRequest {
    connection_id: String,
    sender: oneshot::Sender<Result<McpResult, GameCallError>>,
}

struct Inner {
    expected_instance_id: Mutex<String>,
    token: String,
    maximum_frame_size: usize,
    probe_timeout: Duration,
    next_request_id: AtomicU64,
    active: Mutex<Option<ActiveConnection>>,
    pending: Mutex<HashMap<u64, PendingRequest>>,
    process: Mutex<ProcessObservation>,
    transport: Mutex<TransportState>,
    host: Mutex<HostState>,
}

#[derive(Clone)]
pub struct SupervisorBackend {
    inner: Arc<Inner>,
}

impl SupervisorBackend {
    fn new(
        expected_instance_id: String,
        token: String,
        maximum_frame_size: usize,
        probe_timeout: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                expected_instance_id: Mutex::new(expected_instance_id),
                token,
                maximum_frame_size,
                probe_timeout,
                next_request_id: AtomicU64::new(1),
                active: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                process: Mutex::new(ProcessObservation::Unknown),
                transport: Mutex::new(TransportState::Disconnected),
                host: Mutex::new(HostState::Waiting),
            }),
        }
    }

    pub fn snapshot(&self) -> SupervisorSnapshot {
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

    async fn call_on_connection(
        &self,
        command: McpCommand,
        timeout: Duration,
        required_connection_id: Option<&str>,
        allow_waiting: bool,
    ) -> Result<McpResult, GameCallError> {
        if !allow_waiting && *self.inner.host.lock().unwrap() != HostState::Ready {
            return Err(GameCallError::new(
                "GAME_UNAVAILABLE",
                "The game transport is connected but the Bevy host is not ready",
            ));
        }

        let active = self
            .inner
            .active
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| GameCallError::new("GAME_UNAVAILABLE", "No game is connected"))?;

        if let Some(required) = required_connection_id
            && active.connection_id != required
        {
            return Err(GameCallError::new(
                "CONNECTION_REPLACED",
                "The game connection generation changed before the command was sent",
            ));
        }

        let request_id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(
            request_id,
            PendingRequest {
                connection_id: active.connection_id.clone(),
                sender,
            },
        );

        let envelope = WireEnvelope::on_connection(
            active.connection_id.clone(),
            WireMessage::Command(bevy_mcp_core::wire::WireCommand {
                request_id,
                command,
            }),
        );
        let write_result = {
            let mut writer = active.writer.lock().await;
            write_envelope(&mut *writer, &envelope, self.inner.maximum_frame_size).await
        };
        if let Err(error) = write_result {
            self.inner.pending.lock().unwrap().remove(&request_id);
            return Err(GameCallError::new(
                "GAME_DISCONNECTED",
                format!("Failed to write to game connection: {error}"),
            ));
        }

        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(GameCallError::new(
                "GAME_DISCONNECTED",
                "The game disconnected before returning a response",
            )),
            Err(_) => {
                let mut pending = self.inner.pending.lock().unwrap();
                if pending
                    .get(&request_id)
                    .is_some_and(|pending| pending.connection_id == active.connection_id)
                {
                    pending.remove(&request_id);
                }
                Err(GameCallError::new(
                    "REQUEST_TIMEOUT",
                    format!(
                        "Game did not respond within {:.1} seconds",
                        timeout.as_secs_f64()
                    ),
                ))
            }
        }
    }

    fn route_response(&self, connection_id: &str, response: WireResponse) {
        let pending = {
            let mut pending = self.inner.pending.lock().unwrap();
            match pending.get(&response.request_id) {
                Some(entry) if entry.connection_id == connection_id => {
                    pending.remove(&response.request_id)
                }
                _ => None,
            }
        };
        if let Some(pending) = pending {
            let _ = pending.sender.send(Ok(response.result));
        } else {
            tracing::debug!(
                request_id = response.request_id,
                %connection_id,
                "dropping response from stale or unknown connection generation"
            );
        }
    }

    fn disconnect_generation(&self, connection_id: &str) {
        let detached = {
            let mut active = self.inner.active.lock().unwrap();
            if active
                .as_ref()
                .is_some_and(|active| active.connection_id == connection_id)
            {
                *active = None;
                true
            } else {
                false
            }
        };
        if !detached {
            return;
        }

        *self.inner.transport.lock().unwrap() = TransportState::Disconnected;
        *self.inner.host.lock().unwrap() = HostState::Waiting;
        *self.inner.process.lock().unwrap() = ProcessObservation::Unknown;

        let doomed = {
            let mut pending = self.inner.pending.lock().unwrap();
            let ids: Vec<u64> = pending
                .iter()
                .filter_map(|(id, request)| (request.connection_id == connection_id).then_some(*id))
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id))
                .collect::<Vec<_>>()
        };
        for pending in doomed {
            let _ = pending.sender.send(Err(GameCallError::new(
                "GAME_DISCONNECTED",
                "The game transport disconnected",
            )));
        }
    }

    fn set_host_state_for_generation(&self, connection_id: &str, state: HostState) {
        let is_current = self
            .inner
            .active
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|active| active.connection_id == connection_id);
        if is_current {
            *self.inner.host.lock().unwrap() = state;
        }
    }
}

impl GameCommandBackend for SupervisorBackend {
    fn call(&self, command: McpCommand, timeout: Duration) -> BackendFuture<'_> {
        Box::pin(async move { self.call_on_connection(command, timeout, None, false).await })
    }

    fn status(&self) -> GameBackendStatus {
        let snapshot = self.snapshot();
        GameBackendStatus {
            mode: BackendMode::Supervised,
            connected: snapshot.transport == TransportState::Connected,
            ready: snapshot.host == HostState::Ready,
            instance_id: Some(snapshot.instance_id),
            connection_id: snapshot.connection_id,
        }
    }
}

pub struct SupervisorTransport {
    backend: SupervisorBackend,
    address: std::net::SocketAddr,
}

impl SupervisorTransport {
    pub async fn bind(
        expected_instance_id: impl Into<String>,
        token: impl Into<String>,
    ) -> io::Result<Self> {
        Self::bind_with_options(
            expected_instance_id.into(),
            token.into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(2),
        )
        .await
    }

    pub async fn bind_with_options(
        expected_instance_id: String,
        token: String,
        maximum_frame_size: usize,
        probe_timeout: Duration,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let backend = SupervisorBackend::new(
            expected_instance_id,
            token,
            maximum_frame_size,
            probe_timeout,
        );
        let accept_backend = backend.clone();
        tokio::spawn(async move {
            accept_loop(listener, accept_backend).await;
        });
        Ok(Self { backend, address })
    }

    pub fn backend(&self) -> SupervisorBackend {
        self.backend.clone()
    }

    pub fn address(&self) -> std::net::SocketAddr {
        self.address
    }
}

async fn accept_loop(listener: TcpListener, backend: SupervisorBackend) {
    loop {
        let Ok((stream, peer)) = listener.accept().await else {
            break;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let backend = backend.clone();
        tokio::spawn(async move {
            if let Err(error) = accept_connection(stream, backend).await {
                tracing::debug!(%error, "rejected supervisor game connection");
            }
        });
    }
}

async fn accept_connection(mut stream: TcpStream, backend: SupervisorBackend) -> io::Result<()> {
    let envelope = read_envelope(&mut stream, backend.inner.maximum_frame_size).await?;
    let expected_instance_id = backend.expected_instance_id();
    let peer_pid = match &envelope.message {
        WireMessage::Hello(hello) => hello.pid,
        _ => None,
    };

    let rejection = if envelope.protocol_version != SUPERVISOR_PROTOCOL_VERSION {
        Some(WireError::new(
            "PROTOCOL_MISMATCH",
            format!(
                "Expected protocol {}, got {}",
                SUPERVISOR_PROTOCOL_VERSION, envelope.protocol_version
            ),
        ))
    } else {
        match &envelope.message {
            WireMessage::Hello(hello) if hello.token != backend.inner.token => {
                Some(WireError::new(
                    "AUTH_FAILED",
                    "Supervisor authentication token did not match",
                ))
            }
            WireMessage::Hello(hello) if hello.instance_id != expected_instance_id => {
                Some(WireError::new(
                    "INSTANCE_MISMATCH",
                    format!(
                        "Expected instance {}, got {}",
                        expected_instance_id, hello.instance_id
                    ),
                ))
            }
            WireMessage::Hello(_) if backend.inner.active.lock().unwrap().is_some() => {
                Some(WireError::new(
                    "INSTANCE_ALREADY_CONNECTED",
                    "A game connection is already active for this supervisor",
                ))
            }
            WireMessage::Hello(_) => None,
            _ => Some(WireError::new(
                "MALFORMED_FRAME",
                "The first supervisor frame must be hello",
            )),
        }
    };

    if let Some(error) = rejection {
        let response = WireEnvelope::new(WireMessage::HelloRejected(error));
        let _ = write_envelope(&mut stream, &response, backend.inner.maximum_frame_size).await;
        if backend.inner.active.lock().unwrap().is_none() {
            *backend.inner.transport.lock().unwrap() = TransportState::Disconnected;
        }
        return Ok(());
    }

    *backend.inner.transport.lock().unwrap() = TransportState::Connecting;
    let connection_id = format!("conn-{}", Uuid::new_v4().simple());
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    {
        let mut writer_guard = writer.lock().await;
        write_envelope(
            &mut *writer_guard,
            &WireEnvelope::new(WireMessage::HelloAccepted(HelloAccepted {
                instance_id: expected_instance_id.clone(),
                connection_id: connection_id.clone(),
            })),
            backend.inner.maximum_frame_size,
        )
        .await?;
    }

    *backend.inner.active.lock().unwrap() = Some(ActiveConnection {
        connection_id: connection_id.clone(),
        writer,
        pid: peer_pid,
    });
    *backend.inner.process.lock().unwrap() = ProcessObservation::Running;
    *backend.inner.transport.lock().unwrap() = TransportState::Connected;
    *backend.inner.host.lock().unwrap() = HostState::Waiting;

    let reader_backend = backend.clone();
    let reader_connection_id = connection_id.clone();
    tokio::spawn(async move {
        read_loop(reader, reader_backend.clone(), reader_connection_id.clone()).await;
        reader_backend.disconnect_generation(&reader_connection_id);
    });

    let probe_backend = backend.clone();
    let probe_connection_id = connection_id.clone();
    tokio::spawn(async move {
        let probe_id = Uuid::new_v4().as_u128() as u64;
        let result = probe_backend
            .call_on_connection(
                McpCommand::HostProbe { probe_id },
                probe_backend.inner.probe_timeout,
                Some(&probe_connection_id),
                true,
            )
            .await;
        match result {
            Ok(McpResult::Success(value))
                if value.get("probe_id").and_then(|value| value.as_u64()) == Some(probe_id)
                    && value.get("instance_id").and_then(|value| value.as_str())
                        == Some(probe_backend.expected_instance_id().as_str()) =>
            {
                probe_backend.set_host_state_for_generation(&probe_connection_id, HostState::Ready)
            }
            Err(error) if error.code == "REQUEST_TIMEOUT" => probe_backend
                .set_host_state_for_generation(&probe_connection_id, HostState::Unresponsive),
            _ => probe_backend
                .set_host_state_for_generation(&probe_connection_id, HostState::Unresponsive),
        }
    });

    Ok(())
}

async fn read_loop(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    backend: SupervisorBackend,
    connection_id: String,
) {
    loop {
        let Ok(envelope) = read_envelope(&mut reader, backend.inner.maximum_frame_size).await
        else {
            break;
        };
        if envelope.protocol_version != SUPERVISOR_PROTOCOL_VERSION
            || envelope.connection_id.as_deref() != Some(connection_id.as_str())
        {
            continue;
        }
        match envelope.message {
            WireMessage::Response(response) => backend.route_response(&connection_id, response),
            WireMessage::TransportPong { .. } => {}
            _ => {}
        }
    }
}

async fn read_envelope<R: AsyncRead + Unpin>(
    reader: &mut R,
    maximum: usize,
) -> io::Result<WireEnvelope> {
    let length = reader.read_u32().await? as usize;
    if length > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("FRAME_TOO_LARGE: {length} > {maximum}"),
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn write_envelope<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &WireEnvelope,
    maximum: usize,
) -> io::Result<()> {
    let payload = serde_json::to_vec(envelope)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.len() > maximum || payload.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "FRAME_TOO_LARGE",
        ));
    }
    writer.write_u32(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

pub fn generate_instance_id() -> String {
    format!("run-{}", Uuid::new_v4().simple())
}

pub fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_mcp_core::wire::{Hello, WireCommand};

    async fn fake_hello(
        address: std::net::SocketAddr,
        token: &str,
        instance_id: &str,
    ) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let hello = WireEnvelope::new(WireMessage::Hello(Hello {
            token: token.to_string(),
            instance_id: instance_id.to_string(),
            host_version: "test".into(),
            bevy_version: None,
            pid: None,
        }));
        write_envelope(&mut stream, &hello, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        let response = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        match response.message {
            WireMessage::HelloAccepted(accepted) => (stream, accepted.connection_id),
            other => panic!("unexpected handshake response: {other:?}"),
        }
    }

    async fn wait_for_host_state(
        backend: &SupervisorBackend,
        expected: HostState,
        timeout: Duration,
    ) {
        let observed = tokio::time::timeout(timeout, async {
            loop {
                let actual = backend.snapshot().host;
                if actual == expected {
                    return actual;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        assert_eq!(
            observed.unwrap_or_else(|_| panic!(
                "timed out waiting for host state {expected:?}; snapshot: {:?}",
                backend.snapshot()
            )),
            expected
        );
    }

    async fn wait_for_transport_state(
        backend: &SupervisorBackend,
        expected: TransportState,
        timeout: Duration,
    ) {
        let observed = tokio::time::timeout(timeout, async {
            loop {
                let actual = backend.snapshot().transport;
                if actual == expected {
                    return actual;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        assert_eq!(
            observed.unwrap_or_else(|_| panic!(
                "timed out waiting for transport state {expected:?}; snapshot: {:?}",
                backend.snapshot()
            )),
            expected
        );
    }

    #[tokio::test]
    async fn hello_alone_is_not_ready_and_probe_timeout_marks_unresponsive() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_millis(50),
        )
        .await
        .unwrap();
        let (_stream, _) = fake_hello(transport.address(), "secret", "run-test").await;
        assert_eq!(transport.backend().snapshot().host, HostState::Waiting);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let snapshot = transport.backend().snapshot();
        assert_eq!(snapshot.transport, TransportState::Connected);
        assert_eq!(snapshot.host, HostState::Unresponsive);
    }

    #[tokio::test]
    async fn frame_processed_probe_is_required_for_ready() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let (mut stream, connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        let probe = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        let (request_id, probe_id) = match probe.message {
            WireMessage::Command(WireCommand {
                request_id,
                command: McpCommand::HostProbe { probe_id },
            }) => (request_id, probe_id),
            other => panic!("expected host probe, got {other:?}"),
        };
        write_envelope(
            &mut stream,
            &WireEnvelope::on_connection(
                connection_id,
                WireMessage::Response(WireResponse {
                    request_id,
                    result: McpResult::success(serde_json::json!({
                        "probe_id": probe_id,
                        "instance_id": "run-test",
                        "frame": 1,
                    })),
                }),
            ),
            DEFAULT_MAX_FRAME_SIZE,
        )
        .await
        .unwrap();
        wait_for_host_state(
            &transport.backend(),
            HostState::Ready,
            Duration::from_millis(250),
        )
        .await;
    }

    #[tokio::test]
    async fn wrong_token_is_rejected_without_poisoning_next_connection() {
        let transport = SupervisorTransport::bind("run-test", "secret")
            .await
            .unwrap();
        let mut bad = TcpStream::connect(transport.address()).await.unwrap();
        write_envelope(
            &mut bad,
            &WireEnvelope::new(WireMessage::Hello(Hello {
                token: "wrong".into(),
                instance_id: "run-test".into(),
                host_version: "test".into(),
                bevy_version: None,
                pid: None,
            })),
            DEFAULT_MAX_FRAME_SIZE,
        )
        .await
        .unwrap();
        let rejection = read_envelope(&mut bad, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert!(matches!(rejection.message, WireMessage::HelloRejected(_)));

        let (_good, _) = fake_hello(transport.address(), "secret", "run-test").await;
        assert_eq!(
            transport.backend().snapshot().transport,
            TransportState::Connected
        );
    }

    #[tokio::test]
    async fn pending_request_fails_immediately_when_connection_generation_disconnects() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let backend = transport.backend();
        let (mut stream, connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;

        // Consume the automatic readiness probe without acknowledging it so the host remains Waiting.
        let probe = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert!(matches!(
            probe.message,
            WireMessage::Command(WireCommand {
                command: McpCommand::HostProbe { .. },
                ..
            })
        ));

        let call_backend = backend.clone();
        let required_connection_id = connection_id.clone();
        let call = tokio::spawn(async move {
            call_backend
                .call_on_connection(
                    McpCommand::WorldSummary,
                    Duration::from_secs(2),
                    Some(&required_connection_id),
                    true,
                )
                .await
        });

        let command = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert!(matches!(
            command.message,
            WireMessage::Command(WireCommand {
                command: McpCommand::WorldSummary,
                ..
            })
        ));

        drop(stream);
        let error = tokio::time::timeout(Duration::from_millis(250), call)
            .await
            .expect("pending call was not failed promptly on disconnect")
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "GAME_DISCONNECTED");
        wait_for_transport_state(
            &backend,
            TransportState::Disconnected,
            Duration::from_millis(250),
        )
        .await;
    }

    #[tokio::test]
    async fn reconnect_after_transport_loss_gets_a_new_connection_generation() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let backend = transport.backend();
        let (first, first_connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        drop(first);

        wait_for_transport_state(
            &backend,
            TransportState::Disconnected,
            Duration::from_millis(250),
        )
        .await;

        let (_second, second_connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        assert_ne!(first_connection_id, second_connection_id);
        assert_eq!(
            backend.snapshot().connection_id.as_deref(),
            Some(second_connection_id.as_str())
        );
    }

    #[tokio::test]
    async fn stale_generation_response_cannot_resolve_current_pending_request() {
        let backend = SupervisorBackend::new(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(1),
        );
        let request_id = 77;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        backend.inner.pending.lock().unwrap().insert(
            request_id,
            PendingRequest {
                connection_id: "conn-current".into(),
                sender,
            },
        );

        backend.route_response(
            "conn-stale",
            WireResponse {
                request_id,
                result: McpResult::success(serde_json::json!({ "source": "stale" })),
            },
        );
        assert!(
            backend
                .inner
                .pending
                .lock()
                .unwrap()
                .contains_key(&request_id)
        );

        backend.route_response(
            "conn-current",
            WireResponse {
                request_id,
                result: McpResult::success(serde_json::json!({ "source": "current" })),
            },
        );
        let result = receiver.await.unwrap().unwrap();
        match result {
            McpResult::Success(value) => {
                assert_eq!(
                    value.get("source").and_then(|value| value.as_str()),
                    Some("current")
                );
            }
            other => panic!("expected current-generation success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn simultaneous_second_game_is_rejected() {
        let transport = SupervisorTransport::bind("run-test", "secret")
            .await
            .unwrap();
        let (_first, first_connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        let mut second = TcpStream::connect(transport.address()).await.unwrap();
        write_envelope(
            &mut second,
            &WireEnvelope::new(WireMessage::Hello(Hello {
                token: "secret".into(),
                instance_id: "run-test".into(),
                host_version: "test".into(),
                bevy_version: None,
                pid: None,
            })),
            DEFAULT_MAX_FRAME_SIZE,
        )
        .await
        .unwrap();
        let response = read_envelope(&mut second, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        match response.message {
            WireMessage::HelloRejected(error) => {
                assert_eq!(error.code, "INSTANCE_ALREADY_CONNECTED")
            }
            other => panic!("expected rejection, got {other:?}"),
        }

        let snapshot = transport.backend().snapshot();
        assert_eq!(snapshot.transport, TransportState::Connected);
        assert_eq!(
            snapshot.connection_id.as_deref(),
            Some(first_connection_id.as_str())
        );
    }
}
