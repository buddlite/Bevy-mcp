use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entity_handle::EntityHandle;

pub const ADVANCED_OPERATION_PREFIX: &str = "bevy-mcp:advanced:v1:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOptions {
    pub camera: Option<EntityHandle>,
    pub crop: Option<CaptureRect>,
    #[serde(default)]
    pub ui_only: bool,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCondition {
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdvancedEntityQuery {
    #[serde(default)]
    pub with_components: Vec<String>,
    #[serde(default)]
    pub without_components: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub predicates: HashMap<String, QueryCondition>,
    #[serde(default)]
    pub changed: Vec<String>,
    #[serde(default)]
    pub parent_has: Vec<String>,
    #[serde(default)]
    pub child_has: Vec<String>,
    pub name_contains: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AdvancedRequest {
    Capture {
        options: CaptureOptions,
    },
    ChangesSince {
        frame: u64,
    },
    EntityChanges {
        frame: u64,
        entity: Option<EntityHandle>,
    },
    ComponentChanges {
        frame: u64,
        component: Option<String>,
    },
    ResourceChanges {
        frame: u64,
        resource: Option<String>,
    },
    ScheduleList,
    ScheduleInspect {
        schedule: String,
    },
    SystemList {
        schedule: Option<String>,
    },
    SystemInspect {
        system: String,
        schedule: Option<String>,
    },
    SystemAccess {
        system: String,
        schedule: Option<String>,
    },
    ComponentWriters {
        component: String,
        schedule: Option<String>,
    },
    ResourceWriters {
        resource: String,
        schedule: Option<String>,
    },
    TrackingConfig {
        mode: Option<String>,
        history_frames: Option<usize>,
        components: Option<Vec<String>>,
        resources: Option<Vec<String>>,
        exclude_components: Option<Vec<String>>,
        exclude_resources: Option<Vec<String>>,
    },
    TrackingStatus,
    SystemTimings {
        schedule: Option<String>,
    },
    StateGet {
        state: Option<String>,
    },
    StateTransition {
        state: String,
        value: Value,
    },
    EntityQuery {
        query: AdvancedEntityQuery,
    },
    SemanticActionList,
    SemanticActionInvoke {
        action: String,
        args: Value,
    },
}

pub fn encode_advanced_request(request: &AdvancedRequest) -> Result<String, serde_json::Error> {
    serde_json::to_string(request).map(|json| format!("{ADVANCED_OPERATION_PREFIX}{json}"))
}

pub fn decode_advanced_request(value: &str) -> Option<Result<AdvancedRequest, serde_json::Error>> {
    value
        .strip_prefix(ADVANCED_OPERATION_PREFIX)
        .map(serde_json::from_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_request_round_trip() {
        let request = AdvancedRequest::ChangesSince { frame: 42 };
        let encoded = encode_advanced_request(&request).unwrap();
        let decoded = decode_advanced_request(&encoded).unwrap().unwrap();
        assert!(matches!(
            decoded,
            AdvancedRequest::ChangesSince { frame: 42 }
        ));
    }

    #[test]
    fn ignores_normal_operation_ids() {
        assert!(decode_advanced_request("ordinary-operation-id").is_none());
    }
}
