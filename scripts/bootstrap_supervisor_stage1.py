from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return (ROOT / path).read_text()


def write(path, text):
    (ROOT / path).write_text(text)


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)

# Workspace and module wiring.
path = "Cargo.toml"
text = read(path)
text = replace_once(
    text,
    '    "crates/bevy-mcp-server",\n',
    '    "crates/bevy-mcp-server",\n    "crates/bevy-mcp-supervisor",\n',
    "workspace supervisor member",
)
write(path, text)

path = "crates/bevy-mcp-core/src/lib.rs"
text = read(path)
if "pub mod wire;" not in text:
    text += "pub mod wire;\n"
write(path, text)

# Make the shared command/result surface wire-safe with explicit enum tags.
path = "crates/bevy-mcp-core/src/command.rs"
text = read(path)
if "use serde::{Deserialize, Serialize};" not in text:
    text = text.replace("use serde_json::Value;", "use serde::{Deserialize, Serialize};\nuse serde_json::Value;")
for kind, name, tag in [
    ("enum", "McpCommand", "command"),
    ("enum", "MutationOperation", "operation"),
    ("enum", "PlaytestStep", "step"),
    ("enum", "Assertion", "assertion"),
    ("enum", "McpResult", "status"),
]:
    old = f"#[derive(Debug, Clone)]\npub {kind} {name}"
    new = (
        f"#[derive(Debug, Clone, Serialize, Deserialize)]\n"
        f"#[serde(tag = \"{tag}\", content = \"payload\", rename_all = \"snake_case\")]\n"
        f"pub {kind} {name}"
    )
    text = replace_once(text, old, new, f"serde derive {name}")
text = replace_once(
    text,
    "#[derive(Debug, Clone)]\npub struct McpResponse",
    "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct McpResponse",
    "serde derive McpResponse",
)
text = replace_once(
    text,
    "    // -- Runtime --\n    RuntimeLaunch,",
    "    // -- Runtime --\n    /// Internal supervisor readiness probe. It is acknowledged only after the Bevy ingress path runs.\n    HostProbe { probe_id: u64 },\n    RuntimeLaunch,",
    "host probe command",
)
write(path, text)

# Server library exports the backend abstraction.
path = "crates/bevy-mcp-server/src/lib.rs"
text = read(path)
if "pub mod backend;" not in text:
    text = "pub mod backend;\n" + text
write(path, text)

# Rename the legacy standalone skeleton so the persistent supervisor owns `bevy-mcp`.
path = "crates/bevy-mcp-server/Cargo.toml"
text = read(path)
text = replace_once(text, 'name = "bevy-mcp"\npath = "src/main.rs"', 'name = "bevy-mcp-embedded"\npath = "src/main.rs"', "server binary rename")
write(path, text)

# Unify the legacy state on GameCommandBackend.
path = "crates/bevy-mcp-server/src/tools.rs"
text = read(path)
text = text.replace("use std::sync::atomic::Ordering;\n", "use std::sync::atomic::AtomicBool;\n")
text = text.replace(
    "use crate::response_dispatcher::McpResponseDispatcher;",
    "use crate::backend::{EmbeddedBackend, SharedGameCommandBackend, format_backend_result};",
)
start = text.index("#[derive(Clone)]\npub struct BevyMcpState")
end = text.index("// ---------------------------------------------------------------------------\n// Server", start)
replacement = '''#[derive(Clone)]
pub struct BevyMcpState {
    backend: SharedGameCommandBackend,
}

impl BevyMcpState {
    pub fn new(ingress: McpIngressQueue, results: McpResultQueue) -> Self {
        let connected = Arc::new(AtomicBool::new(false));
        Self {
            backend: Arc::new(EmbeddedBackend::new(ingress, results, connected)),
        }
    }

    /// Construct state for an MCP server embedded in the same process as Bevy.
    /// The caller must give the same queues to `BevyMcpPlugin::with_queues`.
    pub fn embedded(ingress: McpIngressQueue, results: McpResultQueue) -> Self {
        let connected = Arc::new(AtomicBool::new(true));
        Self {
            backend: Arc::new(EmbeddedBackend::new(ingress, results, connected)),
        }
    }

    /// Construct the MCP tool surface over an arbitrary game-command transport.
    pub fn from_backend(backend: SharedGameCommandBackend) -> Self {
        Self { backend }
    }

    pub(crate) fn backend(&self) -> SharedGameCommandBackend {
        self.backend.clone()
    }

    async fn call(&self, command: McpCommand) -> String {
        format_backend_result(
            self.backend
                .call(command, std::time::Duration::from_secs(5))
                .await,
        )
    }
}

'''
text = text[:start] + replacement + text[end:]
write(path, text)

