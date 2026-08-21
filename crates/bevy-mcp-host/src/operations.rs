use bevy::prelude::*;
use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};

/// Tracks asynchronous operations (builds, tests, etc.).
#[derive(Resource, Clone)]
pub struct OperationTracker {
    operations: Arc<Mutex<HashMap<String, Operation>>>,
}

pub struct Operation {
    pub id: String,
    pub kind: String,
    pub status: OperationStatus,
    pub started_at: String,
    pub process: Option<Arc<Mutex<Child>>>,
}

#[derive(Debug, Clone)]
pub enum OperationStatus {
    Running,
    Success {
        exit_code: Option<i32>,
    },
    Failed {
        exit_code: Option<i32>,
        error: String,
    },
    Cancelled,
}

impl OperationTracker {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&self, id: String, kind: String) {
        let mut ops = self.operations.lock().unwrap();
        ops.insert(
            id.clone(),
            Operation {
                id,
                kind,
                status: OperationStatus::Running,
                started_at: chrono::Utc::now().to_rfc3339(),
                process: None,
            },
        );
    }

    pub fn complete(&self, id: &str, success: bool, exit_code: Option<i32>, error: Option<String>) {
        let mut ops = self.operations.lock().unwrap();
        if let Some(op) = ops.get_mut(id) {
            op.status = if success {
                OperationStatus::Success { exit_code }
            } else {
                OperationStatus::Failed {
                    exit_code,
                    error: error.unwrap_or_else(|| "Unknown error".to_string()),
                }
            };
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let mut ops = self.operations.lock().unwrap();
        if let Some(op) = ops.get_mut(id) {
            if let Some(process) = &op.process {
                if let Ok(mut proc) = process.lock() {
                    let _ = proc.kill();
                }
            }
            op.status = OperationStatus::Cancelled;
            true
        } else {
            false
        }
    }

    pub fn get_status(&self, id: Option<&str>) -> serde_json::Value {
        let ops = self.operations.lock().unwrap();
        if let Some(id) = id {
            if let Some(op) = ops.get(id) {
                serde_json::json!({
                    "id": op.id,
                    "kind": op.kind,
                    "status": format_status(&op.status),
                    "started_at": op.started_at,
                })
            } else {
                serde_json::json!({ "error": "NOT_FOUND", "message": format!("Operation '{id}' not found") })
            }
        } else {
            let operations: Vec<serde_json::Value> = ops
                .values()
                .map(|op| {
                    serde_json::json!({
                        "id": op.id,
                        "kind": op.kind,
                        "status": format_status(&op.status),
                        "started_at": op.started_at,
                    })
                })
                .collect();
            serde_json::json!({ "operations": operations, "count": operations.len() })
        }
    }
}

impl Default for OperationTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn format_status(status: &OperationStatus) -> serde_json::Value {
    match status {
        OperationStatus::Running => serde_json::json!({ "state": "running" }),
        OperationStatus::Success { exit_code } => {
            serde_json::json!({ "state": "success", "exit_code": exit_code })
        }
        OperationStatus::Failed { exit_code, error } => {
            serde_json::json!({ "state": "failed", "exit_code": exit_code, "error": error })
        }
        OperationStatus::Cancelled => serde_json::json!({ "state": "cancelled" }),
    }
}
