pub mod advanced;
pub mod agent_api;
pub mod change_tracking;
pub mod checkpoint;
pub mod command;
pub mod debugger;
pub mod deferred;
pub mod entity_handle;
pub mod event_capture;
pub mod instance;
pub mod interaction;
pub mod log_capture;
pub mod operations;
pub mod permissions;
pub mod plugin;
pub mod queue;
pub mod registry;
pub mod schedule;
pub mod supervisor_bridge;
pub mod synthetic_input;
pub mod systems;

pub use agent_api::{
    ActionResult, McpActionRegistry, McpAgentAppExt, McpCaptureTargets, McpStateRegistry,
    McpSystemAccessRegistry, McpSystemAccessSpec, McpSystemTimings,
};
pub use debugger::McpDebugger;
pub use permissions::{McpPermissions, PermissionLevel};
pub use plugin::BevyMcpPlugin;

pub use checkpoint::{McpCheckpointRegistry, McpCheckpointStore, McpRecorder, RecordedAction};
pub use instance::McpInstanceId;
pub use supervisor_bridge::SupervisorBridgeConfig;
