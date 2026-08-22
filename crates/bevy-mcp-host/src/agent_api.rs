use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use bevy::state::state::FreelyMutableState;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::checkpoint::McpCheckpointRegistry;

pub type ActionResult = Result<Value, String>;
type ActionHandler = Arc<dyn Fn(&mut World, Value) -> ActionResult + Send + Sync + 'static>;
type StateGetter = Arc<dyn Fn(&World) -> ActionResult + Send + Sync + 'static>;
type StateSetter = Arc<dyn Fn(&mut World, Value) -> ActionResult + Send + Sync + 'static>;

#[derive(Clone)]
pub struct RegisteredAction {
    pub description: String,
    handler: ActionHandler,
}

#[derive(Resource, Default)]
pub struct McpActionRegistry {
    actions: HashMap<String, RegisteredAction>,
}

impl McpActionRegistry {
    pub fn names(&self) -> impl Iterator<Item = (&String, &RegisteredAction)> {
        self.actions.iter()
    }

    pub fn invoke(&self, name: &str, world: &mut World, args: Value) -> ActionResult {
        let handler = self
            .actions
            .get(name)
            .ok_or_else(|| format!("Unknown semantic action '{name}'"))?
            .handler
            .clone();
        handler(world, args)
    }
}

#[derive(Clone)]
struct RegisteredState {
    description: String,
    get: StateGetter,
    set: StateSetter,
}

#[derive(Resource, Default)]
pub struct McpStateRegistry {
    states: HashMap<String, RegisteredState>,
}

impl McpStateRegistry {
    pub fn list(&self, world: &World) -> Vec<Value> {
        self.states
            .iter()
            .map(|(name, state)| {
                let value = (state.get)(world).unwrap_or_else(|error| json!({ "error": error }));
                json!({
                    "name": name,
                    "description": state.description,
                    "value": value,
                })
            })
            .collect()
    }

    pub fn get(&self, name: &str, world: &World) -> ActionResult {
        let state = self
            .states
            .get(name)
            .ok_or_else(|| format!("Unknown MCP state '{name}'"))?;
        (state.get)(world)
    }

    pub fn set(&self, name: &str, world: &mut World, value: Value) -> ActionResult {
        let setter = self
            .states
            .get(name)
            .ok_or_else(|| format!("Unknown MCP state '{name}'"))?
            .set
            .clone();
        setter(world, value)
    }
}

#[derive(Resource, Default)]
pub struct McpCaptureTargets {
    ui_target: Option<Handle<Image>>,
}

impl McpCaptureTargets {
    pub fn ui_target(&self) -> Option<Handle<Image>> {
        self.ui_target.clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SystemTimingSummary {
    pub samples: u64,
    pub total_ns: u128,
    pub max_ns: u64,
    recent_ns: VecDeque<u64>,
}

impl SystemTimingSummary {
    fn record(&mut self, duration: Duration) {
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.samples += 1;
        self.total_ns += nanos as u128;
        self.max_ns = self.max_ns.max(nanos);
        self.recent_ns.push_back(nanos);
        while self.recent_ns.len() > 120 {
            self.recent_ns.pop_front();
        }
    }

    pub fn as_json(&self) -> Value {
        let average_ns = if self.samples == 0 {
            0
        } else {
            (self.total_ns / self.samples as u128).min(u64::MAX as u128) as u64
        };
        let recent_average_ns = if self.recent_ns.is_empty() {
            0
        } else {
            self.recent_ns.iter().copied().sum::<u64>() / self.recent_ns.len() as u64
        };
        json!({
            "samples": self.samples,
            "average_ns": average_ns,
            "recent_average_ns": recent_average_ns,
            "max_ns": self.max_ns,
        })
    }
}

#[derive(Resource, Default)]
pub struct McpSystemTimings {
    timings: HashMap<String, SystemTimingSummary>,
}

impl McpSystemTimings {
    pub fn record(&mut self, system: impl Into<String>, duration: Duration) {
        self.timings
            .entry(system.into())
            .or_default()
            .record(duration);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &SystemTimingSummary)> {
        self.timings.iter()
    }
}

/// Exact ECS access declared by a game for one named system.
///
/// Bevy 0.19 stores initialized schedule access internally but does not expose that
/// access set through a public getter. Register important systems here when exact
/// writer/read attribution is desired; the MCP falls back to Bevy's public conflict
/// graph for unregistered systems.
#[derive(Debug, Clone, Default)]
pub struct McpSystemAccessSpec {
    pub system: String,
    pub schedule: Option<String>,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub resource_reads: Vec<String>,
    pub resource_writes: Vec<String>,
    pub read_all: bool,
    pub write_all: bool,
}

impl McpSystemAccessSpec {
    pub fn new(system: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            ..Default::default()
        }
    }

    pub fn schedule(mut self, schedule: impl Into<String>) -> Self {
        self.schedule = Some(schedule.into());
        self
    }

    pub fn read<T: Component>(mut self) -> Self {
        self.reads.push(std::any::type_name::<T>().to_string());
        self
    }

    pub fn write<T: Component>(mut self) -> Self {
        self.writes.push(std::any::type_name::<T>().to_string());
        self
    }

    pub fn read_resource<T: Resource>(mut self) -> Self {
        self.resource_reads
            .push(std::any::type_name::<T>().to_string());
        self
    }

    pub fn write_resource<T: Resource>(mut self) -> Self {
        self.resource_writes
            .push(std::any::type_name::<T>().to_string());
        self
    }

    pub fn read_all(mut self) -> Self {
        self.read_all = true;
        self
    }

    pub fn write_all(mut self) -> Self {
        self.write_all = true;
        self
    }

    pub fn as_json(&self) -> Value {
        json!({
            "system": self.system,
            "schedule": self.schedule,
            "reads": self.reads,
            "writes": self.writes,
            "resource_reads": self.resource_reads,
            "resource_writes": self.resource_writes,
            "read_all": self.read_all,
            "write_all": self.write_all,
        })
    }
}

#[derive(Resource, Default)]
pub struct McpSystemAccessRegistry {
    entries: Vec<McpSystemAccessSpec>,
}

impl McpSystemAccessRegistry {
    pub fn register(&mut self, spec: McpSystemAccessSpec) {
        self.entries.retain(|existing| {
            existing.system != spec.system || existing.schedule != spec.schedule
        });
        self.entries.push(spec);
    }

