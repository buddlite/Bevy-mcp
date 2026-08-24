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
