use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::entity_handle::EntityHandle;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

use crate::response_dispatcher::McpResponseDispatcher;

// ---------------------------------------------------------------------------
// Shared state between MCP server and Bevy app
// ---------------------------------------------------------------------------

/// Shared state bridging the MCP server (tokio) and the Bevy app.
///
/// The server holds this; the Bevy app holds the same queues via its
/// Resource wrappers. Both sides communicate through the core queues.
#[derive(Clone)]
pub struct BevyMcpState {
    pub ingress: McpIngressQueue,
    pub results: McpResultQueue,
    pub connected: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) dispatcher: McpResponseDispatcher,
}

impl BevyMcpState {
    pub fn new(ingress: McpIngressQueue, results: McpResultQueue) -> Self {
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dispatcher =
            McpResponseDispatcher::new(ingress.clone(), results.clone(), connected.clone());
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

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct BevyMcpServer {
    state: BevyMcpState,
}

impl BevyMcpServer {
    pub fn new(state: BevyMcpState) -> Self {
        Self { state }
    }
}

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityQueryParams {
    #[schemars(description = "Component types the entity must have")]
    pub with_components: Option<Vec<String>>,
    #[schemars(description = "Component types the entity must NOT have")]
    pub without_components: Option<Vec<String>>,
    #[schemars(description = "Component types to include in the response")]
    pub include: Option<Vec<String>>,
    #[schemars(description = "Maximum number of results")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityGetParams {
    #[schemars(description = "Entity handle URI (entity://instance/world/id/gen)")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentGetParams {
    #[schemars(description = "Entity handle URI")]
    pub entity: String,
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentSchemaParams {
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntitySpawnParams {
    #[schemars(description = "Components to insert on the new entity")]
    pub components: Option<Vec<ComponentValue>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityDespawnParams {
    #[schemars(description = "Entity handle URI")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentUpdateParams {
    #[schemars(description = "Entity handle URI")]
    pub entity: String,
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
    #[schemars(description = "New value (JSON)")]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentInsertParams {
    #[schemars(description = "Entity handle URI")]
    pub entity: String,
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
    #[schemars(description = "Initial value (JSON)")]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentRemoveParams {
    #[schemars(description = "Entity handle URI")]
    pub entity: String,
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[allow(dead_code)]
pub struct ComponentValue {
    #[schemars(description = "Fully qualified component type name")]
    pub component: String,
    #[schemars(description = "Value (JSON)")]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StepParams {
    #[schemars(description = "Number of frames to advance")]
    pub frames: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TimeScaleParams {
    #[schemars(description = "Time scale multiplier (1.0 = normal)")]
    pub scale: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InputKeyParams {
    #[schemars(description = "Key name (e.g. 'Space', 'KeyA', 'ArrowUp')")]
    pub key: String,
    #[schemars(description = "true = press, false = release")]
    pub pressed: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InputMouseParams {
    #[schemars(description = "Mouse event: 'motion' or 'button'")]
    pub event: String,
    #[schemars(description = "X position (pixels)")]
    pub x: Option<f64>,
    #[schemars(description = "Y position (pixels)")]
    pub y: Option<f64>,
    #[schemars(description = "Button: 'left', 'right', 'middle'")]
    pub button: Option<String>,
    #[schemars(description = "true = press, false = release")]
    pub pressed: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InputActionParams {
    #[schemars(description = "Action name (e.g. 'move_forward', 'fire')")]
    pub action: String,
    #[schemars(description = "Action strength (0.0 to 1.0)")]
    pub strength: Option<f64>,
    #[schemars(description = "Duration in milliseconds")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InputGamepadParams {
    #[schemars(
        description = "Gamepad button name (e.g. 'south', 'north', 'east', 'west', 'left_trigger')"
    )]
    pub button: String,
    #[schemars(description = "true = press, false = release")]
    pub pressed: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogsParams {
    #[schemars(description = "Log level filter: 'error', 'warn', 'info', 'debug', 'trace'")]
    pub level: Option<String>,
    #[schemars(description = "Maximum lines to return")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResourceGetParams {
    #[schemars(description = "Fully qualified resource type name")]
    pub resource: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResourceSchemaParams {
    #[schemars(description = "Fully qualified resource type name")]
    pub resource: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResourceUpdateParams {
    #[schemars(description = "Fully qualified resource type name")]
    pub resource: String,
    #[schemars(description = "New value (JSON)")]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityReparentParams {
    #[schemars(description = "Entity handle URI to reparent")]
    pub entity: String,
    #[schemars(description = "New parent entity handle URI, or null to make root")]
    pub parent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityDuplicateParams {
    #[schemars(description = "Entity handle URI to duplicate")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BuildParams {
    #[schemars(description = "Cargo profile: 'dev' or 'release'")]
    pub profile: Option<String>,
    #[schemars(description = "Specific features to enable")]
    pub features: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TestParams {
    #[schemars(description = "Test name filter")]
    pub filter: Option<String>,
    #[schemars(description = "Run tests in a specific package")]
    pub package: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CameraFrameParams {
    #[schemars(description = "Entity handle URI to frame")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CameraSetTransformParams {
    #[schemars(description = "X position")]
    pub x: f64,
    #[schemars(description = "Y position")]
    pub y: f64,
    #[schemars(description = "Z position")]
    pub z: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CameraLookAtParams {
    #[schemars(description = "Entity handle URI to look at")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchOperation {
    #[schemars(description = "Tool name to call")]
    pub tool: String,
    #[schemars(description = "Arguments for the tool")]
    pub arguments: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchParams {
    #[schemars(description = "List of operations to execute in sequence")]
    pub operations: Vec<BatchOperation>,
    #[schemars(description = "If true, stop on first error")]
    pub stop_on_error: Option<bool>,
    #[schemars(description = "Unsupported. Atomic rollback is not available.")]
    pub atomic: Option<bool>,
    #[schemars(
        description = "If true, return a preview without applying changes. Arguments are not validated."
    )]
    pub dry_run: Option<bool>,
    #[schemars(description = "Unsupported. Per-operation verification is not available.")]
    pub verify: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HierarchyParams {
    #[schemars(description = "Root entity handle URI. If omitted, returns all root entities.")]
    pub root: Option<String>,
    #[schemars(description = "Maximum depth to traverse (default 10)")]
    pub max_depth: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveEventsParams {
    #[schemars(description = "Event type to filter by (e.g. 'Collision', 'Trigger')")]
    pub event_type: Option<String>,
    #[schemars(description = "Maximum number of events to return (default 100)")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiQueryParams {
    #[schemars(description = "Root entity handle URI. If omitted, queries all UI elements.")]
    pub root: Option<String>,
    #[schemars(description = "Maximum depth to traverse (default 10)")]
    pub max_depth: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiInspectParams {
    #[schemars(description = "Entity handle URI to inspect")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiClickParams {
    #[schemars(description = "Entity handle URI to click")]
    pub entity: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UiTypeParams {
    #[schemars(description = "Entity handle URI of text input")]
    pub entity: String,
    #[schemars(description = "Text to type")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ColorValue {
    #[schemars(description = "Red channel (0.0-1.0)")]
    pub r: f32,
    #[schemars(description = "Green channel (0.0-1.0)")]
    pub g: f32,
    #[schemars(description = "Blue channel (0.0-1.0)")]
    pub b: f32,
    #[schemars(description = "Alpha channel (0.0-1.0, default 1.0)")]
    pub a: Option<f32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Vec3Value {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MeshSpawnParams {
    #[schemars(description = "Mesh shape: 'cube', 'sphere', 'plane', 'cylinder', 'torus'")]
    pub shape: String,
    #[schemars(description = "Uniform size for cube/plane, height for cylinder (default 1.0)")]
    pub size: Option<f64>,
    #[schemars(description = "Radius for sphere/cylinder/torus (default 0.5)")]
    pub radius: Option<f64>,
    #[schemars(description = "Base color as {r, g, b, a?} with 0.0-1.0 values (default white)")]
    pub color: Option<ColorValue>,
    #[schemars(description = "Metallic value 0.0-1.0 (default 0.0)")]
    pub metallic: Option<f32>,
    #[schemars(description = "Roughness value 0.0-1.0 (default 0.5)")]
    pub roughness: Option<f32>,
    #[schemars(description = "Position as {x, y, z} (default origin)")]
    pub position: Option<Vec3Value>,
    #[schemars(description = "Parent entity handle URI")]
    pub parent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TemplateSaveParams {
    #[schemars(description = "Entity handle URI to save as template")]
    pub entity: String,
    #[schemars(description = "Template name")]
    pub name: String,
    #[schemars(description = "File path (default: templates/{name}.json)")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TemplateLoadParams {
    #[schemars(description = "Template name to load")]
    pub name: String,
    #[schemars(description = "File path override")]
    pub path: Option<String>,
    #[schemars(description = "Parent entity handle URI")]
    pub parent: Option<String>,
    #[schemars(description = "Override position as {x, y, z}")]
    pub position: Option<Vec3Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlaytestStepDef {
    #[schemars(
        description = "Step action: 'runtime_restart', 'runtime_step', 'input_action', 'capture_game'"
    )]
    pub action: String,
    #[schemars(description = "Number of frames (for runtime_step)")]
    pub frames: Option<u32>,
    #[schemars(description = "Action name (for input_action)")]
    pub action_name: Option<String>,
    #[schemars(description = "Duration in seconds")]
    pub duration: Option<f64>,
    #[schemars(description = "Capture name")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlaytestRunParams {
    #[schemars(description = "Sequence of playtest steps")]
    pub steps: Vec<PlaytestStepDef>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssertParams {
    #[schemars(
        description = "Assertion type: 'entity_exists', 'component_exists', 'entity_count'"
    )]
    pub assertion_type: String,
    #[schemars(description = "Entity ID")]
    pub entity_id: Option<u32>,
    #[schemars(description = "Component name")]
    pub component: Option<String>,
    #[schemars(description = "Expected entity count")]
    pub expected_count: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OperationStatusParams {
    #[schemars(description = "Operation ID to check. If omitted, returns all operations.")]
    pub operation_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OperationCancelParams {
    #[schemars(description = "Operation ID to cancel")]
    pub operation_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssetListParams {
    #[schemars(description = "Filter by asset type (e.g. 'Image', 'Mesh', 'Scene')")]
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssetGetParams {
    #[schemars(description = "Asset path (e.g. 'res://textures/icon.png')")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssetStatusParams {
    #[schemars(description = "Asset path")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssetReloadParams {
    #[schemars(description = "Asset path to reload")]
    pub path: String,
}

// ---------------------------------------------------------------------------
// Tool implementations — wired to Bevy via shared queues
// ---------------------------------------------------------------------------

#[tool_router(server_handler)]
impl BevyMcpServer {
    // -- Session & environment --

    #[tool(description = "Check server health, runtime status, FPS, entity count, bevy version")]
    async fn health(&self) -> String {
        self.state.call(McpCommand::Diagnostics).await
    }

    #[tool(description = "List capabilities this server instance provides")]
    fn capabilities(&self) -> String {
        serde_json::json!({
            "ecs": { "inspect": true, "mutate": true, "query": true, "hierarchy": true, "reflection": true },
            "runtime": { "control": true, "step": true, "time_scale": true, "pause": true },
            "input": { "raw": true, "actions": false },
            "capture": { "game": false, "camera": false },
            "assets": { "inspect": false, "reload": false },
            "diagnostics": { "render": false, "performance": true, "logs": true, "observe_events": true },
            "ui": { "query": true, "inspect": true, "click": true, "type_text": true },
            "procedural": { "mesh_spawn": true, "template_save": true, "template_load": true },
            "plugins": { "list": true },
            "build": { "cargo": false, "check": false, "test": false }
        })
        .to_string()
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

    #[tool(description = "Get runtime status: paused state, time scale, frame count, entity count")]
    async fn runtime_status(&self) -> String {
        self.state.call(McpCommand::Diagnostics).await
    }

    #[tool(description = "Get recent errors from the application (separate from logs)")]
    async fn errors(&self) -> String {
        // For now, errors are captured via the same log system.
        self.state
            .call(McpCommand::Logs {
                level: Some("ERROR".to_string()),
                limit: 100,
            })
            .await
    }

    #[tool(description = "Get entity hierarchy tree. Returns parent-child relationships.")]
    async fn hierarchy(&self, Parameters(params): Parameters<HierarchyParams>) -> String {
        let root = match params.root {
            Some(root) => match parse_entity_handle(&root) {
                Ok(handle) => Some(handle),
                Err(message) => return error("INVALID_HANDLE", message),
            },
            None => None,
        };
        self.state
            .call(McpCommand::Hierarchy {
                root,
                max_depth: params.max_depth.unwrap_or(10),
            })
            .await
    }

    #[tool(description = "Observe recent events. Returns captured events of the specified type.")]
    async fn observe_events(&self, Parameters(params): Parameters<ObserveEventsParams>) -> String {
        self.state
            .call(McpCommand::ObserveEvents {
                event_type: params.event_type,
                limit: params.limit.unwrap_or(100),
            })
            .await
    }

    #[tool(description = "Query UI tree. Returns Node, Text, Button elements with layout info.")]
    async fn ui_query(&self, Parameters(params): Parameters<UiQueryParams>) -> String {
        let root = match params.root {
            Some(root) => match parse_entity_handle(&root) {
                Ok(handle) => Some(handle),
                Err(message) => return error("INVALID_HANDLE", message),
            },
            None => None,
        };
        self.state
            .call(McpCommand::UiQuery {
                root,
                max_depth: params.max_depth.unwrap_or(10),
            })
            .await
    }

    #[tool(description = "Inspect a UI element's details (bounds, text, state)")]
    async fn ui_inspect(&self, Parameters(params): Parameters<UiInspectParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state.call(McpCommand::UiInspect { entity }).await
    }

    #[tool(description = "Click a UI element (button, link, etc.)")]
    async fn ui_click(&self, Parameters(params): Parameters<UiClickParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state.call(McpCommand::UiClick { entity }).await
    }

    #[tool(description = "Type text into a UI text input field")]
    async fn ui_type(&self, Parameters(params): Parameters<UiTypeParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::UiType {
                entity,
                text: params.text,
            })
            .await
    }

    // -- Playtest --

    #[tool(description = "Run a sequence of playtest steps (restart, input, assert, capture)")]
    async fn playtest_run(&self, Parameters(params): Parameters<PlaytestRunParams>) -> String {
        let steps: Vec<bevy_mcp_core::command::PlaytestStep> = params
            .steps
            .into_iter()
            .map(|s| match s.action.as_str() {
                "runtime_restart" => bevy_mcp_core::command::PlaytestStep::RuntimeRestart,
                "runtime_step" => bevy_mcp_core::command::PlaytestStep::RuntimeStep {
                    frames: s.frames.unwrap_or(1),
                },
                "input_action" => bevy_mcp_core::command::PlaytestStep::InputAction {
                    action: s.action_name.unwrap_or_default(),
                    duration_secs: s.duration.unwrap_or(1.0),
                },
                "capture_game" => bevy_mcp_core::command::PlaytestStep::CaptureGame {
                    name: s.name.unwrap_or_else(|| "unnamed".to_string()),
                },
                _ => bevy_mcp_core::command::PlaytestStep::RuntimeStep { frames: 1 },
            })
            .collect();

        self.state.call(McpCommand::PlaytestRun { steps }).await
    }

    #[tool(description = "Assert a condition about the game state")]
    async fn assert(&self, Parameters(params): Parameters<AssertParams>) -> String {
        let assertion = match params.assertion_type.as_str() {
            "entity_exists" => bevy_mcp_core::command::Assertion::EntityExists {
                entity_id: params.entity_id.unwrap_or(0),
            },
            "component_exists" => bevy_mcp_core::command::Assertion::ComponentExists {
                entity_id: params.entity_id.unwrap_or(0),
                component: params.component.unwrap_or_default(),
            },
            "entity_count" => bevy_mcp_core::command::Assertion::EntityCount {
                expected: params.expected_count.unwrap_or(0),
            },
            _ => return serde_json::json!({"error": "INVALID_ASSERTION", "message": format!("Unknown assertion type: {}", params.assertion_type)}).to_string(),
        };

        self.state.call(McpCommand::Assert { assertion }).await
    }

    #[tool(description = "List installed Bevy plugins and their capabilities")]
    async fn list_plugins(&self) -> String {
        self.state.call(McpCommand::ListPlugins).await
    }

    #[tool(description = "Get status of async operations (builds, tests)")]
    async fn operation_status(
        &self,
        Parameters(params): Parameters<OperationStatusParams>,
    ) -> String {
        self.state
            .call(McpCommand::OperationStatus {
                operation_id: params.operation_id,
            })
            .await
    }

    #[tool(description = "Cancel a running async operation")]
    async fn operation_cancel(
        &self,
        Parameters(params): Parameters<OperationCancelParams>,
    ) -> String {
        self.state
            .call(McpCommand::OperationCancel {
                operation_id: params.operation_id,
            })
            .await
    }

    // -- ECS inspection --

    #[tool(
        description = "Get a summary of the current world: entity count, archetype count, component type count"
    )]
    async fn world_summary(&self) -> String {
        self.state.call(McpCommand::WorldSummary).await
    }

    #[tool(
        description = "Get a comprehensive snapshot of the entire ECS world: entity count by archetype, all registered component types with field names and entity counts, all registered resources, full entity hierarchy tree, and current runtime state. One call for full project context."
    )]
    async fn world_context_scan(&self) -> String {
        self.state.call(McpCommand::WorldContextScan).await
    }

    #[tool(description = "Query entities by component filters. Returns matching entity handles.")]
    async fn entity_query(&self, Parameters(params): Parameters<EntityQueryParams>) -> String {
        self.state
            .call(McpCommand::EntityQuery {
                with_components: params.with_components.unwrap_or_default(),
                without_components: params.without_components.unwrap_or_default(),
                include: params.include.unwrap_or_default(),
                limit: params.limit.unwrap_or(100),
            })
            .await
    }

    #[tool(description = "Get full component data for a specific entity")]
    async fn entity_get(&self, Parameters(params): Parameters<EntityGetParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state.call(McpCommand::EntityGet { entity }).await
    }

    #[tool(description = "Get a specific component's value from an entity")]
    async fn component_get(&self, Parameters(params): Parameters<ComponentGetParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::ComponentGet {
                entity,
                component: params.component,
            })
            .await
    }

    #[tool(description = "Get the reflected schema for a component type (fields, types, defaults)")]
    async fn component_schema(
        &self,
        Parameters(params): Parameters<ComponentSchemaParams>,
    ) -> String {
        self.state
            .call(McpCommand::ComponentSchema {
                component: params.component,
            })
            .await
    }

    #[tool(description = "List all resources in the world")]
    async fn resource_list(&self) -> String {
        self.state.call(McpCommand::ResourceList).await
    }

    #[tool(description = "Get a resource's current value")]
    async fn resource_get(&self, Parameters(params): Parameters<ResourceGetParams>) -> String {
        self.state
            .call(McpCommand::ResourceGet {
                resource: params.resource,
            })
            .await
    }

    #[tool(description = "Get the reflected schema for a resource type")]
    async fn resource_schema(
        &self,
        Parameters(params): Parameters<ResourceSchemaParams>,
    ) -> String {
        self.state
            .call(McpCommand::ResourceSchema {
                resource: params.resource,
            })
            .await
    }

    #[tool(description = "Update a resource's value via reflection")]
    async fn resource_update(
        &self,
        Parameters(params): Parameters<ResourceUpdateParams>,
    ) -> String {
        self.state
            .call(McpCommand::ResourceUpdate {
                resource: params.resource,
                value: params.value,
            })
            .await
    }

    // -- ECS mutation --

    #[tool(description = "Spawn a new entity with optional components. Returns the entity handle.")]
    async fn entity_spawn(&self, Parameters(params): Parameters<EntitySpawnParams>) -> String {
        let components = params
            .components
            .unwrap_or_default()
            .into_iter()
            .map(|cv| (cv.component, cv.value))
            .collect();
        self.state
            .call(McpCommand::EntitySpawn { components })
            .await
    }

    #[tool(description = "Despawn an entity and all its children")]
    async fn entity_despawn(&self, Parameters(params): Parameters<EntityDespawnParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state.call(McpCommand::EntityDespawn { entity }).await
    }

    #[tool(description = "Insert a component on an entity")]
    async fn component_insert(
        &self,
        Parameters(params): Parameters<ComponentInsertParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::ComponentInsert {
                entity,
                component: params.component,
                value: params.value,
            })
            .await
    }

    #[tool(description = "Update a component's value on an entity")]
    async fn component_update(
        &self,
        Parameters(params): Parameters<ComponentUpdateParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::ComponentUpdate {
                entity,
                component: params.component,
                value: params.value,
            })
            .await
    }

    #[tool(description = "Remove a component from an entity")]
    async fn component_remove(
        &self,
        Parameters(params): Parameters<ComponentRemoveParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::ComponentRemove {
                entity,
                component: params.component,
            })
            .await
    }

    #[tool(description = "Reparent an entity under a new parent (null to make root)")]
    async fn entity_reparent(
        &self,
        Parameters(params): Parameters<EntityReparentParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        let parent = match params.parent {
            Some(parent) => match parse_entity_handle(&parent) {
                Ok(handle) => Some(handle),
                Err(message) => return error("INVALID_HANDLE", message),
            },
            None => None,
        };
        self.state
            .call(McpCommand::EntityReparent { entity, parent })
            .await
    }

    #[tool(description = "Duplicate an entity and all its components")]
    async fn entity_duplicate(
        &self,
        Parameters(params): Parameters<EntityDuplicateParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::EntityDuplicate { entity })
            .await
    }

    // -- Runtime control --

    #[tool(description = "Launch/run the Bevy application")]
    async fn runtime_launch(&self) -> String {
        self.state.call(McpCommand::RuntimeLaunch).await
    }

    #[tool(description = "Stop the running Bevy application")]
    async fn runtime_stop(&self) -> String {
        self.state.call(McpCommand::RuntimeStop).await
    }

    #[tool(description = "Restart the Bevy application (stop + launch)")]
    async fn runtime_restart(&self) -> String {
        self.state.call(McpCommand::RuntimeRestart).await
    }

    #[tool(description = "Pause the running Bevy application")]
    async fn runtime_pause(&self) -> String {
        self.state.call(McpCommand::RuntimePause).await
    }

    #[tool(description = "Resume a paused Bevy application")]
    async fn runtime_resume(&self) -> String {
        self.state.call(McpCommand::RuntimeResume).await
    }

    #[tool(description = "Advance the simulation by N frames (default 1)")]
    async fn runtime_step(&self, Parameters(params): Parameters<StepParams>) -> String {
        self.state
            .call(McpCommand::RuntimeStep {
                frames: params.frames.unwrap_or(1),
            })
            .await
    }

    #[tool(description = "Set the simulation time scale (1.0 = normal speed)")]
    async fn runtime_time_scale(&self, Parameters(params): Parameters<TimeScaleParams>) -> String {
        self.state
            .call(McpCommand::RuntimeTimeScale {
                scale: params.scale,
            })
            .await
    }

    // -- Input injection --

    #[tool(description = "Inject a keyboard key press/release")]
    async fn input_key(&self, Parameters(params): Parameters<InputKeyParams>) -> String {
        self.state
            .call(McpCommand::InputKey {
                key: params.key,
                pressed: params.pressed.unwrap_or(true),
            })
            .await
    }

    #[tool(description = "Inject a mouse event (motion or button)")]
    async fn input_mouse(&self, Parameters(params): Parameters<InputMouseParams>) -> String {
        match params.event.as_str() {
            "motion" => self.state.call(McpCommand::InputMouseMove {
                x: params.x.unwrap_or(0.0),
                y: params.y.unwrap_or(0.0),
            }).await,
            "button" => self.state.call(McpCommand::InputMouseButton {
                button: params.button.unwrap_or_else(|| "left".into()),
                pressed: params.pressed.unwrap_or(true),
                x: params.x,
                y: params.y,
            }).await,
            _ => serde_json::json!({"error": "INVALID_PARAMS", "message": "event must be 'motion' or 'button'"}).to_string(),
        }
    }

    #[tool(description = "Inject a high-level input action (e.g. 'move_forward', 'fire')")]
    async fn input_action(&self, Parameters(_params): Parameters<InputActionParams>) -> String {
        error(
            "NOT_IMPLEMENTED",
            "High-level input actions require a game-specific action adapter",
        )
    }

    #[tool(description = "Inject a gamepad button press/release")]
    async fn input_gamepad(&self, Parameters(params): Parameters<InputGamepadParams>) -> String {
        // Gamepad input requires GamepadInput resource which is not yet implemented.
        // For now, return a clear error.
        serde_json::json!({
            "error": "NOT_IMPLEMENTED",
            "message": format!("Gamepad injection not yet implemented (button={})", params.button)
        })
        .to_string()
    }

    // -- Capture --

    #[tool(description = "Capture a screenshot of the game viewport")]
    async fn capture_game(&self) -> String {
        self.state.call(McpCommand::CaptureGame).await
    }

    // -- Assets --

    #[tool(description = "List loaded assets, optionally filtered by type")]
    async fn asset_list(&self, Parameters(params): Parameters<AssetListParams>) -> String {
        self.state
            .call(McpCommand::AssetList {
                filter: params.filter,
            })
            .await
    }

    #[tool(description = "Get asset metadata by path")]
    async fn asset_get(&self, Parameters(params): Parameters<AssetGetParams>) -> String {
        self.state
            .call(McpCommand::AssetGet { path: params.path })
            .await
    }

    #[tool(description = "Get asset loading status")]
    async fn asset_status(&self, Parameters(params): Parameters<AssetStatusParams>) -> String {
        self.state
            .call(McpCommand::AssetStatus { path: params.path })
            .await
    }

    #[tool(description = "Force reload an asset")]
    async fn asset_reload(&self, Parameters(params): Parameters<AssetReloadParams>) -> String {
        self.state
            .call(McpCommand::AssetReload { path: params.path })
            .await
    }

    // -- Procedural assets --

    #[tool(
        description = "Spawn an entity with a procedural mesh, PBR material, and transform. Shapes: cube, sphere, plane, cylinder, torus."
    )]
    async fn mesh_spawn(&self, Parameters(params): Parameters<MeshSpawnParams>) -> String {
        let shape = params.shape.to_lowercase();
        match shape.as_str() {
            "cube" | "sphere" | "plane" | "cylinder" | "torus" => {}
            _ => {
                return error(
                    "INVALID_SHAPE",
                    format!(
                        "Unknown shape '{}'. Valid shapes: cube, sphere, plane, cylinder, torus",
                        params.shape
                    ),
                );
            }
        }

        let parent = match params.parent {
            Some(ref p) => match parse_entity_handle(p) {
                Ok(handle) => Some(handle),
                Err(message) => return error("INVALID_HANDLE", message),
            },
            None => None,
        };

        let color_val = params.color.unwrap_or(ColorValue {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: None,
        });
        let pos_val = params.position.unwrap_or(Vec3Value {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });

        self.state
            .call(McpCommand::MeshSpawn {
                shape,
                size: params.size.unwrap_or(1.0),
                radius: params.radius.unwrap_or(0.5),
                color: (
                    color_val.r,
                    color_val.g,
                    color_val.b,
                    color_val.a.unwrap_or(1.0),
                ),
                metallic: params.metallic.unwrap_or(0.0),
                roughness: params.roughness.unwrap_or(0.5),
                position: (pos_val.x, pos_val.y, pos_val.z),
                parent,
            })
            .await
    }

    #[tool(
        description = "Save an entity subtree as a JSON template. Serializes Name, Transform, and reflected components."
    )]
    async fn template_save(&self, Parameters(params): Parameters<TemplateSaveParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::TemplateSave {
                entity,
                name: params.name,
                path: params.path,
            })
            .await
    }

    #[tool(
        description = "Load a JSON template and spawn entities from it. Optionally override parent and position."
    )]
    async fn template_load(&self, Parameters(params): Parameters<TemplateLoadParams>) -> String {
        let parent = match params.parent {
            Some(ref p) => match parse_entity_handle(p) {
                Ok(handle) => Some(handle),
                Err(message) => return error("INVALID_HANDLE", message),
            },
            None => None,
        };
        let position = params.position.map(|p| (p.x, p.y, p.z));
        self.state
            .call(McpCommand::TemplateLoad {
                name: params.name,
                path: params.path,
                parent,
                position,
            })
            .await
    }

    #[tool(description = "List cameras in the scene")]
    async fn camera_list(&self) -> String {
        // Query for entities with Camera component.
        self.state
            .call(McpCommand::EntityQuery {
                with_components: vec!["Camera".to_string()],
                without_components: vec![],
                include: vec![],
                limit: 100,
            })
            .await
    }

    #[tool(description = "Move the inspection camera to frame a specific entity")]
    async fn camera_frame_entity(
        &self,
        Parameters(params): Parameters<CameraFrameParams>,
    ) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state
            .call(McpCommand::CameraFrameEntity { entity })
            .await
    }

    #[tool(description = "Inspect the current camera (position, rotation, FOV)")]
    async fn camera_inspect(&self) -> String {
        self.state.call(McpCommand::CameraInspect).await
    }

    #[tool(description = "Set camera position")]
    async fn camera_set_transform(
        &self,
        Parameters(params): Parameters<CameraSetTransformParams>,
    ) -> String {
        self.state
            .call(McpCommand::CameraSetTransform {
                x: params.x,
                y: params.y,
                z: params.z,
            })
            .await
    }

    #[tool(description = "Point camera at an entity")]
    async fn camera_look_at(&self, Parameters(params): Parameters<CameraLookAtParams>) -> String {
        let entity = match parse_entity_handle(&params.entity) {
            Ok(handle) => handle,
            Err(message) => return error("INVALID_HANDLE", message),
        };
        self.state.call(McpCommand::CameraLookAt { entity }).await
    }

    #[tool(description = "Capture a screenshot from the active camera")]
    async fn capture_camera(&self) -> String {
        self.state.call(McpCommand::CaptureCamera).await
    }

    // -- Logs & diagnostics --

    #[tool(description = "Get recent log output from the Bevy application")]
    async fn logs(&self, Parameters(params): Parameters<LogsParams>) -> String {
        self.state
            .call(McpCommand::Logs {
                level: params.level,
                limit: params.limit.unwrap_or(100),
            })
            .await
    }

    #[tool(description = "Get current diagnostics (FPS, frame time, entity count, memory)")]
    async fn diagnostics(&self) -> String {
        self.state.call(McpCommand::Diagnostics).await
    }

    // -- Build --

    #[tool(description = "Run cargo check on the project. Returns structured errors.")]
    async fn build_check(&self) -> String {
        error(
            "BUILD_NOT_AVAILABLE",
            "Build tools are disabled; run cargo from a trusted local terminal",
        )
    }

    #[tool(description = "Build the project with cargo. Returns structured build result.")]
    async fn build(&self, Parameters(_params): Parameters<BuildParams>) -> String {
        error(
            "BUILD_NOT_AVAILABLE",
            "Build tools are disabled; run cargo from a trusted local terminal",
        )
    }

    #[tool(description = "Run cargo test. Returns structured test results.")]
    async fn test(&self, Parameters(_params): Parameters<TestParams>) -> String {
        error(
            "BUILD_NOT_AVAILABLE",
            "Build tools are disabled; run cargo from a trusted local terminal",
        )
    }

    // -- Batch --

    #[tool(
        description = "Execute a limited set of read operations sequentially. Preview mode does not validate arguments."
    )]
    async fn batch(&self, Parameters(params): Parameters<BatchParams>) -> String {
        let mut results = Vec::new();
        let stop_on_error = params.stop_on_error.unwrap_or(true);
        let dry_run = params.dry_run.unwrap_or(false);
        let atomic = params.atomic.unwrap_or(false);
        let verify = params.verify.unwrap_or(false);

        if atomic || verify {
            return error(
                "UNSUPPORTED_BATCH_MODE",
                "Atomic rollback and verification are not implemented; use sequential mode or preview mode",
            );
        }

        // In dry_run mode, just validate the operations without executing.
        if dry_run {
            for op in &params.operations {
                results.push(serde_json::json!({
                    "tool": op.tool,
                    "status": "would_execute",
                    "arguments": op.arguments,
                }));
            }
            return serde_json::json!({
                "mode": "dry_run",
                "results": results,
                "count": results.len(),
            })
            .to_string();
        }

        for op in &params.operations {
            let result = match op.tool.as_str() {
                "health" => self.health().await,
                "world_summary" => self.world_summary().await,
                "entity_query" => {
                    if let Some(args) = &op.arguments {
                        if let Ok(p) = serde_json::from_value::<EntityQueryParams>(args.clone()) {
                            self.entity_query(Parameters(p)).await
                        } else {
                            serde_json::json!({"error": "INVALID_PARAMS"}).to_string()
                        }
                    } else {
                        serde_json::json!({"error": "MISSING_PARAMS"}).to_string()
                    }
                }
                "entity_get" => {
                    if let Some(args) = &op.arguments {
                        if let Ok(p) = serde_json::from_value::<EntityGetParams>(args.clone()) {
                            self.entity_get(Parameters(p)).await
                        } else {
                            serde_json::json!({"error": "INVALID_PARAMS"}).to_string()
                        }
                    } else {
                        serde_json::json!({"error": "MISSING_PARAMS"}).to_string()
                    }
                }
                "component_get" => {
                    if let Some(args) = &op.arguments {
                        if let Ok(p) = serde_json::from_value::<ComponentGetParams>(args.clone()) {
                            self.component_get(Parameters(p)).await
                        } else {
                            serde_json::json!({"error": "INVALID_PARAMS"}).to_string()
                        }
                    } else {
                        serde_json::json!({"error": "MISSING_PARAMS"}).to_string()
                    }
                }
                "component_schema" => {
                    if let Some(args) = &op.arguments {
                        if let Ok(p) = serde_json::from_value::<ComponentSchemaParams>(args.clone()) {
                            self.component_schema(Parameters(p)).await
                        } else {
                            serde_json::json!({"error": "INVALID_PARAMS"}).to_string()
                        }
                    } else {
                        serde_json::json!({"error": "MISSING_PARAMS"}).to_string()
                    }
                }
                "resource_list" => self.resource_list().await,
                "runtime_status" => self.runtime_status().await,
                "errors" => self.errors().await,
                _ => serde_json::json!({"error": "UNKNOWN_TOOL", "message": format!("Unknown tool: {}", op.tool)}).to_string(),
            };

            let is_error = result.contains("\"error\"");
            results.push(serde_json::json!({
                "tool": op.tool,
                "result": serde_json::from_str::<serde_json::Value>(&result).unwrap_or(serde_json::Value::String(result.clone())),
                "success": !is_error,
            }));

            if stop_on_error && is_error {
                break;
            }
        }

        let all_success = results
            .iter()
            .all(|r| r["success"].as_bool().unwrap_or(false));
        serde_json::json!({
            "mode": "sequential",
            "results": results,
            "count": results.len(),
            "all_success": all_success,
        })
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_entity_handle(uri: &str) -> Result<EntityHandle, String> {
    let handle = EntityHandle::from_uri(uri)?;
    if handle.instance != "default" || handle.world != "main" {
        return Err("unknown entity instance or world".to_owned());
    }
    u32::try_from(handle.id).map_err(|_| "entity ID is out of range".to_owned())?;
    u32::try_from(handle.generation).map_err(|_| "entity generation is out of range".to_owned())?;
    Ok(handle)
}

fn error(code: &str, message: impl Into<String>) -> String {
    serde_json::json!({ "error": code, "message": message.into() }).to_string()
}

fn format_result(result: McpResult) -> String {
    match result {
        McpResult::Success(value) => value.to_string(),
        McpResult::Error { code, message } => error(&code, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_handles_must_be_complete_and_in_the_default_world() {
        assert!(parse_entity_handle("entity://default/main/42/3").is_ok());
        assert!(parse_entity_handle("entity://default/main/42").is_err());
        assert!(parse_entity_handle("entity://other/main/42/3").is_err());
        assert!(parse_entity_handle("entity://default/main/42/3/ignored").is_err());
    }
}
