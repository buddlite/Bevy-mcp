use serde_json::Value;

use crate::entity_handle::EntityHandle;

/// A command from the MCP server, using raw entity IDs.
///
/// The host crate converts these to Bevy `Entity` values.
#[derive(Debug, Clone)]
pub enum McpCommand {
    // -- ECS inspection --
    WorldSummary,
    EntityQuery {
        with_components: Vec<String>,
        without_components: Vec<String>,
        include: Vec<String>,
        limit: u32,
    },
    EntityGet {
        entity: EntityHandle,
    },
    ComponentGet {
        entity: EntityHandle,
        component: String,
    },
    ComponentSchema {
        component: String,
    },
    ResourceList,
    ResourceGet {
        resource: String,
    },
    ResourceSchema {
        resource: String,
    },

    // -- ECS mutation --
    EntitySpawn {
        components: Vec<(String, Value)>,
    },
    EntityDespawn {
        entity: EntityHandle,
    },
    ComponentInsert {
        entity: EntityHandle,
        component: String,
        value: Value,
    },
    ComponentUpdate {
        entity: EntityHandle,
        component: String,
        value: Value,
    },
    ComponentRemove {
        entity: EntityHandle,
        component: String,
    },

    // -- Runtime --
    RuntimeLaunch,
    RuntimeStop,
    RuntimeRestart,
    RuntimePause,
    RuntimeResume,
    RuntimeStep {
        frames: u32,
    },
    RuntimeTimeScale {
        scale: f64,
    },

    // -- Resources --
    ResourceUpdate {
        resource: String,
        value: Value,
    },
    ResourceInsert {
        resource: String,
        value: Value,
    },
    ResourceRemove {
        resource: String,
    },

    // -- Hierarchy --
    EntityReparent {
        entity: EntityHandle,
        parent: Option<EntityHandle>,
    },
    EntityDuplicate {
        entity: EntityHandle,
    },

    // -- Input --
    InputKey {
        key: String,
        pressed: bool,
    },
    InputMouseButton {
        button: String,
        pressed: bool,
        x: Option<f64>,
        y: Option<f64>,
    },
    InputMouseMove {
        x: f64,
        y: f64,
    },
    InputAction {
        action: String,
        strength: f64,
    },
    InputGamepad {
        button: String,
        pressed: bool,
    },

    // -- Diagnostics --
    Logs {
        level: Option<String>,
        limit: u32,
    },
    Diagnostics,

    // -- Hierarchy --
    Hierarchy {
        root: Option<EntityHandle>,
        max_depth: u32,
    },

    // -- Events --
    ObserveEvents {
        event_type: Option<String>,
        limit: u32,
    },

    // -- UI --
    UiQuery {
        root: Option<EntityHandle>,
        max_depth: u32,
    },

    // -- Plugins --
    ListPlugins,

    // -- Operations --
    OperationStatus {
        operation_id: Option<String>,
    },
    OperationCancel {
        operation_id: String,
    },

    // -- Capture --
    CaptureGame,

    // -- Assets --
    AssetList {
        filter: Option<String>,
    },
    AssetGet {
        path: String,
    },
    AssetStatus {
        path: String,
    },
    AssetReload {
        path: String,
    },

    // -- Camera --
    CameraFrameEntity {
        entity: EntityHandle,
    },
    CameraInspect,
    CameraSetTransform {
        x: f64,
        y: f64,
        z: f64,
    },
    CameraLookAt {
        entity: EntityHandle,
    },
    CaptureCamera,

    // -- Semantic UI --
    UiInspect {
        entity: EntityHandle,
    },
    UiClick {
        entity: EntityHandle,
    },
    UiType {
        entity: EntityHandle,
        text: String,
    },

    // -- Playtest --
    PlaytestRun {
        steps: Vec<PlaytestStep>,
    },
    Assert {
        assertion: Assertion,
    },
}

/// A single step in a playtest scenario.
#[derive(Debug, Clone)]
pub enum PlaytestStep {
    RuntimeRestart,
    RuntimeStep {
        frames: u32,
    },
    InputAction {
        action: String,
        duration_secs: f64,
    },
    InputKey {
        key: String,
        pressed: bool,
    },
    WaitForEntity {
        component: String,
        timeout_secs: f64,
    },
    Assert {
        assertion: Assertion,
    },
    CaptureGame {
        name: String,
    },
}

/// An assertion to verify game state.
#[derive(Debug, Clone)]
pub enum Assertion {
    EntityExists {
        entity_id: u32,
    },
    ComponentExists {
        entity_id: u32,
        component: String,
    },
    ComponentEquals {
        entity_id: u32,
        component: String,
        field: String,
        value: Value,
    },
    EntityCount {
        expected: u32,
    },
    ResourceEquals {
        resource: String,
        field: String,
        value: Value,
    },
}

/// Response from the Bevy host back to the MCP server.
#[derive(Debug, Clone)]
pub struct McpResponse {
    pub request_id: u64,
    pub result: McpResult,
}

#[derive(Debug, Clone)]
pub enum McpResult {
    Success(Value),
    Error { code: String, message: String },
}

impl McpResult {
    pub fn success(value: Value) -> Self {
        Self::Success(value)
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
