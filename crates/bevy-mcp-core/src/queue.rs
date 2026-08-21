use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::command::{McpCommand, McpResponse};

/// Entry in the ingress queue with a request ID for correlating responses.
#[derive(Debug)]
pub struct IngressEntry {
    pub request_id: u64,
    pub command: McpCommand,
}

/// Thread-safe queue for commands from the MCP server → Bevy systems.
///
/// This is the core queue type without Bevy dependencies.
/// The host crate wraps it with `#[derive(Resource)]`.
#[derive(Clone)]
pub struct McpIngressQueue {
    inner: Arc<Mutex<VecDeque<IngressEntry>>>,
}

impl Default for McpIngressQueue {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl McpIngressQueue {
    pub fn push(&self, request_id: u64, command: McpCommand) {
        self.inner.lock().unwrap().push_back(IngressEntry {
            request_id,
            command,
        });
    }

    pub fn drain(&self) -> Vec<IngressEntry> {
        self.inner.lock().unwrap().drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}

/// Thread-safe queue for results from Bevy systems → MCP server.
#[derive(Clone)]
pub struct McpResultQueue {
    inner: Arc<Mutex<VecDeque<McpResponse>>>,
}

impl Default for McpResultQueue {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl McpResultQueue {
    pub fn push(&self, response: McpResponse) {
        self.inner.lock().unwrap().push_back(response);
    }

    pub fn drain(&self) -> Vec<McpResponse> {
        self.inner.lock().unwrap().drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}