# Advanced surface uses the same backend, with its existing longer timeout.
path = "crates/bevy-mcp-server/src/advanced_tools.rs"
text = read(path)
text = text.replace(
    "use crate::response_dispatcher::McpResponseDispatcher;\nuse crate::tools::{BevyMcpServer, BevyMcpState};",
    "use crate::backend::{SharedGameCommandBackend, format_backend_result};\nuse crate::tools::{BevyMcpServer, BevyMcpState};",
)
old = '''#[derive(Clone)]
struct AdvancedMcpState {
    dispatcher: McpResponseDispatcher,
}

impl AdvancedMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            dispatcher: state.dispatcher.clone(),
        }
    }

    async fn call(&self, request: AdvancedRequest) -> String {
        let operation_id = match encode_advanced_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "ADVANCED_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        self.dispatcher
            .call(
                McpCommand::OperationStatus {
                    operation_id: Some(operation_id),
                },
                std::time::Duration::from_secs(15),
            )
            .await
    }
}
'''
new = '''#[derive(Clone)]
struct AdvancedMcpState {
    backend: SharedGameCommandBackend,
}

impl AdvancedMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            backend: state.backend(),
        }
    }

    async fn call(&self, request: AdvancedRequest) -> String {
        let operation_id = match encode_advanced_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "ADVANCED_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        format_backend_result(
            self.backend
                .call(
                    McpCommand::OperationStatus {
                        operation_id: Some(operation_id),
                    },
                    std::time::Duration::from_secs(15),
                )
                .await,
        )
    }
}
'''
text = replace_once(text, old, new, "advanced backend state")
write(path, text)

# Debugger/playtest surface also uses the same backend.
path = "crates/bevy-mcp-server/src/debug_tools.rs"
text = read(path)
text = text.replace(
    "use crate::response_dispatcher::McpResponseDispatcher;",
    "use crate::backend::{SharedGameCommandBackend, format_backend_result};",
)
old = '''#[derive(Clone)]
struct DebugMcpState {
    dispatcher: McpResponseDispatcher,
}

impl DebugMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            dispatcher: state.dispatcher.clone(),
        }
    }

    async fn call(&self, request: DebugRequest) -> String {
        let operation_id = match encode_debug_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "DEBUG_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        self.dispatcher
            .call(
                McpCommand::OperationStatus {
                    operation_id: Some(operation_id),
                },
                std::time::Duration::from_secs(5),
            )
            .await
    }
}
'''
new = '''#[derive(Clone)]
struct DebugMcpState {
    backend: SharedGameCommandBackend,
}

impl DebugMcpState {
    fn from_base(state: &BevyMcpState) -> Self {
        Self {
            backend: state.backend(),
        }
    }

    async fn call(&self, request: DebugRequest) -> String {
        let operation_id = match encode_debug_request(&request) {
            Ok(value) => value,
            Err(error) => {
                return serde_json::json!({
                    "error": "DEBUG_REQUEST_ENCODING_FAILED",
                    "message": error.to_string(),
                })
                .to_string();
            }
        };
        format_backend_result(
            self.backend
                .call(
                    McpCommand::OperationStatus {
                        operation_id: Some(operation_id),
                    },
                    std::time::Duration::from_secs(5),
                )
                .await,
        )
    }
}
'''
text = replace_once(text, old, new, "debug backend state")
write(path, text)

