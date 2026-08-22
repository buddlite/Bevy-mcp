use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub type CheckpointResult = Result<Value, String>;
type CaptureFn = Arc<dyn Fn(&World) -> CheckpointResult + Send + Sync + 'static>;
type RestoreFn = Arc<dyn Fn(&mut World, Value) -> Result<(), String> + Send + Sync + 'static>;

#[derive(Clone)]
pub struct CheckpointAdapter {
    pub description: String,
    capture: CaptureFn,
    restore: RestoreFn,
}

#[derive(Resource, Default)]
pub struct McpCheckpointRegistry {
    adapters: HashMap<String, CheckpointAdapter>,
}

impl McpCheckpointRegistry {
    pub fn register_adapter<F, R>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        capture: F,
        restore: R,
    ) where
        F: Fn(&World) -> CheckpointResult + Send + Sync + 'static,
        R: Fn(&mut World, Value) -> Result<(), String> + Send + Sync + 'static,
    {
        self.adapters.insert(
            name.into(),
            CheckpointAdapter {
                description: description.into(),
                capture: Arc::new(capture),
                restore: Arc::new(restore),
            },
        );
    }

    pub fn register_resource<T>(&mut self, name: impl Into<String>, description: impl Into<String>)
    where
        T: Resource + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        self.register_adapter(
            name,
            description,
            |world| {
                let value = world.get_resource::<T>().ok_or_else(|| {
                    format!("Resource {} is not present", std::any::type_name::<T>())
                })?;
                serde_json::to_value(value).map_err(|e| e.to_string())
            },
            |world, value| {
                let value: T = serde_json::from_value(value).map_err(|e| e.to_string())?;
                world.insert_resource(value);
                Ok(())
            },
        );
    }

    pub fn capture(&self, world: &World) -> Result<Map<String, Value>, String> {
        let mut values = Map::new();
        let mut names: Vec<_> = self.adapters.keys().cloned().collect();
        names.sort();
        for name in names {
            let adapter = &self.adapters[&name];
            values.insert(name, (adapter.capture)(world)?);
        }
        Ok(values)
    }

    pub fn restore(&self, world: &mut World, values: &Map<String, Value>) -> Result<(), String> {
        let mut names: Vec<String> = values.keys().cloned().collect();
        names.sort();

        for name in &names {
            if !self.adapters.contains_key(name) {
                return Err(format!(
                    "Checkpoint adapter '{name}' is no longer registered"
                ));
            }
        }

        let mut rollback_values = Map::new();
        for name in &names {
            let adapter = &self.adapters[name];
            let value = (adapter.capture)(world).map_err(|error| {
                format!("Could not capture rollback state for checkpoint adapter '{name}': {error}")
            })?;
            rollback_values.insert(name.clone(), value);
        }

        let mut applied = Vec::new();
        for name in &names {
            let adapter = &self.adapters[name];
            let target = values
                .get(name)
                .expect("checkpoint name was collected from this value map")
                .clone();
            if let Err(error) = (adapter.restore)(world, target) {
                let mut rollback_errors = Vec::new();
                for rollback_name in std::iter::once(name).chain(applied.iter().rev()) {
                    let rollback_adapter = &self.adapters[rollback_name];
                    let rollback_value = rollback_values
                        .get(rollback_name)
                        .expect("rollback value captured before mutation")
                        .clone();
                    if let Err(rollback_error) = (rollback_adapter.restore)(world, rollback_value) {
                        rollback_errors.push(format!("{rollback_name}: {rollback_error}"));
                    }
                }

                if rollback_errors.is_empty() {
                    return Err(format!(
                        "Checkpoint restore failed for adapter '{name}': {error}; all touched adapters were rolled back"
                    ));
                }
                return Err(format!(
                    "Checkpoint restore failed for adapter '{name}': {error}; rollback also failed for {}",
                    rollback_errors.join(", ")
                ));
            }
            applied.push(name.clone());
        }
        Ok(())
    }

    pub fn coverage(&self) -> Value {
        let mut rows: Vec<_> = self
            .adapters
            .iter()
            .map(|(name, adapter)| json!({ "name": name, "description": adapter.description }))
            .collect();
        rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Value::Array(rows)
    }
}

#[derive(Debug, Clone)]
pub struct StoredCheckpoint {
    pub id: String,
    pub name: String,
    pub frame: u64,
    pub values: Map<String, Value>,
}

