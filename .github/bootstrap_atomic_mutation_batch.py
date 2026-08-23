from pathlib import Path


def read(path: str) -> str:
    return Path(path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    Path(path).write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected exactly one literal match, found {count}")
    write(path, text.replace(old, new, 1))


CORE = "crates/bevy-mcp-core/src/command.rs"
DEFERRED = "crates/bevy-mcp-host/src/deferred.rs"
SYSTEMS = "crates/bevy-mcp-host/src/systems.rs"
TOOLS = "crates/bevy-mcp-server/src/tools.rs"
README = "README.md"
TESTS = "crates/bevy-mcp-host/tests/atomic_mutation_batch.rs"

# ---------------------------------------------------------------------------
# Core protocol
# ---------------------------------------------------------------------------

replace_once(
    CORE,
    """    ComponentRemove {\n        entity: EntityHandle,\n        component: String,\n    },\n\n    // -- Runtime --""",
    """    ComponentRemove {\n        entity: EntityHandle,\n        component: String,\n    },\n    AtomicMutationBatch {\n        operations: Vec<MutationOperation>,\n        dry_run: bool,\n    },\n\n    // -- Runtime --""",
)

replace_once(
    CORE,
    """}\n\n/// A single step in a playtest scenario.""",
    """}\n\n/// A reflected ECS/resource mutation that can participate in an atomic batch.\n#[derive(Debug, Clone)]\npub enum MutationOperation {\n    ComponentInsert {\n        entity: EntityHandle,\n        component: String,\n        value: Value,\n    },\n    ComponentUpdate {\n        entity: EntityHandle,\n        component: String,\n        value: Value,\n    },\n    ComponentRemove {\n        entity: EntityHandle,\n        component: String,\n    },\n    ResourceUpdate {\n        resource: String,\n        value: Value,\n    },\n}\n\n/// A single step in a playtest scenario.""",
)

# ---------------------------------------------------------------------------
# Deferred queue
# ---------------------------------------------------------------------------

replace_once(
    DEFERRED,
    "use bevy_mcp_core::command::McpCommand;",
    "use bevy_mcp_core::command::{McpCommand, MutationOperation};",
)

replace_once(
    DEFERRED,
    """    RemoveComponent {\n        entity: Entity,\n        component: String,\n        result_id: u64,\n    },\n    InputKey {""",
    """    RemoveComponent {\n        entity: Entity,\n        component: String,\n        result_id: u64,\n    },\n    AtomicMutationBatch {\n        operations: Vec<MutationOperation>,\n        dry_run: bool,\n        result_id: u64,\n    },\n    InputKey {""",
)

# ---------------------------------------------------------------------------
# Host routing, validation, commit, capability contract
# ---------------------------------------------------------------------------

replace_once(
    SYSTEMS,
    "use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult};",
    "use bevy_mcp_core::command::{McpCommand, McpResponse, McpResult, MutationOperation};",
)

replace_once(
    SYSTEMS,
    """        | McpCommand::ComponentRemove { .. }\n        | McpCommand::ResourceUpdate { .. }""",
    """        | McpCommand::ComponentRemove { .. }\n        | McpCommand::AtomicMutationBatch { .. }\n        | McpCommand::ResourceUpdate { .. }""",
)

replace_once(
    SYSTEMS,
    """            McpCommand::InputKey { key, pressed } => {""",
    """            McpCommand::AtomicMutationBatch { operations, dry_run } => {\n                world.resource_mut::<DeferredMcpCommands>().pending.push(\n                    DeferredCommand::AtomicMutationBatch {\n                        operations: operations.clone(),\n                        dry_run: *dry_run,\n                        result_id: entry.request_id,\n                    },\n                );\n            }\n            McpCommand::InputKey { key, pressed } => {""",
)

replace_once(
    SYSTEMS,
    """            DeferredCommand::InputKey {\n                key,\n                pressed,\n                result_id,\n            } => {""",
    """            DeferredCommand::AtomicMutationBatch {\n                operations,\n                dry_run,\n                result_id,\n            } => {\n                let result = apply_atomic_mutation_batch(world, &operations, dry_run);\n                world.resource::<McpResultQueue>().push(McpResponse {\n                    request_id: result_id,\n                    result,\n                });\n            }\n            DeferredCommand::InputKey {\n                key,\n                pressed,\n                result_id,\n            } => {""",
)

replace_once(
    SYSTEMS,
    """        McpCommand::ComponentRemove { entity, component } => {\n            component_remove(world, entity, component)\n        }\n        McpCommand::RuntimePause => runtime_pause(registry),""",
    """        McpCommand::ComponentRemove { entity, component } => {\n            component_remove(world, entity, component)\n        }\n        McpCommand::AtomicMutationBatch { .. } => {\n            McpResult::error(\"INTERNAL\", \"Atomic mutation batches should be deferred\")\n        }\n        McpCommand::RuntimePause => runtime_pause(registry),""",
)

replace_once(
    SYSTEMS,
    """            \"mutate\": capability(true, reflected_types_available, can_mutate),\n            \"entity_duplicate\": capability(false, false, false),""",
    """            \"mutate\": capability(true, reflected_types_available, can_mutate),\n            \"atomic_mutation_batch\": capability(true, reflected_types_available, can_mutate),\n            \"entity_duplicate\": capability(false, false, false),""",
)

TRANSACTION_IMPL = r'''fn mutation_operation_name(operation: &MutationOperation) -> &'static str {
    match operation {
        MutationOperation::ComponentInsert { .. } => "component_insert",
        MutationOperation::ComponentUpdate { .. } => "component_update",
        MutationOperation::ComponentRemove { .. } => "component_remove",
        MutationOperation::ResourceUpdate { .. } => "resource_update",
    }
}

fn transaction_validation_error(
    index: usize,
    operation: &MutationOperation,
    error: McpResult,
) -> McpResult {
    match error {
        McpResult::Error { code, message } => McpResult::error(
            "TRANSACTION_VALIDATION_FAILED",
            format!(
                "Operation {index} ({}) failed validation [{code}]: {message}",
                mutation_operation_name(operation)
            ),
        ),
        McpResult::Success(_) => McpResult::error(
            "TRANSACTION_VALIDATION_FAILED",
            format!(
                "Operation {index} ({}) returned an invalid validation result",
                mutation_operation_name(operation)
            ),
        ),
    }
}

fn validate_component_write(
    world: &World,
    entity_handle: &EntityHandle,
    component: &str,
    value: &Value,
) -> Result<(), McpResult> {
    if resolve_entity(world, entity_handle).is_none() {
        return Err(McpResult::error(
            "ENTITY_NOT_FOUND",
            format!("Entity {entity_handle} not found"),
        ));
    }

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == component)
        .ok_or_else(|| {
            McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .is_none()
    {
        return Err(McpResult::error(
            "COMPONENT_NOT_REFLECTED",
            format!("Component '{component}' does not have ReflectComponent data"),
        ));
    }

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json = wrapped.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    reflect_deserializer
        .deserialize(&mut deserializer)
        .map_err(|error| {
            McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize '{component}': {error}"),
            )
        })?;

    Ok(())
}

fn validate_component_remove(
    world: &World,
    entity_handle: &EntityHandle,
    component: &str,
) -> Result<(), McpResult> {
    if resolve_entity(world, entity_handle).is_none() {
        return Err(McpResult::error(
            "ENTITY_NOT_FOUND",
            format!("Entity {entity_handle} not found"),
        ));
    }

    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == component)
        .ok_or_else(|| {
            McpResult::error(
                "COMPONENT_NOT_REGISTERED",
                format!("Component '{component}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::ecs::reflect::ReflectComponent>()
        .is_none()
    {
        return Err(McpResult::error(
            "COMPONENT_NOT_REFLECTED",
            format!("Component '{component}' does not have ReflectComponent data"),
        ));
    }

    Ok(())
}

fn validate_resource_write(world: &World, resource: &str, value: &Value) -> Result<(), McpResult> {
    let app_registry = world.resource::<AppTypeRegistry>().clone();
    let registry = app_registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path_table().short_path() == resource)
        .ok_or_else(|| {
            McpResult::error(
                "RESOURCE_NOT_REGISTERED",
                format!("Resource '{resource}' is not registered in the type registry"),
            )
        })?;

    if registration
        .data::<bevy::reflect::ReflectFromPtr>()
        .is_none()
    {
        return Err(McpResult::error(
            "RESOURCE_NOT_REFLECTED",
            format!("Resource '{resource}' does not have ReflectFromPtr data"),
        ));
    }

    if world.components().get_id(registration.type_id()).is_none() {
        return Err(McpResult::error(
            "RESOURCE_NOT_PRESENT",
            format!("Resource '{resource}' is not registered as a component"),
        ));
    }

    let type_id = registration.type_id();
    if !world
        .iter_resources()
        .any(|(info, _)| info.type_id() == Some(type_id))
    {
        return Err(McpResult::error(
            "RESOURCE_NOT_PRESENT",
            format!("Resource '{resource}' is not present in the world"),
        ));
    }

    let type_path = registration.type_info().type_path_table().path();
    let wrapped = json!({ type_path: value });
    let json = wrapped.to_string();
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let reflect_deserializer = bevy::reflect::serde::ReflectDeserializer::new(&registry);
    reflect_deserializer
        .deserialize(&mut deserializer)
        .map_err(|error| {
            McpResult::error(
                "DESERIALIZATION_ERROR",
                format!("Failed to deserialize resource '{resource}': {error}"),
            )
        })?;

    Ok(())
}

fn apply_atomic_mutation_batch(
    world: &mut World,
    operations: &[MutationOperation],
    dry_run: bool,
) -> McpResult {
    if operations.is_empty() {
        return McpResult::error(
            "EMPTY_TRANSACTION",
            "Atomic mutation batches require at least one operation",
        );
    }
    if operations.len() > 256 {
        return McpResult::error(
            "TRANSACTION_TOO_LARGE",
            "Atomic mutation batches are limited to 256 operations",
        );
    }

    // Validate the entire transaction against one exclusive World snapshot before
    // applying the first mutation. Supported operations do not despawn entities,
    // change the type registry, or advance the schedule, so successful validation
    // makes the commit phase deterministic within this exclusive system call.
    for (index, operation) in operations.iter().enumerate() {
        let validation = match operation {
            MutationOperation::ComponentInsert {
                entity,
                component,
                value,
            }
            | MutationOperation::ComponentUpdate {
                entity,
                component,
                value,
            } => validate_component_write(world, entity, component, value),
            MutationOperation::ComponentRemove { entity, component } => {
                validate_component_remove(world, entity, component)
            }
            MutationOperation::ResourceUpdate { resource, value } => {
                validate_resource_write(world, resource, value)
            }
        };

        if let Err(error) = validation {
            return transaction_validation_error(index, operation, error);
        }
    }

    if dry_run {
        return McpResult::success(json!({
            "mode": "atomic_dry_run",
            "validated": true,
            "committed": false,
            "operation_count": operations.len(),
            "operations": operations
                .iter()
                .enumerate()
                .map(|(index, operation)| json!({
                    "index": index,
                    "operation": mutation_operation_name(operation),
                }))
                .collect::<Vec<_>>(),
        }));
    }

    let mut applied = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        let result = match operation {
            MutationOperation::ComponentInsert {
                entity,
                component,
                value,
            }
            | MutationOperation::ComponentUpdate {
                entity,
                component,
                value,
            } => {
                let Some(entity) = resolve_entity(world, entity) else {
                    return McpResult::error(
                        "TRANSACTION_COMMIT_INVARIANT_FAILED",
                        format!("Validated entity disappeared before operation {index}"),
                    );
                };
                insert_component_by_reflect(world, entity, component, value)
            }
            MutationOperation::ComponentRemove { entity, component } => {
                let Some(entity) = resolve_entity(world, entity) else {
                    return McpResult::error(
                        "TRANSACTION_COMMIT_INVARIANT_FAILED",
                        format!("Validated entity disappeared before operation {index}"),
                    );
                };
                remove_component_by_reflect(world, entity, component)
            }
            MutationOperation::ResourceUpdate { resource, value } => {
                resource_update(world, resource, value)
            }
        };

        match result {
            McpResult::Success(_) => applied.push(json!({
                "index": index,
                "operation": mutation_operation_name(operation),
            })),
            McpResult::Error { code, message } => {
                return McpResult::error(
                    "TRANSACTION_COMMIT_INVARIANT_FAILED",
                    format!(
                        "Prevalidated operation {index} ({}) unexpectedly failed [{code}]: {message}",
                        mutation_operation_name(operation)
                    ),
                );
            }
        }
    }

    McpResult::success(json!({
        "mode": "atomic",
        "validated": true,
        "committed": true,
        "operation_count": operations.len(),
        "operations": applied,
    }))
}

'''

replace_once(
    SYSTEMS,
    "fn insert_component_by_reflect(\n",
    TRANSACTION_IMPL + "fn insert_component_by_reflect(\n",
)

# ---------------------------------------------------------------------------
# MCP batch API
# ---------------------------------------------------------------------------

replace_once(
    TOOLS,
    "use bevy_mcp_core::command::McpCommand;",
    "use bevy_mcp_core::command::{McpCommand, MutationOperation};",
)

replace_once(
    TOOLS,
    """    #[schemars(description = \"Unsupported. Atomic rollback is not available.\")]\n    pub atomic: Option<bool>,\n    #[schemars(\n        description = \"If true, return a preview without applying changes. Arguments are not validated.\"\n    )]\n    pub dry_run: Option<bool>,""",
    """    #[schemars(\n        description = \"If true, execute supported reflected mutations as one prevalidated all-or-nothing transaction.\"\n    )]\n    pub atomic: Option<bool>,\n    #[schemars(\n        description = \"For atomic batches, validate the full transaction without committing. For sequential batches, return an unvalidated preview.\"\n    )]\n    pub dry_run: Option<bool>,""",
)

OLD_BATCH_PREFIX = """    #[tool(\n        description = \"Execute a limited set of read operations sequentially. Preview mode does not validate arguments.\"\n    )]\n    async fn batch(&self, Parameters(params): Parameters<BatchParams>) -> String {\n        let mut results = Vec::new();\n        let stop_on_error = params.stop_on_error.unwrap_or(true);\n        let dry_run = params.dry_run.unwrap_or(false);\n        let atomic = params.atomic.unwrap_or(false);\n        let verify = params.verify.unwrap_or(false);\n\n        if atomic || verify {\n            return error(\n                \"UNSUPPORTED_BATCH_MODE\",\n                \"Atomic rollback and verification are not implemented; use sequential mode or preview mode\",\n            );\n        }\n\n        // In dry_run mode, just validate the operations without executing.\n        if dry_run {"""

NEW_BATCH_PREFIX = """    #[tool(\n        description = \"Execute limited reads sequentially, or set atomic=true for a prevalidated all-or-nothing reflected mutation batch. Atomic operations: component_insert, component_update, component_remove, resource_update.\"\n    )]\n    async fn batch(&self, Parameters(params): Parameters<BatchParams>) -> String {\n        let mut results = Vec::new();\n        let stop_on_error = params.stop_on_error.unwrap_or(true);\n        let dry_run = params.dry_run.unwrap_or(false);\n        let atomic = params.atomic.unwrap_or(false);\n        let verify = params.verify.unwrap_or(false);\n\n        if verify {\n            return error(\n                \"UNSUPPORTED_BATCH_MODE\",\n                \"Per-operation verification is not implemented\",\n            );\n        }\n\n        if atomic {\n            let mut operations = Vec::with_capacity(params.operations.len());\n            for (index, operation) in params.operations.iter().enumerate() {\n                match mutation_operation_from_batch(operation) {\n                    Ok(operation) => operations.push(operation),\n                    Err(message) => {\n                        return error(\n                            \"INVALID_ATOMIC_OPERATION\",\n                            format!(\"Operation {index}: {message}\"),\n                        );\n                    }\n                }\n            }\n            return self\n                .state\n                .call(McpCommand::AtomicMutationBatch { operations, dry_run })\n                .await;\n        }\n\n        // Sequential dry-run mode is intentionally a preview only; arguments are not validated.\n        if dry_run {"""

replace_once(TOOLS, OLD_BATCH_PREFIX, NEW_BATCH_PREFIX)

BATCH_HELPER = r'''fn mutation_operation_from_batch(operation: &BatchOperation) -> Result<MutationOperation, String> {
    let arguments = operation
        .arguments
        .clone()
        .ok_or_else(|| format!("{} requires arguments", operation.tool))?;

    match operation.tool.as_str() {
        "component_insert" => {
            let params: ComponentInsertParams = serde_json::from_value(arguments)
                .map_err(|error| format!("invalid component_insert arguments: {error}"))?;
            Ok(MutationOperation::ComponentInsert {
                entity: parse_entity_handle(&params.entity)?,
                component: params.component,
                value: params.value,
            })
        }
        "component_update" => {
            let params: ComponentUpdateParams = serde_json::from_value(arguments)
                .map_err(|error| format!("invalid component_update arguments: {error}"))?;
            Ok(MutationOperation::ComponentUpdate {
                entity: parse_entity_handle(&params.entity)?,
                component: params.component,
                value: params.value,
            })
        }
        "component_remove" => {
            let params: ComponentRemoveParams = serde_json::from_value(arguments)
                .map_err(|error| format!("invalid component_remove arguments: {error}"))?;
            Ok(MutationOperation::ComponentRemove {
                entity: parse_entity_handle(&params.entity)?,
                component: params.component,
            })
        }
        "resource_update" => {
            let params: ResourceUpdateParams = serde_json::from_value(arguments)
                .map_err(|error| format!("invalid resource_update arguments: {error}"))?;
            Ok(MutationOperation::ResourceUpdate {
                resource: params.resource,
                value: params.value,
            })
        }
        other => Err(format!(
            "tool '{other}' cannot participate in an atomic mutation batch; supported tools are component_insert, component_update, component_remove, resource_update"
        )),
    }
}

'''

replace_once(
    TOOLS,
    "fn parse_entity_handle(uri: &str) -> Result<EntityHandle, String> {\n",
    BATCH_HELPER + "fn parse_entity_handle(uri: &str) -> Result<EntityHandle, String> {\n",
)

replace_once(
    TOOLS,
    """    #[test]\n    fn entity_handles_must_be_complete_and_in_the_default_world() {""",
    """    #[test]\n    fn atomic_batch_parser_accepts_component_update() {\n        let operation = BatchOperation {\n            tool: \"component_update\".into(),\n            arguments: Some(serde_json::json!({\n                \"entity\": \"entity://default/main/42/3\",\n                \"component\": \"Health\",\n                \"value\": { \"current\": 75 }\n            })),\n        };\n        assert!(matches!(\n            mutation_operation_from_batch(&operation),\n            Ok(MutationOperation::ComponentUpdate { .. })\n        ));\n    }\n\n    #[test]\n    fn atomic_batch_parser_rejects_read_tools() {\n        let operation = BatchOperation {\n            tool: \"entity_get\".into(),\n            arguments: Some(serde_json::json!({\n                \"entity\": \"entity://default/main/42/3\"\n            })),\n        };\n        let error = mutation_operation_from_batch(&operation).unwrap_err();\n        assert!(error.contains(\"cannot participate\"));\n    }\n\n    #[test]\n    fn entity_handles_must_be_complete_and_in_the_default_world() {""",
)

# ---------------------------------------------------------------------------
# Integration tests
# ---------------------------------------------------------------------------

write(
    TESTS,
    r'''use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult, MutationOperation};
use bevy_mcp_core::entity_handle::EntityHandle;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};

#[derive(Component, Reflect, Debug, PartialEq)]
#[reflect(Component)]
struct Health {
    current: i32,
}

#[derive(Component, Reflect, Debug, PartialEq)]
#[reflect(Component)]
struct Tag {
    value: i32,
}

#[derive(Resource, Reflect, Debug, PartialEq)]
#[reflect(Resource)]
struct GameConfig {
    difficulty: i32,
}

fn handle(entity: Entity) -> EntityHandle {
    EntityHandle::from_uri(&format!(
        "entity://default/main/{}/{}",
        entity.index().index(),
        entity.generation()
    ))
    .unwrap()
}

fn setup() -> (App, McpIngressQueue, McpResultQueue, Entity) {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(McpPermissions::write()),
        )
        .register_type::<Health>()
        .register_type::<Tag>()
        .register_type::<GameConfig>();
    app.insert_resource(GameConfig { difficulty: 1 });
    let entity = app.world_mut().spawn(Health { current: 100 }).id();
    (app, ingress, results, entity)
}

fn result_for(results: &McpResultQueue, request_id: u64) -> McpResult {
    results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response")
        .result
}

#[test]
fn atomic_batch_commits_all_prevalidated_mutations() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        1,
        McpCommand::AtomicMutationBatch {
            operations: vec![
                MutationOperation::ComponentUpdate {
                    entity: handle(entity),
                    component: "Health".into(),
                    value: serde_json::json!({ "current": 75 }),
                },
                MutationOperation::ComponentInsert {
                    entity: handle(entity),
                    component: "Tag".into(),
                    value: serde_json::json!({ "value": 9 }),
                },
                MutationOperation::ResourceUpdate {
                    resource: "GameConfig".into(),
                    value: serde_json::json!({ "difficulty": 3 }),
                },
            ],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 1) else {
        panic!("expected transaction success");
    };
    assert_eq!(value["committed"], true);
    assert_eq!(value["operation_count"], 3);
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 75);
    assert_eq!(app.world().get::<Tag>(entity).unwrap().value, 9);
    assert_eq!(app.world().resource::<GameConfig>().difficulty, 3);
}

#[test]
fn atomic_batch_validation_failure_leaves_earlier_operations_unapplied() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        2,
        McpCommand::AtomicMutationBatch {
            operations: vec![
                MutationOperation::ComponentUpdate {
                    entity: handle(entity),
                    component: "Health".into(),
                    value: serde_json::json!({ "current": 10 }),
                },
                MutationOperation::ResourceUpdate {
                    resource: "GameConfig".into(),
                    value: serde_json::json!({ "difficulty": "impossible" }),
                },
            ],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Error { code, message } = result_for(&results, 2) else {
        panic!("expected transaction validation failure");
    };
    assert_eq!(code, "TRANSACTION_VALIDATION_FAILED");
    assert!(message.contains("Operation 1"));
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 100);
    assert_eq!(app.world().resource::<GameConfig>().difficulty, 1);
}

#[test]
fn atomic_batch_dry_run_validates_without_committing() {
    let (mut app, ingress, results, entity) = setup();
    ingress.push(
        3,
        McpCommand::AtomicMutationBatch {
            operations: vec![MutationOperation::ComponentUpdate {
                entity: handle(entity),
                component: "Health".into(),
                value: serde_json::json!({ "current": 25 }),
            }],
            dry_run: true,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 3) else {
        panic!("expected dry-run validation success");
    };
    assert_eq!(value["validated"], true);
    assert_eq!(value["committed"], false);
    assert_eq!(value["mode"], "atomic_dry_run");
    assert_eq!(app.world().get::<Health>(entity).unwrap().current, 100);
}

#[test]
fn atomic_batch_can_remove_reflected_components() {
    let (mut app, ingress, results, entity) = setup();
    app.world_mut().entity_mut(entity).insert(Tag { value: 4 });
    ingress.push(
        4,
        McpCommand::AtomicMutationBatch {
            operations: vec![MutationOperation::ComponentRemove {
                entity: handle(entity),
                component: "Tag".into(),
            }],
            dry_run: false,
        },
    );

    app.update();
    let McpResult::Success(value) = result_for(&results, 4) else {
        panic!("expected remove transaction success");
    };
    assert_eq!(value["committed"], true);
    assert!(app.world().get::<Tag>(entity).is_none());
}
''',
)

# ---------------------------------------------------------------------------
# Front-page documentation
# ---------------------------------------------------------------------------

replace_once(
    README,
    """Spawn and despawn entities, insert/update/remove reflected components, update resources, reparent entities, transition registered Bevy states, invoke game-defined semantic actions, and create procedural meshes/templates. ECS mutations are deferred to safe schedule boundaries.\n\n**Representative tools:** `entity_spawn` · `entity_despawn` · `component_insert` · `component_update` · `component_remove` · `resource_update` · `entity_reparent` · `state_transition` · `semantic_action_invoke` · `mesh_spawn` · `template_save` · `template_load`""",
    """Spawn and despawn entities, insert/update/remove reflected components, update resources, reparent entities, transition registered Bevy states, invoke game-defined semantic actions, and create procedural meshes/templates. ECS mutations are deferred to safe schedule boundaries.\n\nFor multi-write edits, `batch` with `atomic: true` provides a prevalidated all-or-nothing transaction for reflected `component_insert`, `component_update`, `component_remove`, and `resource_update` operations. The entire batch is validated against one exclusive world snapshot before the first write; `dry_run: true` performs the same validation without committing.\n\n**Representative tools:** `entity_spawn` · `entity_despawn` · `component_insert` · `component_update` · `component_remove` · `resource_update` · `batch` · `entity_reparent` · `state_transition` · `semantic_action_invoke` · `mesh_spawn` · `template_save` · `template_load`""",
)

replace_once(
    README,
    """- **Atomic batch rollback is not implemented.** `batch` supports a limited sequential read surface and preview mode; `atomic` and `verify` modes are rejected.""",
    """- **Atomic batch scope is intentionally narrow.** Atomic mode currently accepts reflected `component_insert`, `component_update`, `component_remove`, and `resource_update`. Entity lifecycle, hierarchy changes, runtime/input operations, semantic actions, and other arbitrary side effects are not transaction members. `verify` mode remains unavailable.""",
)

print("atomic mutation batch transformation applied")
