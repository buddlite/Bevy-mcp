use bevy::prelude::Resource;

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct McpInstanceId(String);

impl McpInstanceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for McpInstanceId {
    fn default() -> Self {
        Self("default".to_string())
    }
}
