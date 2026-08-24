from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return (ROOT / path).read_text()


def write(path, text):
    (ROOT / path).write_text(text)


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)

# Session tools must query the backend contract, never an embedded-only AtomicBool.
path = "crates/bevy-mcp-server/src/tools.rs"
text = read(path)
text = text.replace("use std::sync::atomic::AtomicBool;", "use std::sync::atomic::AtomicBool;")
old = '''    #[tool(
        description = "Report the live MCP capability contract from the Bevy host, including implementation, runtime availability, permission allowance, and deprecations."
    )]
    async fn capabilities(&self) -> String {
        if !self.state.connected.load(Ordering::Relaxed) {
            return serde_json::json!({
                "schema_version": 2,
                "connected": false,
                "message": "Bevy host is not connected; runtime availability and permissions are unknown"
            })
            .to_string();
        }
        self.state.call(McpCommand::Capabilities).await
    }

    #[tool(description = "List connected Bevy application instances")]
    fn instances(&self) -> String {
        let connected = self.state.connected.load(Ordering::Relaxed);
        serde_json::json!({
            "instances": if connected {
                vec![serde_json::json!({"id": "default", "status": "running"})]
            } else { vec![] }
        })
        .to_string()
    }

    #[tool(description = "Get project info (name, path, bevy version, cargo metadata)")]
    fn project_info(&self) -> String {
        serde_json::json!({
            "connected": self.state.connected.load(Ordering::Relaxed),
        })
        .to_string()
    }
'''
new = '''    #[tool(
        description = "Report the live MCP capability contract from the Bevy host, including implementation, runtime availability, permission allowance, and deprecations."
    )]
    async fn capabilities(&self) -> String {
        let status = self.state.backend().status();
        if !status.connected || !status.ready {
            return serde_json::json!({
                "schema_version": 2,
                "mode": status.mode.as_str(),
                "connected": status.connected,
                "ready": status.ready,
                "instance_id": status.instance_id,
                "connection_id": status.connection_id,
                "message": "Bevy host is not ready; runtime availability and permissions are unknown"
            })
            .to_string();
        }
        self.state.call(McpCommand::Capabilities).await
    }

    #[tool(description = "List connected Bevy application instances")]
    fn instances(&self) -> String {
        let status = self.state.backend().status();
        let instances = if status.connected {
            vec![serde_json::json!({
                "id": status.instance_id.clone().unwrap_or_else(|| "unknown".to_string()),
                "status": if status.ready { "running" } else { "connecting" },
                "mode": status.mode.as_str(),
                "connection_id": status.connection_id,
            })]
        } else {
            vec![]
        };
        serde_json::json!({ "instances": instances }).to_string()
    }

    #[tool(description = "Get project info (name, path, bevy version, cargo metadata)")]
    fn project_info(&self) -> String {
        let status = self.state.backend().status();
        serde_json::json!({
            "mode": status.mode.as_str(),
            "connected": status.connected,
            "ready": status.ready,
            "instance_id": status.instance_id,
            "connection_id": status.connection_id,
        })
        .to_string()
    }
'''
text = replace_once(text, old, new, "backend-neutral session tools")
write(path, text)

# The supervisor backend reports the same generic status contract as embedded mode.
path = "crates/bevy-mcp-supervisor/src/backend.rs"
text = read(path)
text = replace_once(
    text,
    "use bevy_mcp_server::backend::{BackendFuture, GameCallError, GameCommandBackend};",
    "use bevy_mcp_server::backend::{\n    BackendFuture, BackendMode, GameBackendStatus, GameCallError, GameCommandBackend,\n};",
    "supervisor backend status imports",
)
old = '''impl GameCommandBackend for SupervisorBackend {
    fn call(&self, command: McpCommand, timeout: Duration) -> BackendFuture<'_> {
        Box::pin(async move {
            self.call_on_connection(command, timeout, None, false)
                .await
        })
    }
}
'''
new = '''impl GameCommandBackend for SupervisorBackend {
    fn call(&self, command: McpCommand, timeout: Duration) -> BackendFuture<'_> {
        Box::pin(async move {
            self.call_on_connection(command, timeout, None, false)
                .await
        })
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
'''
text = replace_once(text, old, new, "supervisor GameCommandBackend status")
write(path, text)

print("Stage 1 backend status fix applied")
