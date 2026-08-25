use bevy::prelude::*;
use serde::de::DeserializeSeed;
use serde_json::{Value, json};

use crate::deferred::{DeferredCommand, DeferredMcpCommands};
use crate::entity_handle::{
    entity_to_uri, resolve_entity, resolve_entity_by_index, validate_command_entity_handles,
};
use crate::permissions::{McpPermissions, PermissionLevel};
use crate::queue::{McpIngressQueue, McpResultQueue};
use crate::registry::McpRegistry;
use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult, MutationOperation};

mod assertions;
mod assets;
mod camera;
mod capabilities;
mod dispatch;
mod ecs_inspect;
mod ecs_mutate;
mod input;
mod procedural;
mod resources;
mod runtime;
mod ui;

pub use dispatch::{deferred_apply_system, ingress_system};
pub use runtime::{diagnostics_system, runtime_system};

/// Resolve a reflected type using Bevy's indexed registry APIs.
///
/// Full type paths always win. Short paths are accepted only when Bevy considers
/// them unambiguous; `get_with_short_type_path` returns `None` for collisions.
pub(crate) fn find_type_registration<'a>(
    registry: &'a bevy::reflect::TypeRegistry,
    name: &str,
) -> Option<&'a bevy::reflect::TypeRegistration> {
    registry
        .get_with_type_path(name)
        .or_else(|| registry.get_with_short_type_path(name))
}

#[cfg(test)]
mod bevy_019_type_registry_tests {
    use super::*;
    use bevy::reflect::{TypePath, TypeRegistry};

    mod left {
        use bevy::reflect::Reflect;
        #[derive(Reflect)]
        pub struct Duplicate;
    }

    mod right {
        use bevy::reflect::Reflect;
        #[derive(Reflect)]
        pub struct Duplicate;
    }

    #[test]
    fn ambiguous_short_paths_require_a_full_type_path() {
        let mut registry = TypeRegistry::default();
        registry.register::<left::Duplicate>();
        registry.register::<right::Duplicate>();
        assert!(registry.is_ambiguous("Duplicate"));
        assert!(find_type_registration(&registry, "Duplicate").is_none());
        assert!(
            find_type_registration(&registry, <left::Duplicate as TypePath>::type_path()).is_some()
        );
    }
}
