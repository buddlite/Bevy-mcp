use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::command::{McpCommand, McpResult};

pub const SUPERVISOR_PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_MAX_FRAME_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireEnvelope {
    pub protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub message: WireMessage,
}

impl WireEnvelope {
    pub fn new(message: WireMessage) -> Self {
        Self {
            protocol_version: SUPERVISOR_PROTOCOL_VERSION,
            connection_id: None,
            message,
        }
    }

    pub fn on_connection(connection_id: impl Into<String>, message: WireMessage) -> Self {
        Self {
            protocol_version: SUPERVISOR_PROTOCOL_VERSION,
            connection_id: Some(connection_id.into()),
            message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum WireMessage {
    Hello(Hello),
    HelloAccepted(HelloAccepted),
    HelloRejected(WireError),
    Command(WireCommand),
    Response(WireResponse),
    TransportPing { nonce: u64 },
    TransportPong { nonce: u64 },
    Shutdown(ShutdownRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub token: String,
    pub instance_id: String,
    pub host_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bevy_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloAccepted {
    pub instance_id: String,
    pub connection_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireError {
    pub code: String,
    pub message: String,
}

impl WireError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireCommand {
    pub request_id: u64,
    pub command: McpCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireResponse {
    pub request_id: u64,
    pub result: McpResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShutdownRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Error)]
pub enum WireProtocolError {
    #[error("frame length {length} exceeds maximum {maximum}")]
    FrameTooLarge { length: usize, maximum: usize },
    #[error("malformed wire JSON: {0}")]
    MalformedJson(#[from] serde_json::Error),
    #[error("wire I/O failed: {0}")]
    Io(#[from] io::Error),
}

pub fn encode_payload(envelope: &WireEnvelope) -> Result<Vec<u8>, WireProtocolError> {
    Ok(serde_json::to_vec(envelope)?)
}

pub fn decode_payload(payload: &[u8]) -> Result<WireEnvelope, WireProtocolError> {
    Ok(serde_json::from_slice(payload)?)
}

pub fn write_frame<W: Write>(
    writer: &mut W,
    envelope: &WireEnvelope,
    maximum: usize,
) -> Result<(), WireProtocolError> {
    let payload = encode_payload(envelope)?;
    if payload.len() > maximum {
        return Err(WireProtocolError::FrameTooLarge {
            length: payload.len(),
            maximum,
        });
    }
    let length = u32::try_from(payload.len()).map_err(|_| WireProtocolError::FrameTooLarge {
        length: payload.len(),
        maximum,
    })?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read>(
    reader: &mut R,
    maximum: usize,
) -> Result<WireEnvelope, WireProtocolError> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > maximum {
        return Err(WireProtocolError::FrameTooLarge { length, maximum });
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    decode_payload(&payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_message_tag_roundtrips() {
        let envelope = WireEnvelope::new(WireMessage::TransportPing { nonce: 7 });
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["message"]["type"], "transport_ping");
        let decoded: WireEnvelope = serde_json::from_value(json).unwrap();
        assert!(matches!(
            decoded.message,
            WireMessage::TransportPing { nonce: 7 }
        ));
    }

    #[test]
    fn rejects_oversized_frame_before_payload_allocation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(1024_u32).to_be_bytes());
        let error = read_frame(&mut bytes.as_slice(), 16).unwrap_err();
        assert!(matches!(error, WireProtocolError::FrameTooLarge { .. }));
    }
}
