from pathlib import Path

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

# Give callers that do not own a World a safe, explicit instance-scoped formatter.
path = "crates/bevy-mcp-host/src/entity_handle.rs"
text = read(path)
old = '''/// Build an entity handle URI scoped to the current game-process instance.
pub fn entity_to_uri(world: &World, entity: Entity) -> String {
    format!(
        "entity://{}/main/{}/{}",
        current_instance(world),
        entity.index().index(),
        entity.generation()
    )
}
'''
new = '''pub fn entity_to_uri_for_instance(instance_id: &str, entity: Entity) -> String {
    format!(
        "entity://{}/main/{}/{}",
        instance_id,
        entity.index().index(),
        entity.generation()
    )
}

/// Build an entity handle URI scoped to the current game-process instance.
pub fn entity_to_uri(world: &World, entity: Entity) -> String {
    entity_to_uri_for_instance(current_instance(world), entity)
}
'''
text = replace_once(text, old, new, "instance-scoped entity formatter")
text = text.replace(
    "entity.generation() as u64,",
    "entity.generation().to_bits() as u64,",
)
write(path, text)

# Change history can outlive the specific World borrow that produced it, so every recorded
# frame carries the process instance that owns its entity IDs.
path = "crates/bevy-mcp-host/src/change_tracking.rs"
text = read(path)
text = replace_once(
    text,
    "use crate::entity_handle::entity_to_uri;\nuse crate::registry::McpRegistry;",
    "use crate::entity_handle::entity_to_uri_for_instance;\nuse crate::instance::McpInstanceId;\nuse crate::registry::McpRegistry;",
    "change tracking imports",
)
text = replace_once(
    text,
    '''impl ComponentChangeRecord {
    pub fn as_json(&self) -> Value {
        json!({
            "entity": entity_to_uri(world, self.entity),
            "component": self.component,
            "kind": self.kind.as_str(),
        })
    }
}
''',
    '''impl ComponentChangeRecord {
    pub fn as_json(&self, instance_id: &str) -> Value {
        json!({
            "entity": entity_to_uri_for_instance(instance_id, self.entity),
            "component": self.component,
            "kind": self.kind.as_str(),
        })
    }
}
''',
    "component change serialization",
)
text = replace_once(
    text,
    '''pub struct FrameChanges {
    pub frame: u64,
    pub spawned: Vec<Entity>,''',
    '''pub struct FrameChanges {
    pub frame: u64,
    pub instance_id: String,
    pub spawned: Vec<Entity>,''',
    "frame instance id",
)
text = replace_once(
    text,
    '''    dynamic_resources: HashSet<String>,
}''',
    '''    dynamic_resources: HashSet<String>,
    instance_id: String,
}''',
    "tracker instance field",
)
text = replace_once(
    text,
    '''            dynamic_resources: HashSet::new(),
        }''',
    '''            dynamic_resources: HashSet::new(),
            instance_id: "default".to_string(),
        }''',
    "tracker instance default",
)
text = text.replace(
    'entity_to_uri(world, *current)',
    'entity_to_uri_for_instance(&entry.instance_id, *current)',
)
text = text.replace(
    'entity_to_uri(world, change.entity)',
    'entity_to_uri_for_instance(&entry.instance_id, change.entity)',
)
text = replace_once(
    text,
    'json!({ "since_frame": frame, "entity": entity.map(entity_to_uri), "spawned": spawned, "despawned": despawned, "components": components })',
    'json!({ "since_frame": frame, "entity": entity.map(|entity| entity_to_uri_for_instance(&self.instance_id, entity)), "spawned": spawned, "despawned": despawned, "components": components })',
    "entity change filter serialization",
)
text = replace_once(
    text,
    '''    let mut current_entities = HashMap::new();
    let mut changes = FrameChanges {
        frame,
        ..Default::default()
    };''',
    '''    let instance_id = world
        .get_resource::<McpInstanceId>()
        .map(McpInstanceId::as_str)
        .unwrap_or("default")
        .to_string();
    tracker.instance_id = instance_id.clone();
    let mut current_entities = HashMap::new();
    let mut changes = FrameChanges {
        frame,
        instance_id,
        ..Default::default()
    };''',
    "capture frame instance",
)
text = replace_once(
    text,
    '''fn frame_changes_json(entry: &FrameChanges) -> Value {
    json!({
        "frame": entry.frame,
        "spawned": entry.spawned.iter().copied().map(entity_to_uri).collect::<Vec<_>>(),
        "despawned": entry.despawned.iter().copied().map(entity_to_uri).collect::<Vec<_>>(),
        "components": entry.components.iter().map(ComponentChangeRecord::as_json).collect::<Vec<_>>(),
        "resources": entry.resources.iter().map(ResourceChangeRecord::as_json).collect::<Vec<_>>(),
    })
}''',
    '''fn frame_changes_json(entry: &FrameChanges) -> Value {
    json!({
        "frame": entry.frame,
        "instance_id": entry.instance_id,
        "spawned": entry.spawned.iter().copied().map(|entity| entity_to_uri_for_instance(&entry.instance_id, entity)).collect::<Vec<_>>(),
        "despawned": entry.despawned.iter().copied().map(|entity| entity_to_uri_for_instance(&entry.instance_id, entity)).collect::<Vec<_>>(),
        "components": entry.components.iter().map(|change| change.as_json(&entry.instance_id)).collect::<Vec<_>>(),
        "resources": entry.resources.iter().map(ResourceChangeRecord::as_json).collect::<Vec<_>>(),
    })
}''',
    "frame change serialization",
)
write(path, text)

# Keep the supervisor implementation warning-clean.
path = "crates/bevy-mcp-supervisor/src/backend.rs"
text = read(path)
text = text.replace("use std::future::Future;\n", "")
text = text.replace("use std::pin::Pin;\n", "")
write(path, text)

print("Stage 1 entity URI propagation fix applied")