# Host exports identity and bridge integration.
path = "crates/bevy-mcp-host/src/lib.rs"
text = read(path)
if "pub mod instance;" not in text:
    text = text.replace("pub mod interaction;\n", "pub mod interaction;\npub mod instance;\n")
if "pub mod supervisor_bridge;" not in text:
    text = text.replace("pub mod systems;\n", "pub mod systems;\npub mod supervisor_bridge;\n")
if "pub use instance::McpInstanceId;" not in text:
    text += "pub use instance::McpInstanceId;\n"
if "pub use supervisor_bridge::SupervisorBridgeConfig;" not in text:
    text += "pub use supervisor_bridge::SupervisorBridgeConfig;\n"
write(path, text)

# Plugin owns only game-side identity and explicitly enabled bridge configuration.
path = "crates/bevy-mcp-host/src/plugin.rs"
text = read(path)
text = text.replace(
    "use crate::interaction::{self, McpInteractionState, mcp_pointer_id};",
    "use crate::interaction::{self, McpInteractionState, mcp_pointer_id};\nuse crate::instance::McpInstanceId;",
)
text = text.replace(
    "use crate::systems;",
    "use crate::systems;\nuse crate::supervisor_bridge::{SupervisorBridgeConfig, spawn_supervisor_bridge};",
)
text = replace_once(
    text,
    "    operation_tracker: Option<OperationTracker>,\n}",
    "    operation_tracker: Option<OperationTracker>,\n    instance_id: String,\n    supervisor_bridge: Option<SupervisorBridgeConfig>,\n}",
    "plugin fields",
)
text = replace_once(
    text,
    "            operation_tracker: None,\n        }",
    "            operation_tracker: None,\n            instance_id: \"default\".to_string(),\n            supervisor_bridge: None,\n        }",
    "plugin defaults",
)
anchor = '''    pub fn with_queues(
        mut self,
        ingress: bevy_mcp_core::queue::McpIngressQueue,
        results: bevy_mcp_core::queue::McpResultQueue,
    ) -> Self {
        self.ingress = Some(McpIngressQueue::from_core(ingress));
        self.results = Some(McpResultQueue::from_core(results));
        self
    }
'''
addition = anchor + '''
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
'''
text = replace_once(text, anchor, addition, "plugin bridge methods")
text = replace_once(
    text,
    "        app.insert_resource(self.ingress.clone().unwrap_or_default());\n        app.insert_resource(self.results.clone().unwrap_or_default());",
    '''        let ingress = self.ingress.clone().unwrap_or_default();
        let results = self.results.clone().unwrap_or_default();
        app.insert_resource(ingress.clone());
        app.insert_resource(results.clone());
        app.insert_resource(McpInstanceId::new(self.instance_id.clone()));
        if let Some(config) = self.supervisor_bridge.clone() {
            if let Err(error) = spawn_supervisor_bridge(
                config,
                ingress.inner().clone(),
                results.inner().clone(),
            ) {
                tracing::error!(%error, "failed to start bevy-mcp supervisor bridge");
            }
        }''',
    "plugin queue bridge wiring",
)
write(path, text)

