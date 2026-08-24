use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};

use crate::response_dispatcher::{DispatchError, McpResponseDispatcher, format_result};

pub type BackendFuture<'a> =
    Pin<Box<dyn Future<Output = Result<McpResult, GameCallError>> + Send + 'a>>;
pub type SharedGameCommandBackend = Arc<dyn GameCommandBackend>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCallError {
    pub code: &'static str,
    pub message: String,
}

impl GameCallError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub trait GameCommandBackend: Send + Sync {
    fn call(&self, command: McpCommand, timeout: Duration) -> BackendFuture<'_>;
}

#[derive(Clone)]
pub struct EmbeddedBackend {
    dispatcher: McpResponseDispatcher,
}

impl EmbeddedBackend {
    pub fn new(
        ingress: McpIngressQueue,
        results: McpResultQueue,
        connected: Arc<AtomicBool>,
    ) -> Self {
        Self {
            dispatcher: McpResponseDispatcher::new(ingress, results, connected),
        }
    }
}

impl GameCommandBackend for EmbeddedBackend {
    fn call(&self, command: McpCommand, timeout: Duration) -> BackendFuture<'_> {
        Box::pin(async move {
            self.dispatcher
                .call_result(command, timeout)
                .await
                .map_err(|error| match error {
                    DispatchError::Disconnected => GameCallError::new(
                        "GAME_UNAVAILABLE",
                        "No embedded Bevy application is connected",
                    ),
                    DispatchError::Closed => GameCallError::new(
                        "GAME_DISCONNECTED",
                        "The embedded response dispatcher closed before a result was delivered",
                    ),
                    DispatchError::Timeout => GameCallError::new(
                        "REQUEST_TIMEOUT",
                        format!(
                            "Bevy app did not respond within {:.1} seconds",
                            timeout.as_secs_f64()
                        ),
                    ),
                })
        })
    }
}

pub fn format_backend_result(result: Result<McpResult, GameCallError>) -> String {
    match result {
        Ok(result) => format_result(result),
        Err(error) => serde_json::json!({
            "error": error.code,
            "message": error.message,
        })
        .to_string(),
    }
}
