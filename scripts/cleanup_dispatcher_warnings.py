from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}")
    file.write_text(text.replace(old, new, 1))


replace_once(
    "crates/bevy-mcp-server/src/advanced_tools.rs",
    "use bevy_mcp_core::command::{McpCommand, McpResult};\nuse bevy_mcp_core::entity_handle::EntityHandle;\nuse bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};\n",
    "use bevy_mcp_core::command::McpCommand;\nuse bevy_mcp_core::entity_handle::EntityHandle;\n",
)

replace_once(
    "crates/bevy-mcp-server/src/debug_tools.rs",
    "use bevy_mcp_core::command::{McpCommand, McpResult};\n",
    "use bevy_mcp_core::command::McpCommand;\n",
)
replace_once(
    "crates/bevy-mcp-server/src/debug_tools.rs",
    "use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};\n",
    "",
)

replace_once(
    "crates/bevy-mcp-server/src/tools.rs",
    "use std::collections::HashMap;\nuse std::sync::atomic::{AtomicU64, Ordering};\nuse std::sync::{Arc, Mutex};\n\nuse bevy_mcp_core::command::{McpCommand, McpResult};\n",
    "use std::sync::Arc;\nuse std::sync::atomic::Ordering;\n\nuse bevy_mcp_core::command::McpCommand;\n",
)
replace_once(
    "crates/bevy-mcp-server/src/tools.rs",
    '''fn format_result(result: McpResult) -> String {
    match result {
        McpResult::Success(value) => value.to_string(),
        McpResult::Error { code, message } => error(&code, message),
    }
}

''',
    "",
)
