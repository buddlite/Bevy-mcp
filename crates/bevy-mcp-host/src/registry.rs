use bevy::prelude::Resource;

/// Control-plane metadata — NOT game state.
///
/// Tracks MCP connection state, capabilities, and runtime status.
/// Game state stays in normal ECS components/resources.
#[derive(Resource, Default)]
pub struct McpRegistry {
    /// Whether an MCP client is connected.
    pub connected: bool,
    /// Bevy version string.
    pub bevy_version: Option<String>,
    /// Current frame number.
    pub frame: u64,
    /// Whether the runtime is paused.
    pub paused: bool,
    /// Time scale multiplier.
    pub time_scale: f64,
    /// Number of frames to advance when paused (step mode).
    pub step_remaining: u32,
}

impl McpRegistry {
    pub fn new(bevy_version: impl Into<String>) -> Self {
        Self {
            connected: true,
            bevy_version: Some(bevy_version.into()),
            frame: 0,
            paused: false,
            time_scale: 1.0,
            step_remaining: 0,
        }
    }
}
