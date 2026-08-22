from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one anchor, found {count}")
    file.write_text(text.replace(old, new, 1))


# S3: dynamic interests become an idempotent replaceable set. In scoped mode,
# snapshots reset only when that set actually changes.
replace_once(
    "crates/bevy-mcp-host/src/change_tracking.rs",
    '''    pub fn add_dynamic_interests<I, J>(&mut self, components: I, resources: J)
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
    {
        self.dynamic_components.extend(components);
        self.dynamic_resources.extend(resources);
    }

    pub fn clear_dynamic_interests(&mut self) {
        self.dynamic_components.clear();
        self.dynamic_resources.clear();
        if self.mode == TrackingMode::Scoped {
            self.reset_snapshots();
            self.previous_resources.clear();
        }
    }
''',
    '''    pub fn add_dynamic_interests<I, J>(&mut self, components: I, resources: J)
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
''',
)

# S3: after watchpoints/playtests advance, reconcile dynamic subscriptions from
# only the still-active debugger conditions. This removes stale subscriptions
# from removed/one-shot watchpoints and completed/cancelled playtests.
replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''    tick_replays(world, frame);
    world.insert_resource(debugger);
}
''',
    '''    tick_replays(world, frame);
    world.insert_resource(debugger);
    reconcile_dynamic_tracking_interests(world);
}
''',
)

replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    '''fn register_condition_tracking_interests(world: &mut World, condition: &DebugCondition) {
    let mut components = Vec::new();
    let mut resources = Vec::new();
    match condition {
        DebugCondition::QueryCount { query, .. } => {
            components.extend(query.with_components.clone());
            components.extend(query.without_components.clone());
            components.extend(query.changed.clone());
            components.extend(query.parent_has.clone());
            components.extend(query.child_has.clone());
            components.extend(query.predicates.keys().filter_map(|path| {
                path.split_once('.')
                    .map(|(component, _)| component.to_string())
            }));
        }
        DebugCondition::EntityField { component, .. } => components.push(component.clone()),
        DebugCondition::ResourceField { resource, .. } => resources.push(resource.clone()),
        DebugCondition::ChangeOccurred {
            component,
            resource,
            ..
        } => {
            if let Some(component) = component {
                components.push(component.clone());
            }
            if let Some(resource) = resource {
                resources.push(resource.clone());
            }
        }
        _ => {}
    }
    world
        .resource_mut::<WorldChangeTracker>()
        .add_dynamic_interests(components, resources);
}
''',
    '''fn collect_condition_tracking_interests(
    condition: &DebugCondition,
    components: &mut Vec<String>,
    resources: &mut Vec<String>,
) {
    match condition {
        DebugCondition::QueryCount { query, .. } => {
            components.extend(query.with_components.clone());
            components.extend(query.without_components.clone());
            components.extend(query.changed.clone());
            components.extend(query.parent_has.clone());
            components.extend(query.child_has.clone());
            components.extend(query.predicates.keys().filter_map(|path| {
                path.split_once('.')
                    .map(|(component, _)| component.to_string())
            }));
        }
        DebugCondition::EntityField { component, .. } => components.push(component.clone()),
        DebugCondition::ResourceField { resource, .. } => resources.push(resource.clone()),
        DebugCondition::ChangeOccurred {
            component,
            resource,
            ..
        } => {
            if let Some(component) = component {
                components.push(component.clone());
            }
            if let Some(resource) = resource {
                resources.push(resource.clone());
            }
        }
        _ => {}
    }
}

fn reconcile_dynamic_tracking_interests(world: &mut World) {
    let (components, resources) = {
        let debugger = world.resource::<McpDebugger>();
        let mut components = Vec::new();
        let mut resources = Vec::new();

        for watchpoint in debugger.watchpoints.values().filter(|watchpoint| watchpoint.enabled) {
            collect_condition_tracking_interests(
                &watchpoint.spec.condition,
                &mut components,
                &mut resources,
            );
        }

        for session in debugger
            .playtests
            .values()
            .filter(|session| session.status == PlaytestStatus::Running)
        {
            for step in session.plan.steps.iter().skip(session.step_index) {
                match step {
                    DebugPlaytestStep::Wait { condition, .. }
                    | DebugPlaytestStep::Assert { condition, .. } => {
                        collect_condition_tracking_interests(
                            condition,
                            &mut components,
                            &mut resources,
                        );
                    }
                    _ => {}
                }
            }
        }

        (components, resources)
    };

    world
        .resource_mut::<WorldChangeTracker>()
        .set_dynamic_interests(components, resources);
}

