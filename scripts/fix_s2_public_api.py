from pathlib import Path
import re


def read(path):
    return Path(path).read_text()


def write(path, text):
    Path(path).write_text(text)


def replace_once(text, old, new, label):
    if old not in text:
        raise SystemExit(f"missing anchor: {label}")
    return text.replace(old, new, 1)

# ---------------------------------------------------------------------------
# agent_api.rs: exact opt-in system access declarations
# ---------------------------------------------------------------------------
path = "crates/bevy-mcp-host/src/agent_api.rs"
t = read(path)

anchor = '''impl McpSystemTimings {
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
'''
addition = anchor + r'''

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
'''
t = replace_once(t, anchor, addition, "system access registry insertion")

t = replace_once(
    t,
    '''    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self;\n''',
    '''    fn register_mcp_system_access(&mut self, spec: McpSystemAccessSpec) -> &mut Self;\n\n    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self;\n''',
    "trait access registration",
)

t = replace_once(
    t,
    '''    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self {\n''',
    '''    fn register_mcp_system_access(&mut self, spec: McpSystemAccessSpec) -> &mut Self {\n        self.world_mut().init_resource::<McpSystemAccessRegistry>();\n        self.world_mut()\n            .resource_mut::<McpSystemAccessRegistry>()\n            .register(spec);\n        self\n    }\n\n    fn set_mcp_ui_capture_target(&mut self, target: Handle<Image>) -> &mut Self {\n''',
    "impl access registration",
)
write(path, t)

# ---------------------------------------------------------------------------
# plugin.rs + lib.rs: install/export registry
# ---------------------------------------------------------------------------
path = "crates/bevy-mcp-host/src/plugin.rs"
t = read(path)
t = replace_once(
    t,
    '''use crate::agent_api::{McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemTimings};''',
    '''use crate::agent_api::{\n    McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemAccessRegistry,\n    McpSystemTimings,\n};''',
    "plugin access import",
)
t = replace_once(
    t,
    '''        app.init_resource::<McpSystemTimings>();\n''',
    '''        app.init_resource::<McpSystemTimings>();\n        app.init_resource::<McpSystemAccessRegistry>();\n''',
    "plugin registry init",
)
write(path, t)

path = "crates/bevy-mcp-host/src/lib.rs"
t = read(path)
t = replace_once(
    t,
    '''    McpSystemTimings,\n''',
    '''    McpSystemAccessRegistry, McpSystemAccessSpec, McpSystemTimings,\n''',
    "lib access exports",
)
write(path, t)

# ---------------------------------------------------------------------------
# advanced.rs: public-API hybrid exact registry + conflict fallback
# ---------------------------------------------------------------------------
path = "crates/bevy-mcp-host/src/advanced.rs"
t = read(path)
t = t.replace('use bevy::ecs::query::ComponentAccessKind;\n', '')
t = t.replace('use bevy::ecs::schedule::{Schedule, Schedules, SystemKey};', 'use bevy::ecs::schedule::{Schedule, Schedules};')
t = t.replace(
    'use crate::agent_api::{McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemTimings};',
    'use crate::agent_api::{\n    McpActionRegistry, McpCaptureTargets, McpStateRegistry, McpSystemAccessRegistry,\n    McpSystemAccessSpec, McpSystemTimings,\n};',
)
t = t.replace(
    'use crate::change_tracking::WorldChangeTracker;',
    'use crate::change_tracking::{WorldChangeTracker, component_name_matches};',
)

