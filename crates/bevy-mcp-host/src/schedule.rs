use bevy::prelude::*;

/// MCP schedule sets that run at known boundaries.
///
/// PreUpdate:
///   McpIngress → McpValidate → McpApply
///
/// PostUpdate:
///   McpCapture → McpDiagnostics → McpEgress
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum McpSet {
    /// Drain commands from the ingress queue.
    Ingress,
    /// Validate commands (entity existence, permissions, schema).
    Validate,
    /// Apply validated commands to the ECS.
    Apply,
    /// Capture screenshots, visual state.
    Capture,
    /// Collect diagnostics (FPS, entity count, etc.).
    Diagnostics,
    /// Push results to the egress queue.
    Egress,
}

/// Plugin that registers MCP schedules into the app.
pub struct McpSchedulePlugin;

impl Plugin for McpSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PreUpdate,
            (McpSet::Ingress, McpSet::Validate, McpSet::Apply).chain(),
        );

        app.configure_sets(
            PostUpdate,
            (McpSet::Capture, McpSet::Diagnostics, McpSet::Egress).chain(),
        );
    }
}