fn register_condition_tracking_interests(world: &mut World, condition: &DebugCondition) {
    let mut components = Vec::new();
    let mut resources = Vec::new();
    collect_condition_tracking_interests(condition, &mut components, &mut resources);
    world
        .resource_mut::<WorldChangeTracker>()
        .add_dynamic_interests(components, resources);
}
''',
)

replace_once(
    "crates/bevy-mcp-host/src/debugger.rs",
    "use crate::entity_handle::{entity_to_uri, resolve_entity};\n",
    "use crate::entity_handle::resolve_entity;\n",
)

# S4: make checkpoint restore transactional. Capture all rollback state before
# the first mutation, validate adapters up front, and restore both the failing
# adapter and every already-applied adapter if anything errors.
replace_once(
    "crates/bevy-mcp-host/src/checkpoint.rs",
    '''    pub fn restore(&self, world: &mut World, values: &Map<String, Value>) -> Result<(), String> {
        for (name, value) in values {
            let adapter = self
                .adapters
                .get(name)
                .ok_or_else(|| format!("Checkpoint adapter '{name}' is no longer registered"))?;
            (adapter.restore)(world, value.clone())?;
        }
        Ok(())
    }
''',
    '''    pub fn restore(&self, world: &mut World, values: &Map<String, Value>) -> Result<(), String> {
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
''',
)

# Integration coverage for replacement semantics, rollback, and recording offsets.
test_path = Path("crates/bevy-mcp-host/tests/intelligence.rs")
test_text = test_path.read_text()
test_text = test_text.replace(
    '''    McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpSystemAccessRegistry,
    McpSystemAccessSpec,
''',
    '''    McpAgentAppExt, McpCheckpointRegistry, McpCheckpointStore, McpRecorder,
    McpSystemAccessRegistry, McpSystemAccessSpec, RecordedAction,
''',
    1,
)
if "McpRecorder" not in test_text:
    raise SystemExit("test import anchor replacement failed")

append = r'''

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct RestoreFirst(i32);

#[derive(Resource, Serialize, Deserialize, Debug, PartialEq)]
struct RestoreSecond(i32);

#[test]
fn scoped_dynamic_interests_replace_stale_subscriptions() {
    let mut tracker = WorldChangeTracker::default();
    tracker.configure(Some("scoped"), None, None, None, None, None).unwrap();
    tracker.add_dynamic_interests(vec!["Cargo".into()], vec!["Clock".into()]);
    tracker.set_dynamic_interests(vec!["Health".into()], Vec::<String>::new());

    let status = tracker.status_json();
    let components = status["dynamic_components"].as_array().unwrap();
    assert!(components.contains(&serde_json::json!("Health")));
    assert!(!components.contains(&serde_json::json!("Cargo")));
    assert!(status["dynamic_resources"].as_array().unwrap().is_empty());
}

#[test]
fn checkpoint_restore_rolls_back_partial_failure() {
    let mut app = App::new();
    app.insert_resource(RestoreFirst(1));
    app.insert_resource(RestoreSecond(2));
    app.register_mcp_checkpoint_resource::<RestoreFirst>("a_first", "first restore resource");
    app.world_mut()
        .resource_mut::<McpCheckpointRegistry>()
        .register_adapter(
            "b_second",
            "adapter that fails only for the requested target",
            |world| Ok(serde_json::json!(world.resource::<RestoreSecond>().0)),
            |world, value| {
                let value = value
                    .as_i64()
                    .ok_or_else(|| "RestoreSecond value must be an integer".to_string())?
                    as i32;
                world.resource_mut::<RestoreSecond>().0 = value;
                if value == 20 {
                    Err("intentional restore failure".into())
                } else {
                    Ok(())
                }
            },
        );

    let mut target = serde_json::Map::new();
    target.insert("a_first".into(), serde_json::json!(10));
    target.insert("b_second".into(), serde_json::json!(20));
    let error = app
        .world_mut()
        .resource_scope(|world, registry: Mut<McpCheckpointRegistry>| registry.restore(world, &target))
        .unwrap_err();

    assert!(error.contains("rolled back"));
    assert_eq!(app.world().resource::<RestoreFirst>(), &RestoreFirst(1));
    assert_eq!(app.world().resource::<RestoreSecond>(), &RestoreSecond(2));
}

#[test]
fn recordings_preserve_frame_offsets_for_replay() {
    let mut recorder = McpRecorder::default();
    let recording_id = recorder.start("route test".into(), 100).unwrap();
    recorder.record(
        103,
        RecordedAction::SemanticAction {
            action: "spawn_ship".into(),
            args: serde_json::json!({"id": 7}),
        },
    );
    recorder.record(
        111,
        RecordedAction::StateTransition {
            state: "phase".into(),
            value: serde_json::json!("running"),
        },
    );
    let recording = recorder.stop().unwrap();
    assert_eq!(recording.id, recording_id);
    assert_eq!(recording.events[0].offset_frames, 3);
    assert_eq!(recording.events[1].offset_frames, 11);

    let replay_id = recorder
        .start_replay(recording.id.clone(), Some("checkpoint-1".into()), 500)
        .unwrap();
    let replay = recorder.replays.get(&replay_id).unwrap();
    assert_eq!(replay.start_frame, 500);
    assert_eq!(replay.next_event, 0);
    assert_eq!(replay.checkpoint_id.as_deref(), Some("checkpoint-1"));
}
'''
if "checkpoint_restore_rolls_back_partial_failure" in test_text:
    raise SystemExit("hardening tests already present")
test_path.write_text(test_text.rstrip() + append + "\n")
