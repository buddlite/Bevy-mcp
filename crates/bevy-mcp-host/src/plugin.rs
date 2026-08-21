use bevy::prelude::*;

use crate::deferred::DeferredMcpCommands;
use crate::event_capture::EventCapture;
use crate::log_capture::LogCapture;
use crate::operations::OperationTracker;
use crate::permissions::McpPermissions;
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;
use crate::schedule::{McpSchedulePlugin, McpSet};
use crate::systems;

/// The main Bevy plugin that bridges MCP and ECS.
///
/// ```ignore
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(BevyMcpPlugin::new())
///     .run();
/// ```
pub struct BevyMcpPlugin {
    bevy_version: String,
    ingress: Option<McpIngressQueue>,
    results: Option<McpResultQueue>,
    log_capture: Option<LogCapture>,
    permissions: Option<McpPermissions>,
    event_capture: Option<EventCapture>,
    operation_tracker: Option<OperationTracker>,
}

impl BevyMcpPlugin {
    pub fn new() -> Self {
        Self {
            bevy_version: format!("{}.{}.{}", 0, 19, 1),
            ingress: None,
            results: None,
            log_capture: None,
            permissions: None,
            event_capture: None,
            operation_tracker: None,
        }
    }

    pub fn with_bevy_version(mut self, version: impl Into<String>) -> Self {
        self.bevy_version = version.into();
        self
    }

    /// Use externally-provided core queues (for sharing with the MCP server).
    pub fn with_queues(
        mut self,
        ingress: bevy_mcp_core::queue::McpIngressQueue,
        results: bevy_mcp_core::queue::McpResultQueue,
    ) -> Self {
        self.ingress = Some(McpIngressQueue::from_core(ingress));
        self.results = Some(McpResultQueue::from_core(results));
        self
    }

    /// Set the log capture instance for the `logs` tool.
    ///
    /// The `LogCapture` should also be installed as a tracing layer
    /// for it to receive log messages.
    pub fn with_log_capture(mut self, log_capture: LogCapture) -> Self {
        self.log_capture = Some(log_capture);
        self
    }

    /// Set the permission level for MCP operations.
    ///
    /// Controls what operations the MCP server can perform:
    /// - `read_only()`: query and inspect only
    /// - `write()`: query, inspect, and mutate ECS
    /// - `full()`: all operations including input and runtime control
    pub fn with_permissions(mut self, permissions: McpPermissions) -> Self {
        self.permissions = Some(permissions);
        self
    }

    /// Set the event capture instance for the `observe_events` tool.
    pub fn with_event_capture(mut self, event_capture: EventCapture) -> Self {
        self.event_capture = Some(event_capture);
        self
    }
}

impl Default for BevyMcpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for BevyMcpPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(McpSchedulePlugin);

        app.insert_resource(self.ingress.clone().unwrap_or_default());
        app.insert_resource(self.results.clone().unwrap_or_default());
        app.insert_resource(McpRegistry::new(&self.bevy_version));
        app.insert_resource(DeferredMcpCommands::default());
        app.insert_resource(
            self.log_capture
                .clone()
                .unwrap_or_else(|| LogCapture::new(1000)),
        );
        app.insert_resource(self.permissions.clone().unwrap_or_default());
        app.insert_resource(self.event_capture.clone().unwrap_or_default());
        app.insert_resource(self.operation_tracker.clone().unwrap_or_default());

        app.add_systems(
            PreUpdate,
            (
                systems::ingress_system.in_set(McpSet::Ingress),
                systems::runtime_system.in_set(McpSet::Apply),
            ),
        );

        // Deferred mutations run in Update (has &mut World).
        app.add_systems(Update, systems::deferred_apply_system);

        app.add_systems(
            PostUpdate,
            systems::diagnostics_system.in_set(McpSet::Diagnostics),
        );
    }
}
