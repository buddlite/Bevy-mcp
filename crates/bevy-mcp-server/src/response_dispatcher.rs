use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use tokio::sync::oneshot;

/// A single response pump shared by every MCP surface.
///
/// Requesters register a one-shot sender before pushing their command. The pump is the
/// only consumer of `McpResultQueue`, so legacy, advanced, and debugger calls can all be
/// in flight concurrently without stealing one another's responses.
#[derive(Clone)]
pub struct McpResponseDispatcher {
    ingress: McpIngressQueue,
    results: McpResultQueue,
    connected: Arc<std::sync::atomic::AtomicBool>,
    next_id: Arc<AtomicU64>,
    waiters: Arc<Mutex<HashMap<u64, oneshot::Sender<McpResult>>>>,
    pump_started: Arc<AtomicBool>,
}

impl McpResponseDispatcher {
    pub fn new(
        ingress: McpIngressQueue,
        results: McpResultQueue,
        connected: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            ingress,
            results,
            connected,
            next_id: Arc::new(AtomicU64::new(1)),
            waiters: Arc::new(Mutex::new(HashMap::new())),
            pump_started: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn call(&self, command: McpCommand, timeout: Duration) -> String {
        if !self.connected.load(Ordering::Relaxed) {
            return serde_json::json!({
                "error": "RUNTIME_NOT_RUNNING",
                "message": "No embedded Bevy application is connected."
            })
            .to_string();
        }

        self.ensure_pump();
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().unwrap().insert(request_id, tx);
        self.ingress.push(request_id, command);

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => format_result(result),
            Ok(Err(_)) => {
                self.waiters.lock().unwrap().remove(&request_id);
                serde_json::json!({
                    "error": "RESPONSE_DISPATCHER_CLOSED",
                    "message": "The MCP response dispatcher closed before a result was delivered"
                })
                .to_string()
            }
            Err(_) => {
                self.waiters.lock().unwrap().remove(&request_id);
                serde_json::json!({
                    "error": "TIMEOUT",
                    "message": format!("Bevy app did not respond within {:.1} seconds", timeout.as_secs_f64())
                })
                .to_string()
            }
        }
    }

    fn ensure_pump(&self) {
        if self
            .pump_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let results = self.results.clone();
        let waiters = self.waiters.clone();
        tokio::spawn(async move {
            loop {
                for response in results.drain() {
                    if let Some(waiter) = waiters.lock().unwrap().remove(&response.request_id) {
                        let _ = waiter.send(response.result);
                    } else {
                        tracing::debug!(
                            request_id = response.request_id,
                            "dropping late or unknown MCP response"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
    }
}

pub fn format_result(result: McpResult) -> String {
    match result {
        McpResult::Success(value) => {
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        }
        McpResult::Error { code, message } => serde_json::json!({
            "error": code,
            "message": message,
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_mcp_core::command::{McpResponse, McpResult};

    #[tokio::test]
    async fn routes_out_of_order_concurrent_responses() {
        let ingress = McpIngressQueue::default();
        let results = McpResultQueue::default();
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let dispatcher = McpResponseDispatcher::new(ingress.clone(), results.clone(), connected);

        let a = {
            let dispatcher = dispatcher.clone();
            tokio::spawn(async move {
                dispatcher
                    .call(McpCommand::WorldSummary, Duration::from_secs(1))
                    .await
            })
        };
        let b = {
            let dispatcher = dispatcher.clone();
            tokio::spawn(async move {
                dispatcher
                    .call(McpCommand::Diagnostics, Duration::from_secs(1))
                    .await
            })
        };

        tokio::time::sleep(Duration::from_millis(10)).await;
        let requests = ingress.drain();
        assert_eq!(requests.len(), 2);
        for request in requests.into_iter().rev() {
            results.push(McpResponse {
                request_id: request.request_id,
                result: McpResult::success(serde_json::json!({ "id": request.request_id })),
            });
        }

        let a = a.await.unwrap();
        let b = b.await.unwrap();
        assert!(a.contains("id"));
        assert!(b.contains("id"));
    }
}
