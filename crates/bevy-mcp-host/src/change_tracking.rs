use std::collections::{HashMap, HashSet, VecDeque};

use bevy::ecs::component::ComponentId;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use serde_json::{Value, json};

use crate::entity_handle::entity_to_uri_for_instance;
use crate::instance::McpInstanceId;
use crate::registry::McpRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Changed,
    Removed,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Changed => "changed",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackingMode {
    Full,
    Scoped,
}

impl TrackingMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Scoped => "scoped",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "scoped" | "subscribed" => Some(Self::Scoped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComponentChangeRecord {
    pub entity: Entity,
    pub component: String,
    pub kind: ChangeKind,
}

impl ComponentChangeRecord {
    pub fn as_json(&self, instance_id: &str) -> Value {
        json!({
            "entity": entity_to_uri_for_instance(instance_id, self.entity),
            "component": self.component,
            "kind": self.kind.as_str(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResourceChangeRecord {
    pub resource: String,
    pub kind: ChangeKind,
}

impl ResourceChangeRecord {
    pub fn as_json(&self) -> Value {
        json!({ "resource": self.resource, "kind": self.kind.as_str() })
    }
}

#[derive(Debug, Clone, Default)]
pub struct FrameChanges {
    pub frame: u64,
    pub instance_id: String,
    pub spawned: Vec<Entity>,
    pub despawned: Vec<Entity>,
    pub components: Vec<ComponentChangeRecord>,
    pub resources: Vec<ResourceChangeRecord>,
}

impl FrameChanges {
    pub fn is_empty(&self) -> bool {
        self.spawned.is_empty()
            && self.despawned.is_empty()
            && self.components.is_empty()
            && self.resources.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct TickSnapshot {
    added: u32,
    changed: u32,
}

#[derive(Debug, Clone, Default)]
struct EntitySnapshot {
    components: HashMap<ComponentId, TickSnapshot>,
}

#[derive(Resource)]
pub struct WorldChangeTracker {
    history: VecDeque<FrameChanges>,
    previous_entities: HashMap<Entity, EntitySnapshot>,
    previous_resources: HashMap<ComponentId, TickSnapshot>,
    resource_names: HashMap<ComponentId, String>,
    latest_changed_components: HashSet<(Entity, String)>,
    max_history: usize,
    initialized: bool,
    mode: TrackingMode,
    components: HashSet<String>,
    resources: HashSet<String>,
    exclude_components: HashSet<String>,
    exclude_resources: HashSet<String>,
    dynamic_components: HashSet<String>,
    dynamic_resources: HashSet<String>,
    instance_id: String,
}

impl Default for WorldChangeTracker {
    fn default() -> Self {
        Self {
            history: VecDeque::new(),
            previous_entities: HashMap::new(),
            previous_resources: HashMap::new(),
            resource_names: HashMap::new(),
            latest_changed_components: HashSet::new(),
            max_history: 600,
            initialized: false,
            mode: TrackingMode::Full,
            components: HashSet::new(),
            resources: HashSet::new(),
            exclude_components: HashSet::new(),
            exclude_resources: HashSet::new(),
            dynamic_components: HashSet::new(),
            dynamic_resources: HashSet::new(),
            instance_id: "default".to_string(),
        }
    }
}

impl WorldChangeTracker {
    pub fn configure(
        &mut self,
        mode: Option<&str>,
        history_frames: Option<usize>,
        components: Option<Vec<String>>,
        resources: Option<Vec<String>>,
        exclude_components: Option<Vec<String>>,
        exclude_resources: Option<Vec<String>>,
    ) -> Result<Value, String> {
        if let Some(mode) = mode {
            let parsed = TrackingMode::parse(mode)
                .ok_or_else(|| format!("Unknown tracking mode '{mode}'; use full or scoped"))?;
            if parsed != self.mode {
                self.mode = parsed;
                self.reset_snapshots();
                self.previous_resources.clear();
            }
        }
        if let Some(history) = history_frames {
            self.max_history = history.clamp(1, 100_000);
            while self.history.len() > self.max_history {
                self.history.pop_front();
            }
        }
        if let Some(values) = components {
            self.components = values.into_iter().collect();
            self.reset_snapshots();
        }
        if let Some(values) = resources {
            self.resources = values.into_iter().collect();
            self.previous_resources.clear();
        }
        if let Some(values) = exclude_components {
            self.exclude_components = values.into_iter().collect();
            self.reset_snapshots();
        }
        if let Some(values) = exclude_resources {
            self.exclude_resources = values.into_iter().collect();
            self.previous_resources.clear();
        }
        Ok(self.status_json())
    }

    pub fn add_dynamic_interests<I, J>(&mut self, components: I, resources: J)
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
    {
        let mut next_components = self.dynamic_components.clone();
        let mut next_resources = self.dynamic_resources.clone();
        next_components.extend(components);
        next_resources.extend(resources);
        self.set_dynamic_interests(next_components, next_resources);
    }

    pub fn set_dynamic_interests<I, J>(&mut self, components: I, resources: J)
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
    {
        let next_components: HashSet<String> = components.into_iter().collect();
        let next_resources: HashSet<String> = resources.into_iter().collect();
        if self.dynamic_components == next_components && self.dynamic_resources == next_resources {
            return;
        }

        self.dynamic_components = next_components;
        self.dynamic_resources = next_resources;
        if self.mode == TrackingMode::Scoped {
            self.reset_snapshots();
            self.previous_resources.clear();
        }
    }

    pub fn clear_dynamic_interests(&mut self) {
        self.set_dynamic_interests(Vec::<String>::new(), Vec::<String>::new());
    }

    pub fn status_json(&self) -> Value {
        let mut components: Vec<_> = self.components.iter().cloned().collect();
        let mut resources: Vec<_> = self.resources.iter().cloned().collect();
        let mut dynamic_components: Vec<_> = self.dynamic_components.iter().cloned().collect();
        let mut dynamic_resources: Vec<_> = self.dynamic_resources.iter().cloned().collect();
        components.sort();
        resources.sort();
        dynamic_components.sort();
        dynamic_resources.sort();
        json!({
            "mode": self.mode.as_str(),
            "history_frames": self.max_history,
            "history_len": self.history.len(),
            "components": components,
            "resources": resources,
            "dynamic_components": dynamic_components,
            "dynamic_resources": dynamic_resources,
            "tracked_entities": self.previous_entities.len(),
            "tracked_resources": self.previous_resources.len(),
        })
    }

    fn reset_snapshots(&mut self) {
        self.previous_entities.clear();
        self.initialized = false;
    }

    fn should_track_component(&self, name: &str) -> bool {
        if self
            .exclude_components
            .iter()
            .any(|v| component_name_matches(name, v))
        {
            return false;
        }
        self.mode == TrackingMode::Full
            || self
                .components
                .iter()
                .any(|v| component_name_matches(name, v))
            || self
                .dynamic_components
                .iter()
                .any(|v| component_name_matches(name, v))
    }

    fn should_track_resource(&self, name: &str) -> bool {
        if self
            .exclude_resources
            .iter()
            .any(|v| component_name_matches(name, v))
        {
            return false;
        }
        self.mode == TrackingMode::Full
            || self
                .resources
                .iter()
                .any(|v| component_name_matches(name, v))
            || self
                .dynamic_resources
                .iter()
                .any(|v| component_name_matches(name, v))
    }

    pub fn changes_since(&self, frame: u64) -> Value {
        let frames: Vec<Value> = self
            .history
            .iter()
            .filter(|e| e.frame > frame)
            .map(frame_changes_json)
            .collect();
        json!({
            "since_frame": frame,
            "oldest_available_frame": self.history.front().map(|e| e.frame),
            "latest_available_frame": self.history.back().map(|e| e.frame),
            "tracking": self.status_json(),
            "frames": frames,
        })
    }

    pub fn entity_changes_since(&self, frame: u64, entity: Option<Entity>) -> Value {
        let mut spawned = Vec::new();
        let mut despawned = Vec::new();
        let mut components = Vec::new();
        for entry in self.history.iter().filter(|entry| entry.frame > frame) {
            for current in &entry.spawned {
                if entity.is_none_or(|wanted| wanted == *current) {
                    spawned
                        .push(json!({ "frame": entry.frame, "entity": entity_to_uri_for_instance(&entry.instance_id, *current) }));
                }
            }
            for current in &entry.despawned {
                if entity.is_none_or(|wanted| wanted == *current) {
                    despawned
                        .push(json!({ "frame": entry.frame, "entity": entity_to_uri_for_instance(&entry.instance_id, *current) }));
                }
            }
            for change in &entry.components {
                if entity.is_none_or(|wanted| wanted == change.entity) {
                    components.push(json!({
                        "frame": entry.frame,
                        "entity": entity_to_uri_for_instance(&entry.instance_id, change.entity),
                        "component": change.component,
                        "kind": change.kind.as_str(),
                    }));
                }
            }
        }
        json!({ "since_frame": frame, "entity": entity.map(|entity| entity_to_uri_for_instance(&self.instance_id, entity)), "spawned": spawned, "despawned": despawned, "components": components })
    }

    pub fn component_changes_since(&self, frame: u64, component: Option<&str>) -> Value {
        let changes: Vec<Value> = self.history.iter().filter(|e| e.frame > frame).flat_map(|entry| {
            entry.components.iter().filter_map(move |change| {
                if component.is_none_or(|wanted| component_name_matches(&change.component, wanted)) {
                    Some(json!({ "frame": entry.frame, "entity": entity_to_uri_for_instance(&entry.instance_id, change.entity), "component": change.component, "kind": change.kind.as_str() }))
                } else { None }
            })
        }).collect();
        json!({ "since_frame": frame, "component": component, "changes": changes })
    }

    pub fn resource_changes_since(&self, frame: u64, resource: Option<&str>) -> Value {
        let changes: Vec<Value> = self.history.iter().filter(|e| e.frame > frame).flat_map(|entry| {
            entry.resources.iter().filter_map(move |change| {
                if resource.is_none_or(|wanted| component_name_matches(&change.resource, wanted)) {
                    Some(json!({ "frame": entry.frame, "resource": change.resource, "kind": change.kind.as_str() }))
                } else { None }
            })
        }).collect();
        json!({ "since_frame": frame, "resource": resource, "changes": changes })
    }

    pub fn component_changed_last_frame(&self, entity: Entity, component: &str) -> bool {
        self.latest_changed_components
            .iter()
            .any(|(e, c)| *e == entity && component_name_matches(c, component))
    }

    pub fn latest_frame(&self) -> Option<u64> {
        self.history.back().map(|entry| entry.frame)
    }
}

pub fn track_world_changes(world: &mut World) {
    let mut tracker = world
        .remove_resource::<WorldChangeTracker>()
        .unwrap_or_default();
    let frame = world
        .get_resource::<McpRegistry>()
        .map(|r| r.frame)
        .unwrap_or_default();
    let instance_id = world
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
    };

    for entity_ref in world.iter_entities() {
        if entity_ref.contains::<IsResource>() {
            continue;
        }
        let entity = entity_ref.id();
        let mut snapshot = EntitySnapshot::default();
        for component_id in entity_ref.archetype().components() {
            let name = component_name(world, *component_id);
            if !tracker.should_track_component(&name) {
                continue;
            }
            let Some(ticks) = entity_ref.get_change_ticks_by_id(*component_id) else {
                continue;
            };
            snapshot.components.insert(
                *component_id,
                TickSnapshot {
                    added: ticks.added.get(),
                    changed: ticks.changed.get(),
                },
            );
        }
        if tracker.initialized {
            match tracker.previous_entities.get(&entity) {
                None => changes.spawned.push(entity),
                Some(previous) => {
                    for (component_id, ticks) in &snapshot.components {
                        let name = component_name(world, *component_id);
                        match previous.components.get(component_id) {
                            None => changes.components.push(ComponentChangeRecord {
                                entity,
                                component: name,
                                kind: ChangeKind::Added,
                            }),
                            Some(old) if old.changed != ticks.changed => {
                                changes.components.push(ComponentChangeRecord {
                                    entity,
                                    component: name,
                                    kind: ChangeKind::Changed,
                                })
                            }
                            _ => {}
                        }
                    }
                    for component_id in previous.components.keys() {
                        if !snapshot.components.contains_key(component_id) {
                            changes.components.push(ComponentChangeRecord {
                                entity,
                                component: component_name(world, *component_id),
                                kind: ChangeKind::Removed,
                            });
                        }
                    }
                }
            }
        }
        current_entities.insert(entity, snapshot);
    }

    if tracker.initialized {
        for entity in tracker.previous_entities.keys() {
            if !current_entities.contains_key(entity) {
                changes.despawned.push(*entity);
            }
        }
    }

    let mut current_resources = HashMap::new();
    let mut current_resource_names = HashMap::new();
    for (info, _) in world.iter_resources() {
        let name = info.name().to_string();
        if is_internal_mcp_resource(&name) || !tracker.should_track_resource(&name) {
            continue;
        }
        let component_id = info.id();
        let Some(ticks) = world.get_resource_change_ticks_by_id(component_id) else {
            continue;
        };
        let snapshot = TickSnapshot {
            added: ticks.added.get(),
            changed: ticks.changed.get(),
        };
        current_resource_names.insert(component_id, name.clone());
        if tracker.initialized {
            match tracker.previous_resources.get(&component_id) {
                None => changes.resources.push(ResourceChangeRecord {
                    resource: name,
                    kind: ChangeKind::Added,
                }),
                Some(old) if old.changed != snapshot.changed => {
                    changes.resources.push(ResourceChangeRecord {
                        resource: name,
                        kind: ChangeKind::Changed,
                    })
                }
                _ => {}
            }
        }
        current_resources.insert(component_id, snapshot);
    }
    if tracker.initialized {
        for component_id in tracker.previous_resources.keys() {
            if !current_resources.contains_key(component_id) {
                let name = tracker
                    .resource_names
                    .get(component_id)
                    .cloned()
                    .unwrap_or_else(|| format!("resource#{}", component_id.index()));
                changes.resources.push(ResourceChangeRecord {
                    resource: name,
                    kind: ChangeKind::Removed,
                });
            }
        }
    }

    tracker.latest_changed_components = changes
        .components
        .iter()
        .filter(|c| matches!(c.kind, ChangeKind::Added | ChangeKind::Changed))
        .map(|c| (c.entity, c.component.clone()))
        .collect();
    tracker.previous_entities = current_entities;
    tracker.previous_resources = current_resources;
    tracker.resource_names = current_resource_names;
    if tracker.initialized {
        tracker.history.push_back(changes);
        while tracker.history.len() > tracker.max_history {
            tracker.history.pop_front();
        }
    } else {
        tracker.initialized = true;
    }
    world.insert_resource(tracker);
}

fn frame_changes_json(entry: &FrameChanges) -> Value {
    json!({
        "frame": entry.frame,
        "instance_id": entry.instance_id,
        "spawned": entry.spawned.iter().copied().map(|entity| entity_to_uri_for_instance(&entry.instance_id, entity)).collect::<Vec<_>>(),
        "despawned": entry.despawned.iter().copied().map(|entity| entity_to_uri_for_instance(&entry.instance_id, entity)).collect::<Vec<_>>(),
        "components": entry.components.iter().map(|change| change.as_json(&entry.instance_id)).collect::<Vec<_>>(),
        "resources": entry.resources.iter().map(ResourceChangeRecord::as_json).collect::<Vec<_>>(),
    })
}

fn component_name(world: &World, component_id: ComponentId) -> String {
    world
        .components()
        .get_info(component_id)
        .map(|i| i.name().to_string())
        .unwrap_or_else(|| format!("component#{}", component_id.index()))
}

pub fn component_name_matches(actual: &str, requested: &str) -> bool {
    actual == requested
        || actual.rsplit("::").next() == Some(requested)
        || requested.rsplit("::").next() == actual.rsplit("::").next()
}

fn is_internal_mcp_resource(name: &str) -> bool {
    name.contains("bevy_mcp_host::") || name.contains("bevy_mcp_core::")
}