# Replace entity namespace handling with instance-aware generation and validation.
path = "crates/bevy-mcp-host/src/entity_handle.rs"
write(path, '''use bevy::ecs::entity::Entity;
use bevy::prelude::World;
use bevy_mcp_core::command::{McpCommand, McpResult, MutationOperation};
use bevy_mcp_core::entity_handle::EntityHandle;

use crate::instance::McpInstanceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityResolveError {
    StaleInstance,
    InvalidWorld,
    NotFound,
}

fn current_instance(world: &World) -> &str {
    world
        .get_resource::<McpInstanceId>()
        .map(McpInstanceId::as_str)
        .unwrap_or("default")
}

pub fn validate_entity_namespace(
    world: &World,
    handle: &EntityHandle,
) -> Result<(), EntityResolveError> {
    if handle.instance != current_instance(world) {
        return Err(EntityResolveError::StaleInstance);
    }
    if handle.world != "main" {
        return Err(EntityResolveError::InvalidWorld);
    }
    Ok(())
}

pub fn resolve_entity_checked(
    world: &World,
    handle: &EntityHandle,
) -> Result<Entity, EntityResolveError> {
    validate_entity_namespace(world, handle)?;
    let index = u32::try_from(handle.id).map_err(|_| EntityResolveError::NotFound)?;
    let generation = u32::try_from(handle.generation).map_err(|_| EntityResolveError::NotFound)?;
    let index = bevy::ecs::entity::EntityIndex::from_raw_u32(index)
        .ok_or(EntityResolveError::NotFound)?;
    let entity = Entity::from_index_and_generation(
        index,
        bevy::ecs::entity::EntityGeneration::from_bits(generation),
    );
    world
        .get_entity(entity)
        .map(|_| entity)
        .map_err(|_| EntityResolveError::NotFound)
}

pub fn resolve_entity(world: &World, handle: &EntityHandle) -> Option<Entity> {
    resolve_entity_checked(world, handle).ok()
}

fn namespace_error(world: &World, handle: &EntityHandle) -> Option<McpResult> {
    match validate_entity_namespace(world, handle) {
        Ok(()) => None,
        Err(EntityResolveError::StaleInstance) => Some(McpResult::error(
            "STALE_INSTANCE",
            format!(
                "Entity handle belongs to game instance {}; current instance is {}",
                handle.instance,
                current_instance(world)
            ),
        )),
        Err(EntityResolveError::InvalidWorld) => Some(McpResult::error(
            "INVALID_WORLD",
            format!("Entity handle world '{}' is not supported", handle.world),
        )),
        Err(EntityResolveError::NotFound) => None,
    }
}

pub fn validate_command_entity_handles(world: &World, command: &McpCommand) -> Option<McpResult> {
    let check = |handle: &EntityHandle| namespace_error(world, handle);
    match command {
        McpCommand::EntityGet { entity }
        | McpCommand::EntityDespawn { entity }
        | McpCommand::ComponentGet { entity, .. }
        | McpCommand::ComponentInsert { entity, .. }
        | McpCommand::ComponentUpdate { entity, .. }
        | McpCommand::ComponentRemove { entity, .. }
        | McpCommand::CameraFrameEntity { entity, .. }
        | McpCommand::CameraLookAt { entity }
        | McpCommand::UiInspect { entity }
        | McpCommand::UiClick { entity }
        | McpCommand::UiType { entity, .. }
        | McpCommand::EntityDuplicate { entity }
        | McpCommand::TemplateSave { entity, .. } => check(entity),
        McpCommand::Hierarchy { root, .. } | McpCommand::UiQuery { root, .. } => {
            root.as_ref().and_then(check)
        }
        McpCommand::EntityReparent { entity, parent } => {
            check(entity).or_else(|| parent.as_ref().and_then(check))
        }
        McpCommand::MeshSpawn { parent, .. } | McpCommand::TemplateLoad { parent, .. } => {
            parent.as_ref().and_then(check)
        }
        McpCommand::AtomicMutationBatch { operations, .. } => operations.iter().find_map(|operation| {
            let entity = match operation {
                MutationOperation::ComponentInsert { entity, .. }
                | MutationOperation::ComponentUpdate { entity, .. }
                | MutationOperation::ComponentRemove { entity, .. } => Some(entity),
                MutationOperation::ResourceUpdate { .. } => None,
            };
            entity.and_then(check)
        }),
        _ => None,
    }
}

/// Resolve an index-only entity reference for legacy assertion parameters.
pub fn resolve_entity_by_index(world: &World, index: u32) -> Option<Entity> {
    world
        .iter_entities()
        .map(|entity| entity.id())
        .find(|entity| entity.index().index() == index)
}

/// Build an entity handle URI scoped to the current game-process instance.
pub fn entity_to_uri(world: &World, entity: Entity) -> String {
    format!(
        "entity://{}/main/{}/{}",
        current_instance(world),
        entity.index().index(),
        entity.generation()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_instance_is_distinct_from_missing_entity() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-new"));
        let entity = world.spawn_empty().id();
        let stale = EntityHandle::new(
            "run-old",
            "main",
            entity.index().index() as u64,
            entity.generation() as u64,
        );
        assert_eq!(
            resolve_entity_checked(&world, &stale),
            Err(EntityResolveError::StaleInstance)
        );
        let result = validate_command_entity_handles(
            &world,
            &McpCommand::EntityGet { entity: stale },
        )
        .unwrap();
        assert!(matches!(
            result,
            McpResult::Error { ref code, .. } if code == "STALE_INSTANCE"
        ));
    }

    #[test]
    fn generated_handle_uses_current_instance() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-current"));
        let entity = world.spawn_empty().id();
        assert!(entity_to_uri(&world, entity).starts_with("entity://run-current/main/"));
    }
}
''')

