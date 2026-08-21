use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    #[error("entity handle stale: {0}")]
    EntityStale(String),

    #[error("component not registered: {0}")]
    ComponentNotRegistered(String),

    #[error("component not reflected: {0}")]
    ComponentNotReflected(String),

    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("instance disconnected: {0}")]
    InstanceDisconnected(String),

    #[error("runtime not running")]
    RuntimeNotRunning,

    #[error("transaction failed: {0}")]
    TransactionFailed(String),

    #[error("asset not ready: {0}")]
    AssetNotReady(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("build failed")]
    BuildFailed,

    #[error("capability unavailable: {0}")]
    CapabilityUnavailable(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl McpError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::EntityNotFound(_) => "ENTITY_NOT_FOUND",
            Self::EntityStale(_) => "ENTITY_STALE",
            Self::ComponentNotRegistered(_) => "COMPONENT_NOT_REGISTERED",
            Self::ComponentNotReflected(_) => "COMPONENT_NOT_REFLECTED",
            Self::SchemaMismatch(_) => "SCHEMA_MISMATCH",
            Self::PermissionDenied(_) => "PERMISSION_DENIED",
            Self::InstanceDisconnected(_) => "INSTANCE_DISCONNECTED",
            Self::RuntimeNotRunning => "RUNTIME_NOT_RUNNING",
            Self::TransactionFailed(_) => "TRANSACTION_FAILED",
            Self::AssetNotReady(_) => "ASSET_NOT_READY",
            Self::Timeout(_) => "TIMEOUT",
            Self::BuildFailed => "BUILD_FAILED",
            Self::CapabilityUnavailable(_) => "CAPABILITY_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl From<McpError> for StructuredError {
    fn from(err: McpError) -> Self {
        Self {
            code: err.error_code().to_string(),
            message: err.to_string(),
            details: None,
        }
    }
}
