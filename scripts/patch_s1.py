from pathlib import Path
import re

ROOT = Path('.')

def read(path):
    return (ROOT / path).read_text()

def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)

def replace_once(text, old, new, label):
    if old not in text:
        raise RuntimeError(f'missing replacement target: {label}')
    if text.count(old) != 1:
        raise RuntimeError(f'non-unique replacement target ({text.count(old)}): {label}')
    return text.replace(old, new, 1)

def regex_once(text, pattern, repl, label):
    out, n = re.subn(pattern, repl, text, count=1, flags=re.S)
    if n != 1:
        raise RuntimeError(f'regex replacement count={n}: {label}')
    return out

# -----------------------------------------------------------------------------
# S1: central concurrent response dispatcher
# -----------------------------------------------------------------------------
write('crates/bevy-mcp-server/src/response_dispatcher.rs', r'''use std::collections::HashMap;
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
''')

# Patch server lib.
path = 'crates/bevy-mcp-server/src/lib.rs'
t = read(path)
t = replace_once(t, 'pub mod debug_tools;\npub mod tools;\n', 'pub mod debug_tools;\npub mod response_dispatcher;\npub mod tools;\n', 'server lib dispatcher module')
write(path, t)

# Patch BevyMcpState without rewriting the large tool router.
path = 'crates/bevy-mcp-server/src/tools.rs'
t = read(path)
if 'use crate::response_dispatcher::McpResponseDispatcher;' not in t:
    t = replace_once(t, 'use serde::Deserialize;\n', 'use serde::Deserialize;\n\nuse crate::response_dispatcher::McpResponseDispatcher;\n', 'tools dispatcher import')
start = t.index('#[derive(Clone)]\npub struct BevyMcpState')
end = t.index('// ---------------------------------------------------------------------------\n// Server', start)
new_state = r'''#[derive(Clone)]
pub struct BevyMcpState {
    pub ingress: McpIngressQueue,
    pub results: McpResultQueue,
    pub connected: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) dispatcher: McpResponseDispatcher,
}

impl BevyMcpState {
    pub fn new(ingress: McpIngressQueue, results: McpResultQueue) -> Self {
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dispatcher = McpResponseDispatcher::new(
            ingress.clone(),
            results.clone(),
            connected.clone(),
        );
        Self {
            ingress,
            results,
            connected,
            dispatcher,
        }
    }

    /// Construct state for an MCP server embedded in the same process as Bevy.
    /// The caller must give the same queues to `BevyMcpPlugin::with_queues`.
    pub fn embedded(ingress: McpIngressQueue, results: McpResultQueue) -> Self {
        let state = Self::new(ingress, results);
        state.connected.store(true, Ordering::Relaxed);
        state
    }

    /// Push a command and wait for the correlated response through the shared dispatcher.
    async fn call(&self, command: McpCommand) -> String {
        self.dispatcher
            .call(command, std::time::Duration::from_secs(5))
            .await
    }
}

'''
t = t[:start] + new_state + t[end:]
write(path, t)

# Patch advanced server state and remove serialization gate.
path = 'crates/bevy-mcp-server/src/advanced_tools.rs'
t = read(path)
t = t.replace('use std::sync::atomic::{AtomicU64, Ordering};\nuse std::sync::{Arc, Mutex};\n', '')
if 'use crate::response_dispatcher::McpResponseDispatcher;' not in t:
    t = replace_once(t, 'use crate::tools::{BevyMcpServer, BevyMcpState};\n', 'use crate::response_dispatcher::McpResponseDispatcher;\nuse crate::tools::{BevyMcpServer, BevyMcpState};\n', 'advanced dispatcher import')
start = t.index('#[derive(Clone)]\nstruct AdvancedMcpState')
end = t.index('#[derive(Debug, Deserialize, schemars::JsonSchema)]\npub struct CropParams', start)
new = r'''#[derive(Clone)]
struct AdvancedMcpState {
    dispatcher: McpResponseDispatcher,
}

impl AdvancedMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            dispatcher: state.dispatcher.clone(),
        }
    }

    async fn call(&self, request: AdvancedRequest) -> String {
        let operation_id = match encode_advanced_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "ADVANCED_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        self.dispatcher
            .call(
                McpCommand::OperationStatus {
                    operation_id: Some(operation_id),
                },
                std::time::Duration::from_secs(15),
            )
            .await
    }
}

'''
t = t[:start] + new + t[end:]
t = t.replace('/// Combines the legacy tool server and the advanced agent tool server while serializing\n/// queue consumers so correlated responses cannot be drained by the wrong router.\n', '/// Combines legacy and advanced tools. Responses are correlated by the shared dispatcher,\n/// so independent MCP calls can execute concurrently.\n')
t = t.replace('    call_gate: Arc<tokio::sync::Mutex<()>>,\n', '')
t = t.replace('            call_gate: Arc::new(tokio::sync::Mutex::new(())),\n', '')
t = t.replace('        let _guard = self.call_gate.lock().await;\n', '')
write(path, t)

# Patch debugger server state and remove outer serialization gate.
path = 'crates/bevy-mcp-server/src/debug_tools.rs'
t = read(path)
t = t.replace('use std::collections::HashMap;\nuse std::sync::atomic::{AtomicU64, Ordering};\nuse std::sync::{Arc, Mutex};\n', '')
if 'use crate::response_dispatcher::McpResponseDispatcher;' not in t:
    t = replace_once(t, 'use crate::advanced_tools::{AdvancedEntityQueryParams, UnifiedBevyMcpServer};\n', 'use crate::advanced_tools::{AdvancedEntityQueryParams, UnifiedBevyMcpServer};\nuse crate::response_dispatcher::McpResponseDispatcher;\n', 'debug dispatcher import')
start = t.index('#[derive(Clone)]\nstruct DebugMcpState')
end = t.index('#[derive(Debug, Deserialize, schemars::JsonSchema)]\npub struct EvidenceParams', start)
new = r'''#[derive(Clone)]
struct DebugMcpState {
    dispatcher: McpResponseDispatcher,
}

impl DebugMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            dispatcher: state.dispatcher.clone(),
        }
    }

    async fn call(&self, request: DebugRequest) -> String {
        let operation_id = match encode_debug_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "DEBUG_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        self.dispatcher
            .call(
                McpCommand::OperationStatus {
                    operation_id: Some(operation_id),
                },
                std::time::Duration::from_secs(5),
            )
            .await
    }
}

'''
t = t[:start] + new + t[end:]
t = t.replace('/// Top-level MCP server exposing legacy, advanced, and debugger/playtest tools while ensuring\n/// only one tool call drains the shared response queue at a time.\n', '/// Top-level MCP server exposing legacy, advanced, and debugger/playtest tools.\n/// The shared response dispatcher safely supports concurrent calls across all surfaces.\n')
t = t.replace('    call_gate: Arc<tokio::sync::Mutex<()>>,\n', '')
t = t.replace('            call_gate: Arc::new(tokio::sync::Mutex::new(())),\n', '')
t = t.replace('        let _guard = self.call_gate.lock().await;\n', '')
write(path, t)
