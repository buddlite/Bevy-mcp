use bevy::prelude::*;

use crate::advanced;
use crate::agent_api::{
    McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemAccessRegistry,
    McpSystemTimings,
};
use crate::change_tracking::{self, WorldChangeTracker};
use crate::checkpoint::{McpCheckpointRegistry, McpCheckpointStore, McpRecorder};
use crate::debugger::{self, McpDebugger};
use crate::deferred::DeferredMcpCommands;
use crate::event_capture::EventCapture;
use crate::instance::McpInstanceId;
use crate::interaction::{self, McpInteractionState, mcp_pointer_id};
use crate::log_capture::LogCapture;
use crate::operations::OperationTracker;
use crate::permissions::McpPermissions;
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;
use crate::schedule::{McpSchedulePlugin, McpSet};
use crate::supervisor_bridge::{
    SupervisorBridgeConfig, SupervisorShutdownSignal, spawn_supervisor_bridge,
    supervisor_shutdown_system,
};
use crate::synthetic_input::{self, McpSyntheticInputQueue};
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
    instance_id: String,
    supervisor_bridge: Option<SupervisorBridgeConfig>,
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
            instance_id: "default".to_string(),
            supervisor_bridge: None,
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

    pub fn with_instance_id(mut self, instance_id: impl Into<String>) -> Self {
        self.instance_id = instance_id.into();
        self
    }

    /// Explicitly enable the external supervisor bridge for this Bevy app.
    pub fn with_supervisor_bridge(mut self, config: SupervisorBridgeConfig) -> Self {
        self.instance_id = config.instance_id.clone();
        self.supervisor_bridge = Some(config);
        self
    }

    pub fn with_supervisor_bridge_from_env(self) -> Result<Self, String> {
        Ok(self.with_supervisor_bridge(SupervisorBridgeConfig::from_env()?))
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

        let ingress = self.ingress.clone().unwrap_or_default();
        let results = self.results.clone().unwrap_or_default();
        app.insert_resource(ingress.clone());
        app.insert_resource(results.clone());
        app.insert_resource(McpInstanceId::new(self.instance_id.clone()));
        let supervisor_shutdown = SupervisorShutdownSignal::default();
        app.insert_resource(supervisor_shutdown.clone());
        if let Some(config) = self.supervisor_bridge.clone() {
            if let Err(error) = spawn_supervisor_bridge(
                config,
                ingress.inner().clone(),
                results.inner().clone(),
                supervisor_shutdown,
            ) {
                tracing::error!(%error, "failed to start bevy-mcp supervisor bridge");
            }
        }
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
        app.init_resource::<McpActionRegistry>();
        app.init_resource::<McpStateRegistry>();
        app.init_resource::<McpCaptureTargets>();
        app.init_resource::<McpSystemTimings>();
        app.init_resource::<McpSystemAccessRegistry>();
        app.init_resource::<WorldChangeTracker>();
        app.init_resource::<McpCheckpointRegistry>();
        app.init_resource::<McpCheckpointStore>();
        app.init_resource::<McpRecorder>();
        app.init_resource::<McpDebugger>();
        app.init_resource::<McpInteractionState>();
        app.init_resource::<McpSyntheticInputQueue>();
        let pointer_entity = app.world_mut().spawn(mcp_pointer_id()).id();
        app.world_mut()
            .resource_mut::<McpInteractionState>()
            .set_pointer_entity(pointer_entity);

        // Bevy updates Time<Virtual> in First::TimeSystems. Apply our persisted pause/step/speed
        // state immediately before that clock update so each stepped frame has deterministic delta.
        app.add_systems(
            First,
            systems::runtime_system.before(bevy::time::TimeSystems),
        );

        app.add_systems(
            PreUpdate,
            (
                supervisor_shutdown_system.before(debugger::debug_ingress_system),
                debugger::debug_ingress_system
                    .before(advanced::advanced_ingress_system)
                    .before(synthetic_input::synthetic_input_ingress_system)
                    .before(systems::ingress_system)
                    .in_set(McpSet::Ingress),
                advanced::advanced_ingress_system
                    .before(synthetic_input::synthetic_input_ingress_system)
                    .before(systems::ingress_system)
                    .in_set(McpSet::Ingress),
                synthetic_input::synthetic_input_ingress_system
                    .before(systems::ingress_system)
                    .in_set(McpSet::Ingress),
                systems::ingress_system.in_set(McpSet::Ingress),
            ),
        );

        app.add_systems(
            PreUpdate,
            (
                synthetic_input::synthetic_input_apply_system
                    .after(bevy::input::InputSystems)
                    .after(McpSet::Ingress),
                interaction::interaction_input_system
                    .after(McpSet::Ingress)
                    .before(bevy::picking::PickingSystems::ProcessInput),
                synthetic_input::synthetic_pointer_button_system
                    .after(bevy::input::InputSystems)
                    .after(interaction::interaction_input_system)
                    .before(bevy::picking::PickingSystems::ProcessInput),
                interaction::interaction_result_system.after(bevy::picking::PickingSystems::Last),
            ),
        );

        // General reflected ECS/resource mutations run in Update after input state has settled.
        app.add_systems(Update, systems::deferred_apply_system);

        app.add_systems(
            PostUpdate,
            (
                change_tracking::track_world_changes.in_set(McpSet::Capture),
                debugger::debug_tick_system
                    .after(change_tracking::track_world_changes)
                    .in_set(McpSet::Capture),
                systems::diagnostics_system.in_set(McpSet::Diagnostics),
            ),
        );
    }
}