pattern = re.compile(r'fn system_access\([\s\S]*?\nfn system_timings\(', re.M)
replacement = r'''fn system_access(
    world: &World,
    requested_system: &str,
    schedule_filter: Option<&str>,
) -> McpResult {
    let registered = world
        .get_resource::<McpSystemAccessRegistry>()
        .map(|registry| {
            registry
                .iter()
                .filter(|entry| registered_system_matches(entry, requested_system, schedule_filter))
                .map(McpSystemAccessSpec::as_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let Some(schedules) = world.get_resource::<Schedules>() else {
        if registered.is_empty() {
            return McpResult::error(
                "SCHEDULES_NOT_AVAILABLE",
                "Schedules resource is not available",
            );
        }
        return McpResult::success(json!({
            "matches": registered,
            "coverage": "registered_exact",
            "note": "Exact access is game-registered; Bevy schedule conflict fallback was unavailable.",
        }));
    };

    let mut runtime_matches = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter)) {
            continue;
        }
        let Ok(systems) = schedule.systems() else {
            continue;
        };
        for (_, system) in systems {
            let system_name = system.name().to_string();
            if !system_name_matches(&system_name, requested_system) {
                continue;
            }
            let exact = world
                .get_resource::<McpSystemAccessRegistry>()
                .and_then(|registry| {
                    registry.iter().find(|entry| {
                        registered_system_matches(entry, &system_name, Some(&schedule_name))
                    })
                })
                .map(McpSystemAccessSpec::as_json);
            runtime_matches.push(json!({
                "system": system_name,
                "schedule": schedule_name,
                "exact_access": exact,
                "conflicts": conflict_rows_for_system(world, schedule, requested_system),
                "coverage": if exact.is_some() { "registered_exact" } else { "conflict_only" },
            }));
        }
    }

    if runtime_matches.is_empty() && registered.is_empty() {
        McpResult::error(
            "SYSTEM_NOT_FOUND",
            format!("System '{requested_system}' not found or schedule is not initialized"),
        )
    } else {
        McpResult::success(json!({
            "matches": runtime_matches,
            "registered": registered,
            "note": "Bevy 0.19 does not publicly expose stored per-system access sets. exact_access is present for game-registered systems; conflicts are automatic public-API evidence and do not identify which side performed a write.",
        }))
    }
}

fn registered_system_matches(
    entry: &McpSystemAccessSpec,
    requested_system: &str,
    schedule_filter: Option<&str>,
) -> bool {
    system_name_matches(&entry.system, requested_system)
        && schedule_filter.is_none_or(|filter| {
            entry
                .schedule
                .as_deref()
                .is_some_and(|schedule| schedule_name_matches(schedule, filter))
        })
}

fn conflict_rows_for_system(world: &World, schedule: &Schedule, requested_system: &str) -> Vec<Value> {
    schedule
        .graph()
        .conflicting_systems()
        .to_string(schedule.graph(), world.components())
        .filter_map(|(left, right, components)| {
            let left_matches = system_name_matches(&left, requested_system);
            let right_matches = system_name_matches(&right, requested_system);
            if !left_matches && !right_matches {
                return None;
            }
            Some(json!({
                "other_system": if left_matches { right } else { left },
                "components": components.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }))
        })
        .collect()
}

fn writers_for(
    world: &World,
    requested: &str,
    schedule_filter: Option<&str>,
    requested_kind: &str,
) -> McpResult {
    let Some(info) = world
        .components()
        .iter_registered()
        .find(|info| component_name_matches(&info.name().to_string(), requested))
    else {
        return McpResult::error(
            "TYPE_NOT_REGISTERED",
            format!("'{requested}' is not registered in this world"),
        );
    };
    let canonical = info.name().to_string();
    let is_resource = world.contains_resource_by_id(info.id());

    let exact_writers = world
        .get_resource::<McpSystemAccessRegistry>()
        .map(|registry| {
            registry
                .iter()
                .filter(|entry| {
                    if schedule_filter.is_some_and(|filter| {
                        entry
                            .schedule
                            .as_deref()
                            .is_none_or(|schedule| !schedule_name_matches(schedule, filter))
                    }) {
                        return false;
                    }
                    let writes = if is_resource {
                        &entry.resource_writes
                    } else {
                        &entry.writes
                    };
                    entry.write_all
                        || writes
                            .iter()
                            .any(|name| component_name_matches(name, &canonical))
                })
                .map(|entry| {
                    json!({
                        "system": entry.system,
                        "schedule": entry.schedule,
                        "confidence": "registered_exact",
                        "write_all": entry.write_all,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut conflict_candidates = Vec::new();
    if let Some(schedules) = world.get_resource::<Schedules>() {
        for (label, schedule) in schedules.iter() {
            let schedule_name = format!("{label:?}");
            if schedule_filter.is_some_and(|filter| !schedule_name_matches(&schedule_name, filter)) {
                continue;
            }
            for (left, right, components) in schedule
                .graph()
                .conflicting_systems()
                .to_string(schedule.graph(), world.components())
            {
                let touches_target = components
                    .iter()
                    .map(ToString::to_string)
                    .any(|name| component_name_matches(&name, &canonical));
                if !touches_target {
                    continue;
                }
                for system in [left, right] {
                    if !conflict_candidates.iter().any(|row: &Value| {
                        row["system"].as_str() == Some(system.as_str())
                            && row["schedule"].as_str() == Some(schedule_name.as_str())
                    }) {
                        conflict_candidates.push(json!({
                            "system": system,
                            "schedule": schedule_name,
                            "confidence": "conflict_candidate",
                        }));
                    }
                }
            }
        }
    }

    McpResult::success(json!({
        "requested": requested,
        "canonical": canonical,
        "kind": if is_resource { "resource" } else { requested_kind },
        "writers": exact_writers,
        "conflict_candidates": conflict_candidates,
        "count": exact_writers.len(),
        "coverage": "registered_exact_plus_conflict_fallback",
        "note": "writers contains exact opt-in declarations. conflict_candidates is automatic Bevy conflict evidence: either side may be the writer, and a sole writer with no conflicting system will not appear there.",
    }))
}

fn system_timings('''
new_t, count = pattern.subn(replacement, t, count=1)
if count != 1:
    raise SystemExit(f"system access block replacement count={count}")
