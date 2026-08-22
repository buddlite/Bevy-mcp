pub mod advanced;
pub mod agent_api;
pub mod change_tracking;
pub mod checkpoint;
pub mod command;
pub mod debugger;
pub mod deferred;
pub mod entity_handle;
pub mod event_capture;
pub mod log_capture;
pub mod operations;
pub mod permissions;
pub mod plugin;
pub mod queue;
pub mod registry;
pub mod schedule;
pub mod systems;

pub use agent_api::{
    ActionResult, McpActionRegistry, McpAgentAppExt, McpCaptureTargets, McpStateRegistry,
    McpSystemTimings,
};
pub use debugger::McpDebugger;
pub use permissions::{McpPermissions, PermissionLevel};
pub use plugin::BevyMcpPlugin;

pub use checkpoint::{McpCheckpointRegistry, McpCheckpointStore, McpRecorder, RecordedAction};