# Update all host URI generation call sites to pass the World used to create the handle.
for file in (ROOT / "crates/bevy-mcp-host/src").rglob("*.rs"):
    if file.name == "entity_handle.rs":
        continue
    source = file.read_text()
    if "entity_to_uri(" in source:
        source = source.replace("entity_to_uri(", "entity_to_uri(world, ")
        file.write_text(source)

# Import the namespace validator into the systems facade.
path = "crates/bevy-mcp-host/src/systems.rs"
text = read(path)
text = text.replace(
    "use crate::entity_handle::{entity_to_uri, resolve_entity, resolve_entity_by_index};",
    "use crate::entity_handle::{\n    entity_to_uri, resolve_entity, resolve_entity_by_index, validate_command_entity_handles,\n};",
)
write(path, text)

# Enforce namespace validation before permission/dispatch and add the frame-aware probe.
path = "crates/bevy-mcp-host/src/systems/dispatch.rs"
text = read(path)
text = text.replace(
    "        McpCommand::Capabilities => true,",
    "        McpCommand::Capabilities | McpCommand::HostProbe { .. } => true,",
)
needle = '''    for entry in entries {
        let allowed = {'''
replacement = '''    for entry in entries {
        if let Some(result) = validate_command_entity_handles(world, &entry.command) {
            world.resource::<McpResultQueue>().push(McpResponse {
                request_id: entry.request_id,
                result,
            });
            continue;
        }

        let allowed = {'''
text = replace_once(text, needle, replacement, "namespace prevalidation")
text = text.replace(
    "        McpCommand::Capabilities => capabilities(world),",
    '''        McpCommand::Capabilities => capabilities(world),
        McpCommand::HostProbe { probe_id } => McpResult::success(json!({
            "probe_id": probe_id,
            "instance_id": world.resource::<crate::instance::McpInstanceId>().as_str(),
            "frame": registry.frame,
        })),''',
)
text += '''

#[cfg(test)]
mod supervisor_stage1_tests {
    use super::*;
    use crate::instance::McpInstanceId;

    #[test]
    fn host_probe_is_acknowledged_by_normal_command_execution() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("run-test"));
        let mut registry = McpRegistry::new("0.19.1");
        registry.frame = 41;
        let result = execute_command(
            &world,
            &McpCommand::HostProbe { probe_id: 7 },
            &mut registry,
        );
        match result {
            McpResult::Success(value) => {
                assert_eq!(value["probe_id"], 7);
                assert_eq!(value["instance_id"], "run-test");
                assert_eq!(value["frame"], 41);
            }
            other => panic!("expected successful probe, got {other:?}"),
        }
    }
}
'''
write(path, text)

print("Stage 1 source integration complete")
