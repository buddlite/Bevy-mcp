use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

pub fn spawn_supervisor_bridge(
    config: SupervisorBridgeConfig,
    ingress: McpIngressQueue,
    results: McpResultQueue,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("bevy-mcp-supervisor-bridge".into())
        .spawn(move || {
            loop {
                if let Err(error) = run_connection(&config, &ingress, &results) {
                    tracing::debug!(%error, "supervisor bridge connection ended");
                }
                thread::sleep(config.reconnect_delay);
            }
        })
}

fn run_connection(
    config: &SupervisorBridgeConfig,
    ingress: &McpIngressQueue,
    results: &McpResultQueue,
) -> io::Result<()> {
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
            WireMessage::Shutdown(_) => break,
            _ => {}
        }
    }

    connected.store(false, Ordering::Release);
    let _ = response_writer.join();
    Ok(())
}

fn protocol_io(error: bevy_mcp_core::wire::WireProtocolError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