write(path, new_t)

# ---------------------------------------------------------------------------
# server tool descriptions: don't overclaim automatic exact access
# ---------------------------------------------------------------------------
path = "crates/bevy-mcp-server/src/advanced_tools.rs"
t = read(path)
t = t.replace(
    'Inspect the declared ECS read/write access of a system, including resources and unbounded World access.',
    'Inspect system causal-access evidence. Returns exact access for MCP-registered systems and Bevy conflict evidence as a public-API fallback.',
)
t = t.replace(
    'Find initialized Bevy systems that can write a component. Useful for runtime-to-code causal debugging.',
    'Find exact MCP-registered component writers plus automatic Bevy conflict candidates for runtime-to-code causal debugging.',
)
t = t.replace(
    'Find initialized Bevy systems that can write a resource.',
    'Find exact MCP-registered resource writers plus automatic Bevy conflict candidates.',
)
write(path, t)

# ---------------------------------------------------------------------------
# docs: show exact registration path and fallback semantics
# ---------------------------------------------------------------------------
path = "docs/debugging-intelligence.md"
t = read(path)
old = '''## Runtime-to-system causality\n\nUse `system_access` to inspect a system's declared ECS reads/writes and `component_writers` / `resource_writers` to identify candidate systems capable of causing a runtime mutation. Unbounded `&World` / `&mut World` access is reported explicitly.\n\nWriter discovery is based on Bevy's declared system access metadata. It narrows a runtime symptom to systems that *can* perform the write; it does not claim that a particular candidate actually performed a specific write on a specific frame. Exact write provenance would require additional instrumentation.\n'''
new = '''## Runtime-to-system causality\n\nBevy 0.19 keeps the initialized per-system access set private, so the MCP uses a hybrid causal index instead of relying on private fields or unsafe layout assumptions. Register important systems when exact read/write attribution is required:\n\n```rust\napp.register_mcp_system_access(\n    McpSystemAccessSpec::new("combat::apply_damage")\n        .schedule("Update")\n        .read::<DamageEvent>()\n        .write::<Health>()\n        .write_resource::<CombatStats>(),\n);\n```\n\n`system_access`, `component_writers`, and `resource_writers` return these declarations as `registered_exact`. For systems that are not registered, the MCP also uses Bevy's public schedule conflict graph to return `conflict_candidate` evidence. Conflict candidates are deliberately labelled as incomplete: either side of a conflict may be the writer, and a sole writer with no conflicting system cannot be inferred from that graph.\n\nThis narrows a runtime symptom to likely source systems without claiming exact per-frame write provenance. Exact "system X performed this write on frame Y" attribution would require runtime instrumentation around the system execution itself.\n'''
if old not in t:
    raise SystemExit("docs causality anchor not found")
t = t.replace(old, new, 1)
write(path, t)
