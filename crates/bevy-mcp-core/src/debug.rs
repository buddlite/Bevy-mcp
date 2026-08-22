use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::advanced::{AdvancedEntityQuery, QueryCondition};
use crate::entity_handle::EntityHandle;

pub const DEBUG_OPERATION_PREFIX: &str = "bevy-mcp:debug:v1:";

fn default_changes_frames() -> u64 {
    120
}

fn default_logs_limit() -> u32 {
    50
}

fn default_events_limit() -> u32 {
    50
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceOptions {
    #[serde(default = "default_changes_frames")]
    pub changes_frames: u64,
    #[serde(default = "default_logs_limit")]
    pub logs_limit: u32,
    #[serde(default = "default_events_limit")]
    pub events_limit: u32,
    #[serde(default = "default_true")]
    pub include_states: bool,
    #[serde(default = "default_true")]
    pub include_system_timings: bool,
    #[serde(default = "default_true")]
    pub screenshot: bool,
}

impl Default for EvidenceOptions {
    fn default() -> Self {
        Self {
            changes_frames: default_changes_frames(),
            logs_limit: default_logs_limit(),
            events_limit: default_events_limit(),
            include_states: true,
            include_system_timings: true,
            screenshot: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugCondition {
    EntityExists {
        entity: EntityHandle,
    },
    QueryCount {
        query: AdvancedEntityQuery,
        condition: QueryCondition,
    },
    EntityField {
        entity: EntityHandle,
        component: String,
        field: String,
        condition: QueryCondition,
    },
    ResourceField {
        resource: String,
        field: String,
        condition: QueryCondition,
    },
    StateEquals {
        state: String,
        value: Value,
    },
    LogContains {
        level: Option<String>,
        text: String,
    },
    ChangeOccurred {
        entity: Option<EntityHandle>,
        component: Option<String>,
        resource: Option<String>,
    },
    FrameAtLeast {
        frame: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchpointSpec {
    pub name: String,
    pub condition: DebugCondition,
    #[serde(default)]
    pub pause_on_trigger: bool,
    #[serde(default = "default_true")]
    pub once: bool,
    #[serde(default)]
    pub evidence: EvidenceOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DebugPlaytestStep {
    SemanticAction {
        action: String,
        #[serde(default)]
        args: Value,
    },
    StateTransition {
        state: String,
        value: Value,
    },
    Key {
        key: String,
        #[serde(default = "default_true")]
        pressed: bool,
    },
    StepFrames {
        frames: u32,
    },
    Wait {
        condition: DebugCondition,
        #[serde(default = "default_wait_timeout_frames")]
        timeout_frames: u32,
    },
    Assert {
        condition: DebugCondition,
        message: Option<String>,
    },
    Capture {
        name: Option<String>,
    },
}

fn default_wait_timeout_frames() -> u32 {
    600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugPlaytestPlan {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<DebugPlaytestStep>,
    #[serde(default = "default_true")]
    pub pause_on_failure: bool,
    #[serde(default)]
    pub evidence: EvidenceOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DebugRequest {
    WatchpointAdd {
        spec: WatchpointSpec,
    },
    WatchpointList,
    WatchpointRemove {
        id: String,
    },
    WatchpointClear,
    PlaytestStart {
        plan: DebugPlaytestPlan,
    },
    PlaytestStatus {
        id: String,
    },
    PlaytestList,
    PlaytestCancel {
        id: String,
    },
    CheckpointCreate {
        name: String,
    },
    CheckpointList,
    CheckpointRestore {
        id: String,
    },
    RecordingStart {
        name: String,
    },
    RecordingStop,
    RecordingList,
    ReplayStart {
        recording_id: String,
        checkpoint_id: Option<String>,
    },
    ReplayStatus {
        id: String,
    },
    ReplayCancel {
        id: String,
    },
}

pub fn encode_debug_request(request: &DebugRequest) -> Result<String, serde_json::Error> {
    serde_json::to_string(request).map(|json| format!("{DEBUG_OPERATION_PREFIX}{json}"))
}

pub fn decode_debug_request(value: &str) -> Option<Result<DebugRequest, serde_json::Error>> {
    value
        .strip_prefix(DEBUG_OPERATION_PREFIX)
        .map(serde_json::from_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_request_round_trip() {
        let request = DebugRequest::WatchpointClear;
        let encoded = encode_debug_request(&request).unwrap();
        let decoded = decode_debug_request(&encoded).unwrap().unwrap();
        assert!(matches!(decoded, DebugRequest::WatchpointClear));
    }

    #[test]
    fn ignores_unrelated_operation_ids() {
        assert!(decode_debug_request("bevy-mcp:advanced:v1:{}").is_none());
    }

    #[test]
    fn evidence_defaults_are_agent_friendly() {
        let options = EvidenceOptions::default();
        assert_eq!(options.changes_frames, 120);
        assert_eq!(options.logs_limit, 50);
        assert!(options.screenshot);
    }
}
