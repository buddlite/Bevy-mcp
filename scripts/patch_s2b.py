from pathlib import Path
import re

ROOT = Path('.')

def read(path):
    return (ROOT / path).read_text()

def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)

def replace_once(text, old, new, label):
    if old not in text:
        raise RuntimeError(f'missing replacement target: {label}')
    if text.count(old) != 1:
        raise RuntimeError(f'non-unique replacement target ({text.count(old)}): {label}')
    return text.replace(old, new, 1)

def regex_once(text, pattern, repl, label):
    out, n = re.subn(pattern, repl, text, count=1, flags=re.S)
    if n != 1:
        raise RuntimeError(f'regex replacement count={n}: {label}')
    return out

# S2/S3 host advanced dispatcher and access helpers.
path = 'crates/bevy-mcp-host/src/advanced.rs'
t = read(path)
t = replace_once(t, 'use bevy::ecs::hierarchy::{ChildOf, Children};\nuse bevy::ecs::schedule::{Schedule, Schedules};\n', 'use bevy::ecs::hierarchy::{ChildOf, Children};\nuse bevy::ecs::query::ComponentAccessKind;\nuse bevy::ecs::schedule::{Schedule, Schedules, SystemKey};\n', 'advanced access imports')
t = replace_once(t, '''        AdvancedRequest::SystemInspect { system, schedule } => push_result(
            world,
            request_id,
            system_inspect(world, &system, schedule.as_deref()),
        ),
        AdvancedRequest::SystemTimings { schedule } => {
''', '''        AdvancedRequest::SystemInspect { system, schedule } => push_result(
            world,
            request_id,
            system_inspect(world, &system, schedule.as_deref()),
        ),
        AdvancedRequest::SystemAccess { system, schedule } => push_result(
            world,
            request_id,
            system_access(world, &system, schedule.as_deref()),
        ),
        AdvancedRequest::ComponentWriters { component, schedule } => push_result(
            world,
            request_id,
            writers_for(world, &component, schedule.as_deref(), "component"),
        ),
        AdvancedRequest::ResourceWriters { resource, schedule } => push_result(
            world,
            request_id,
            writers_for(world, &resource, schedule.as_deref(), "resource"),
        ),
        AdvancedRequest::TrackingConfig { mode, history_frames, components, resources, exclude_components, exclude_resources } => {
            let result = world.resource_mut::<WorldChangeTracker>().configure(
                mode.as_deref(), history_frames, components, resources, exclude_components, exclude_resources,
            );
            push_result(world, request_id, result.map(McpResult::success).unwrap_or_else(|e| McpResult::error("INVALID_TRACKING_CONFIG", e)));
        }
        AdvancedRequest::TrackingStatus => {
            push_result(world, request_id, McpResult::success(world.resource::<WorldChangeTracker>().status_json()));
        }
        AdvancedRequest::SystemTimings { schedule } => {
''', 'advanced handler S2 S3')