    pub fn iter(&self) -> impl Iterator<Item = &McpSystemAccessSpec> {
        self.entries.iter()
    }
}

pub trait McpAgentAppExt {
    fn register_mcp_action<F>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> &mut Self
    where
        F: Fn(&mut World, Value) -> ActionResult + Send + Sync + 'static;

    fn register_mcp_state<T>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> &mut Self
    where
        T: FreelyMutableState + Serialize + DeserializeOwned + Send + Sync + 'static;

    fn register_mcp_system_access(&mut self, spec: McpSystemAccessSpec) -> &mut Self;

    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self;

    fn register_mcp_checkpoint_resource<T>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> &mut Self
    where
        T: Resource + Serialize + DeserializeOwned + Send + Sync + 'static;
}

impl McpAgentAppExt for App {
    fn register_mcp_action<F>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> &mut Self
    where
        F: Fn(&mut World, Value) -> ActionResult + Send + Sync + 'static,
    {
        self.world_mut().init_resource::<McpActionRegistry>();
        self.world_mut()
            .resource_mut::<McpActionRegistry>()
            .actions
            .insert(
                name.into(),
                RegisteredAction {
                    description: description.into(),
                    handler: Arc::new(handler),
                },
            );
        self
    }

    fn register_mcp_state<T>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> &mut Self
    where
        T: FreelyMutableState + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        self.world_mut().init_resource::<McpStateRegistry>();
        let getter: StateGetter = Arc::new(|world: &World| {
            let state = world.get_resource::<State<T>>().ok_or_else(|| {
                format!(
                    "State<{}> resource is not initialized",
                    std::any::type_name::<T>()
                )
            })?;
            serde_json::to_value(state.get()).map_err(|error| error.to_string())
        });
        let setter: StateSetter = Arc::new(|world: &mut World, value: Value| {
            let next: T = serde_json::from_value(value).map_err(|error| error.to_string())?;
            let mut state = world.get_resource_mut::<NextState<T>>().ok_or_else(|| {
                format!(
                    "NextState<{}> resource is not initialized",
                    std::any::type_name::<T>()
                )
            })?;
            state.set(next);
            Ok(json!({ "queued": true }))
        });
        self.world_mut()
            .resource_mut::<McpStateRegistry>()
            .states
            .insert(
                name.into(),
                RegisteredState {
                    description: description.into(),
                    get: getter,
                    set: setter,
                },
            );
        self
    }

    fn register_mcp_system_access(&mut self, spec: McpSystemAccessSpec) -> &mut Self {
        self.world_mut().init_resource::<McpSystemAccessRegistry>();
        self.world_mut()
            .resource_mut::<McpSystemAccessRegistry>()
            .register(spec);
        self
    }

    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self {
        self.world_mut().init_resource::<McpCaptureTargets>();
        self.world_mut()
            .resource_mut::<McpCaptureTargets>()
            .ui_target = Some(target);
        self
    }

    fn register_mcp_checkpoint_resource<T>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> &mut Self
    where
        T: Resource + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        self.world_mut().init_resource::<McpCheckpointRegistry>();
        self.world_mut()
            .resource_mut::<McpCheckpointRegistry>()
            .register_resource::<T>(name, description);
        self
    }
}
