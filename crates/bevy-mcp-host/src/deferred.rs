use bevy::prelude::*;
use serde_json::Value;

use bevy_mcp_core::command::McpCommand;
use bevy_mcp_core::entity_handle::EntityHandle;

/// Deferred commands that can't be applied from a read-only system.
///
/// The ingress system queues these; a system in `Update` applies them.
/// Both mutations and reads are deferred so that reads see the result of
/// mutations that were queued in the same frame.
#[derive(Resource, Default)]
pub struct DeferredMcpCommands {
    pub pending: Vec<DeferredCommand>,
}

pub enum DeferredCommand {
    Spawn {
        components: Vec<(String, Value)>,
        result_id: u64,
    },
    Despawn {
        entity: Entity,
        result_id: u64,
    },
    InsertComponent {
        entity: Entity,
        component: String,
        value: Value,
        result_id: u64,
    },
    RemoveComponent {
        entity: Entity,
        component: String,
        result_id: u64,
    },
    InputKey {
        key: String,
        pressed: bool,
        result_id: u64,
    },
    InputMouseButton {
        button: String,
        pressed: bool,
        result_id: u64,
    },
    InputMouseMove {
        x: f64,
        y: f64,
        result_id: u64,
    },
    InputGamepad {
        button: String,
        pressed: bool,
        result_id: u64,
    },
    UiType {
        entity: EntityHandle,
        text: String,
        result_id: u64,
    },
    CameraFrameEntity {
        entity: EntityHandle,
        margin: f64,
        result_id: u64,
    },
    CameraSetTransform {
        x: f64,
        y: f64,
        z: f64,
        result_id: u64,
    },
    CameraLookAt {
        entity: EntityHandle,
        result_id: u64,
    },
    ResourceUpdate {
        resource: String,
        value: Value,
        result_id: u64,
    },
    ResourceRemove {
        resource: String,
        result_id: u64,
    },
    EntityReparent {
        entity: EntityHandle,
        parent: Option<EntityHandle>,
        result_id: u64,
    },
    EntityDuplicate {
        entity: EntityHandle,
        result_id: u64,
    },
    MeshSpawn {
        shape: String,
        size: f64,
        radius: f64,
        color: (f32, f32, f32, f32),
        metallic: f32,
        roughness: f32,
        position: (f32, f32, f32),
        parent: Option<EntityHandle>,
        result_id: u64,
    },
    TemplateLoad {
        name: String,
        path: Option<String>,
        parent: Option<EntityHandle>,
        position: Option<(f32, f32, f32)>,
        result_id: u64,
    },
    /// A read command that should execute after mutations.
    Read {
        command: McpCommand,
        result_id: u64,
    },
}