marker = 'fn system_timings(world: &World, schedule_filter: Option<&str>) -> McpResult {'
idx = t.index(marker)
helpers = r'''fn system_access(world: &World, requested_system: &str, schedule_filter: Option<&str>) -> McpResult {
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return McpResult::error("SCHEDULES_NOT_AVAILABLE", "Schedules resource is not available");
    };
    let mut matches = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|f| !schedule_name_matches(&schedule_name, f)) { continue; }
        let Ok(systems) = schedule.systems() else { continue; };
        for (key, system) in systems {
            if system_name_matches(&system.name().to_string(), requested_system) {
                matches.push(system_access_row(world, schedule, key, &schedule_name, &system.name().to_string()));
            }
        }
    }
    if matches.is_empty() {
        McpResult::error("SYSTEM_NOT_FOUND", format!("System '{requested_system}' not found or schedule is not initialized"))
    } else {
        McpResult::success(json!({ "matches": matches }))
    }
}

fn system_access_row(world: &World, schedule: &Schedule, key: SystemKey, schedule_name: &str, system_name: &str) -> Value {
    let Some(system) = schedule.graph().systems.get(key) else {
        return json!({ "system": system_name, "schedule": schedule_name, "initialized": false });
    };
    let access = system.access.combined_access();
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut archetypal = Vec::new();
    let unbounded = access.try_iter_access().is_err();
    if let Ok(entries) = access.try_iter_access() {
        for entry in entries {
            let (id, target) = match entry {
                ComponentAccessKind::Shared(id) => (id, &mut reads),
                ComponentAccessKind::Exclusive(id) => (id, &mut writes),
                ComponentAccessKind::Archetypal(id) => (id, &mut archetypal),
            };
            let name = world.components().get_info(id).map(|i| i.name().to_string()).unwrap_or_else(|| format!("component#{}", id.index()));
            let kind = if world.contains_resource_by_id(id) { "resource" } else { "component" };
            target.push(json!({ "name": name, "kind": kind, "id": id.index() }));
        }
    }
    json!({
        "system": system_name,
        "schedule": schedule_name,
        "reads": reads,
        "writes": writes,
        "archetypal": archetypal,
        "read_all": access.has_read_all(),
        "write_all": access.has_write_all(),
        "unbounded": unbounded,
    })
}

fn writers_for(world: &World, requested: &str, schedule_filter: Option<&str>, requested_kind: &str) -> McpResult {
    let Some(info) = world.components().iter_registered().find(|info| component_name_matches(&info.name().to_string(), requested)) else {
        return McpResult::error("TYPE_NOT_REGISTERED", format!("'{requested}' is not registered in this world"));
    };
    let id = info.id();
    let canonical = info.name().to_string();
    let is_resource = world.contains_resource_by_id(id);
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return McpResult::error("SCHEDULES_NOT_AVAILABLE", "Schedules resource is not available");
    };
    let mut writers = Vec::new();
    for (label, schedule) in schedules.iter() {
        let schedule_name = format!("{label:?}");
        if schedule_filter.is_some_and(|f| !schedule_name_matches(&schedule_name, f)) { continue; }
        let Ok(systems) = schedule.systems() else { continue; };
        for (key, system) in systems {
            let Some(with_access) = schedule.graph().systems.get(key) else { continue; };
            let access = with_access.access.combined_access();
            if access.has_write(id) || access.has_write_all() {
                writers.push(json!({
                    "system": system.name().to_string(),
                    "schedule": schedule_name,
                    "write_all": access.has_write_all(),
                }));
            }
        }
    }
    McpResult::success(json!({
        "requested": requested,
        "canonical": canonical,
        "kind": if is_resource { "resource" } else { requested_kind },
        "writers": writers,
        "count": writers.len(),
    }))
}

'''
t = t[:idx] + helpers + t[idx:]
write(path, t)

# Add S2/S3 server parameters and tools.
path = 'crates/bevy-mcp-server/src/advanced_tools.rs'
t = read(path)
insert_after = '''pub struct SystemInspectParams {
    pub system: String,
    pub schedule: Option<String>,
}
'''
extra = r'''
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriterSearchParams {
    pub name: String,
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrackingConfigParams {
    #[schemars(description = "Tracking mode: full or scoped. Scoped only snapshots subscribed component/resource ticks.")]
    pub mode: Option<String>,
    pub history_frames: Option<usize>,
    pub components: Option<Vec<String>>,
    pub resources: Option<Vec<String>>,
    pub exclude_components: Option<Vec<String>>,
    pub exclude_resources: Option<Vec<String>>,
}
'''
t = replace_once(t, insert_after, insert_after + extra, 'advanced server params S2 S3')
needle = '''    #[tool(description = "Return explicit timing samples recorded through the MCP timing registry.")]
    async fn system_timings'''
methods = r'''    #[tool(description = "Inspect the declared ECS read/write access of a system, including resources and unbounded World access.")]
    async fn system_access(&self, Parameters(params): Parameters<SystemInspectParams>) -> String {
        self.state.call(AdvancedRequest::SystemAccess { system: params.system, schedule: params.schedule }).await
    }

    #[tool(description = "Find initialized Bevy systems that can write a component. Useful for runtime-to-code causal debugging.")]
    async fn component_writers(&self, Parameters(params): Parameters<WriterSearchParams>) -> String {
        self.state.call(AdvancedRequest::ComponentWriters { component: params.name, schedule: params.schedule }).await
    }

    #[tool(description = "Find initialized Bevy systems that can write a resource.")]
    async fn resource_writers(&self, Parameters(params): Parameters<WriterSearchParams>) -> String {
        self.state.call(AdvancedRequest::ResourceWriters { resource: params.name, schedule: params.schedule }).await
    }

    #[tool(description = "Configure world-change tracking. Use scoped mode to reduce per-frame component/resource tick snapshot cost.")]
    async fn tracking_config(&self, Parameters(params): Parameters<TrackingConfigParams>) -> String {
        self.state.call(AdvancedRequest::TrackingConfig {
            mode: params.mode,
            history_frames: params.history_frames,
            components: params.components,
            resources: params.resources,
            exclude_components: params.exclude_components,
            exclude_resources: params.exclude_resources,
        }).await
    }

    #[tool(description = "Inspect current change-tracking mode, history, explicit scopes, and debugger-derived subscriptions.")]
    async fn tracking_status(&self) -> String {
        self.state.call(AdvancedRequest::TrackingStatus).await
    }

'''
if needle not in t:
    raise RuntimeError('missing system_timings insertion point')
t = t.replace(needle, methods + needle, 1)
write(path, t)
