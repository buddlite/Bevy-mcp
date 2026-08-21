pub use bevy_mcp_core::queue::IngressEntry;

/// Bevy Resource wrapper around the core ingress queue.
#[derive(bevy::prelude::Resource, Clone)]
pub struct McpIngressQueue(bevy_mcp_core::queue::McpIngressQueue);

impl Default for McpIngressQueue {
    fn default() -> Self {
        Self(bevy_mcp_core::queue::McpIngressQueue::default())
    }
}

impl McpIngressQueue {
    pub fn from_core(inner: bevy_mcp_core::queue::McpIngressQueue) -> Self {
        Self(inner)
    }

    pub fn push(&self, request_id: u64, command: bevy_mcp_core::command::McpCommand) {
        self.0.push(request_id, command);
    }

    pub fn drain(&self) -> Vec<IngressEntry> {
        self.0.drain()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the inner core queue (for sharing with the MCP server).
    pub fn inner(&self) -> &bevy_mcp_core::queue::McpIngressQueue {
        &self.0
    }
}

/// Bevy Resource wrapper around the core result queue.
#[derive(bevy::prelude::Resource, Clone)]
pub struct McpResultQueue(bevy_mcp_core::queue::McpResultQueue);

impl Default for McpResultQueue {
    fn default() -> Self {
        Self(bevy_mcp_core::queue::McpResultQueue::default())
    }
}

impl McpResultQueue {
    pub fn from_core(inner: bevy_mcp_core::queue::McpResultQueue) -> Self {
        Self(inner)
    }

    pub fn push(&self, response: bevy_mcp_core::command::McpResponse) {
        self.0.push(response);
    }

    pub fn drain(&self) -> Vec<bevy_mcp_core::command::McpResponse> {
        self.0.drain()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the inner core queue (for sharing with the MCP server).
    pub fn inner(&self) -> &bevy_mcp_core::queue::McpResultQueue {
        &self.0
    }
}