#[derive(Resource, Default)]
pub struct McpCheckpointStore {
    next_id: u64,
    checkpoints: HashMap<String, StoredCheckpoint>,
}

impl McpCheckpointStore {
    pub fn next_id(&mut self) -> String {
        self.next_id += 1;
        format!("checkpoint-{}", self.next_id)
    }
    pub fn insert(&mut self, checkpoint: StoredCheckpoint) {
        self.checkpoints.insert(checkpoint.id.clone(), checkpoint);
    }
    pub fn get(&self, id: &str) -> Option<&StoredCheckpoint> {
        self.checkpoints.get(id)
    }
    pub fn list(&self) -> Vec<Value> {
        let mut rows: Vec<_> = self.checkpoints.values().map(|c| json!({ "id": c.id, "name": c.name, "frame": c.frame, "entries": c.values.len() })).collect();
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        rows
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordedAction {
    SemanticAction { action: String, args: Value },
    StateTransition { state: String, value: Value },
    Key { key: String, pressed: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedEvent {
    pub offset_frames: u64,
    pub action: RecordedAction,
}

#[derive(Debug, Clone)]
pub struct Recording {
    pub id: String,
    pub name: String,
    pub start_frame: u64,
    pub events: Vec<RecordedEvent>,
}

#[derive(Debug, Clone)]
pub struct ActiveRecording {
    pub id: String,
    pub name: String,
    pub start_frame: u64,
    pub events: Vec<RecordedEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStatus {
    Running,
    Passed,
    Failed,
    Cancelled,
}
impl ReplayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReplayRuntime {
    pub id: String,
    pub recording_id: String,
    pub checkpoint_id: Option<String>,
    pub start_frame: u64,
    pub next_event: usize,
    pub status: ReplayStatus,
    pub failure: Option<String>,
}

#[derive(Resource, Default)]
pub struct McpRecorder {
    next_id: u64,
    pub active: Option<ActiveRecording>,
    pub recordings: HashMap<String, Recording>,
    pub replays: HashMap<String, ReplayRuntime>,
}

impl McpRecorder {
    fn alloc(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }
    pub fn start(&mut self, name: String, frame: u64) -> Result<String, String> {
        if self.active.is_some() {
            return Err("A recording is already active".into());
        }
        let id = self.alloc("recording");
        self.active = Some(ActiveRecording {
            id: id.clone(),
            name,
            start_frame: frame,
            events: Vec::new(),
        });
        Ok(id)
    }
    pub fn stop(&mut self) -> Result<Recording, String> {
        let active = self
            .active
            .take()
            .ok_or_else(|| "No recording is active".to_string())?;
        let recording = Recording {
            id: active.id,
            name: active.name,
            start_frame: active.start_frame,
            events: active.events,
        };
        self.recordings
            .insert(recording.id.clone(), recording.clone());
        Ok(recording)
    }
    pub fn record(&mut self, frame: u64, action: RecordedAction) {
        if let Some(active) = self.active.as_mut() {
            active.events.push(RecordedEvent {
                offset_frames: frame.saturating_sub(active.start_frame),
                action,
            });
        }
    }
    pub fn start_replay(
        &mut self,
        recording_id: String,
        checkpoint_id: Option<String>,
        frame: u64,
    ) -> Result<String, String> {
        if !self.recordings.contains_key(&recording_id) {
            return Err(format!("Recording '{recording_id}' not found"));
        }
        if self
            .replays
            .values()
            .any(|r| r.status == ReplayStatus::Running)
        {
            return Err("A replay is already running".into());
        }
        let id = self.alloc("replay");
        self.replays.insert(
            id.clone(),
            ReplayRuntime {
                id: id.clone(),
                recording_id,
                checkpoint_id,
                start_frame: frame,
                next_event: 0,
                status: ReplayStatus::Running,
                failure: None,
            },
        );
        Ok(id)
    }
    pub fn list_recordings(&self) -> Vec<Value> {
        let mut rows: Vec<_> = self.recordings.values().map(|r| json!({ "id": r.id, "name": r.name, "start_frame": r.start_frame, "events": r.events.len() })).collect();
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        rows
    }
    pub fn replay_json(&self, id: &str) -> Option<Value> {
        self.replays.get(id).map(|r| json!({ "id": r.id, "recording_id": r.recording_id, "checkpoint_id": r.checkpoint_id, "start_frame": r.start_frame, "next_event": r.next_event, "status": r.status.as_str(), "failure": r.failure }))
    }
}
