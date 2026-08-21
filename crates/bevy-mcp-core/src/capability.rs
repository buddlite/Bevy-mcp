use serde::{Deserialize, Serialize};

/// Capabilities the MCP server advertises to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub ecs: EcsCapabilities,
    pub runtime: RuntimeCapabilities,
    pub input: InputCapabilities,
    pub capture: CaptureCapabilities,
    pub assets: AssetCapabilities,
    pub diagnostics: DiagnosticsCapabilities,
    pub build: BuildCapabilities,
    #[serde(default)]
    pub physics: Option<PhysicsAdapter>,
    #[serde(default)]
    pub ui: Option<UiCapabilities>,
    #[serde(default)]
    pub editor: Option<EditorCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcsCapabilities {
    pub inspect: bool,
    pub mutate: bool,
    pub query: bool,
    pub hierarchy: bool,
    pub reflection: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub control: bool,
    pub step: bool,
    pub time_scale: bool,
    pub pause: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputCapabilities {
    pub raw: bool,
    pub actions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCapabilities {
    pub game: bool,
    pub camera: bool,
    pub depth: bool,
    pub entity_mask: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCapabilities {
    pub inspect: bool,
    pub reload: bool,
    pub search: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsCapabilities {
    pub render: bool,
    pub performance: bool,
    pub logs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildCapabilities {
    pub cargo: bool,
    pub check: bool,
    pub test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsAdapter {
    pub adapter: String,
    pub raycast: bool,
    pub overlap: bool,
    pub contacts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiCapabilities {
    pub inspect: bool,
    pub hit_test: bool,
    pub interaction: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorCapabilities {
    pub selection: bool,
    pub undo: bool,
    pub viewport_capture: bool,
}
