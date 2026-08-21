use bevy::prelude::*;

/// Permission levels for MCP operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionLevel {
    /// No operations allowed.
    None,
    /// Read-only operations (query, inspect).
    #[default]
    Read,
    /// Read + write operations (spawn, despawn, insert, remove).
    Write,
    /// All operations including input injection and runtime control.
    Full,
}

/// Resource that controls what MCP operations are allowed.
///
/// Add this to your app to restrict what the MCP server can do:
///
/// ```ignore
/// App::new()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(BevyMcpPlugin::new())
///     .insert_resource(McpPermissions::read_only())
///     .run();
/// ```
#[derive(Resource, Debug, Clone)]
pub struct McpPermissions {
    pub level: PermissionLevel,
    pub allow_input: bool,
    pub allow_runtime_control: bool,
    pub allow_build: bool,
}

impl Default for McpPermissions {
    fn default() -> Self {
        Self {
            level: PermissionLevel::Read,
            allow_input: false,
            allow_runtime_control: false,
            allow_build: false,
        }
    }
}

impl McpPermissions {
    /// Read-only: can query and inspect, but not modify.
    pub fn read_only() -> Self {
        Self {
            level: PermissionLevel::Read,
            allow_input: false,
            allow_runtime_control: false,
            allow_build: false,
        }
    }

    /// Full access: all operations allowed.
    pub fn full() -> Self {
        Self {
            level: PermissionLevel::Full,
            allow_input: true,
            allow_runtime_control: true,
            allow_build: true,
        }
    }

    /// Read and ECS mutation access, but no input, runtime, or build access.
    pub fn write() -> Self {
        Self {
            level: PermissionLevel::Write,
            allow_input: false,
            allow_runtime_control: false,
            allow_build: false,
        }
    }

    /// Check if a mutation operation is allowed.
    pub fn can_mutate(&self) -> bool {
        matches!(self.level, PermissionLevel::Write | PermissionLevel::Full)
    }

    /// Check if input injection is allowed.
    pub fn can_inject_input(&self) -> bool {
        self.level == PermissionLevel::Full && self.allow_input
    }

    /// Check if runtime control is allowed.
    pub fn can_control_runtime(&self) -> bool {
        self.level == PermissionLevel::Full && self.allow_runtime_control
    }

    /// Check if build/test operations are allowed.
    pub fn can_build(&self) -> bool {
        self.allow_build
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_least_privilege() {
        let permissions = McpPermissions::default();
        assert!(!permissions.can_mutate());
        assert!(!permissions.can_inject_input());
        assert!(!permissions.can_control_runtime());
        assert!(!permissions.can_build());
    }

    #[test]
    fn write_excludes_input_and_runtime_control() {
        let permissions = McpPermissions::write();
        assert!(permissions.can_mutate());
        assert!(!permissions.can_inject_input());
        assert!(!permissions.can_control_runtime());
    }
}
