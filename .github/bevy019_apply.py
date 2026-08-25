from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text()


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text)


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 exact match, found {count}")
    return text.replace(old, new, 1)


def sub_once(text: str, pattern: str, repl: str, label: str) -> str:
    text, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 regex match, found {count}")
    return text


# Bevy 0.19 gamepads are entities carrying Gamepad, not a global ButtonInput resource.
path = "crates/bevy-mcp-host/src/systems/capabilities.rs"
text = read(path)
text = replace_once(
    text,
    "    let gamepad_button_available = world.contains_resource::<ButtonInput<GamepadButton>>();",
    "    let gamepad_button_available = world\n        .iter_entities()\n        .any(|entity| entity.contains::<bevy::input::gamepad::Gamepad>());",
    "capabilities gamepad availability",
)
write(path, text)

# Use Bevy's registry indexes so ambiguous short type paths never select an arbitrary registration.
for path in [
    "crates/bevy-mcp-host/src/systems/resources.rs",
    "crates/bevy-mcp-host/src/systems/ecs_mutate.rs",
]:
    text = read(path)
    text = re.sub(
        r"registry\s*\.iter\(\)\s*\.find\(\|r\| r\.type_info\(\)\.type_path_table\(\)\.short_path\(\) == (component|resource)\)",
        r"find_type_registration(&registry, \1)",
        text,
    )
    write(path, text)

path = "crates/bevy-mcp-host/src/systems/ecs_inspect.rs"
text = read(path)
text = sub_once(
    text,
    r"    let component_id = \|name: &str\| \{\n        registry\n            \.iter\(\)\n            \.find\(\|registration\| \{\n                let path = registration\.type_info\(\)\.type_path_table\(\);\n                path\.short_path\(\) == name \|\| path\.path\(\) == name\n            \}\)\n            \.and_then\(\|registration\| world\.components\(\)\.get_id\(registration\.type_id\(\)\)\)\n    \};",
    "    let component_id = |name: &str| {\n        find_type_registration(&registry, name)\n            .and_then(|registration| world.components().get_id(registration.type_id()))\n    };",
    "ecs inspect query lookup",
)
text = re.sub(
    r"registry\n        \.iter\(\)\n        \.find\(\|r\| r\.type_info\(\)\.type_path_table\(\)\.short_path\(\) == component\)",
    "find_type_registration(&registry, component)",
    text,
)
write(path, text)

path = "crates/bevy-mcp-host/src/debugger.rs"
text = read(path)
text = re.sub(
    r"registry\n                \.iter\(\)\n                \.find\(\|registration\| \{\n                    let path = registration\.type_info\(\)\.type_path_table\(\);\n                    path\.short_path\(\) == name \|\| path\.path\(\) == name\n                \}\)",
    "crate::systems::find_type_registration(&registry, name)",
    text,
)
text = re.sub(
    r"registry\.iter\(\)\.find\(\|registration\| \{\n        let path = registration\.type_info\(\)\.type_path_table\(\);\n        path\.short_path\(\) == requested \|\| path\.path\(\) == requested\n    \}\)",
    "crate::systems::find_type_registration(&registry, requested)",
    text,
)
text = re.sub(
    r"registry\n        \.iter\(\)\n        \.find\(\|registration\| \{\n            let path = registration\.type_info\(\)\.type_path_table\(\);\n            path\.short_path\(\) == requested \|\| path\.path\(\) == requested\n        \}\)",
    "crate::systems::find_type_registration(&registry, requested)",
    text,
)

# Debugger key actions run in PostUpdate. Queue them for the next PreUpdate input phase instead
# of writing a transient ButtonInput edge that Bevy would clear before gameplay sees it.
text = sub_once(
    text,
    r"fn apply_key\(world: &mut World, key: &str, pressed: bool\) -> Result<\(\), String> \{.*?\n\}\n\nfn parse_keycode",
    "fn apply_key(world: &mut World, key: &str, pressed: bool) -> Result<(), String> {\n    crate::synthetic_input::queue_key(world, key, pressed)\n}\n\nfn parse_keycode",
    "debugger queued key application",
)
text = replace_once(
    text,
    "                    complete_step(\n                        session,\n                        frame,\n                        json!({ \"type\": \"key\", \"key\": key, \"pressed\": pressed }),\n                    );\n                }",
    "                    complete_step(\n                        session,\n                        frame,\n                        json!({ \"type\": \"key\", \"key\": key, \"pressed\": pressed, \"queued_for_next_input_phase\": true }),\n                    );\n                    // Let Bevy apply the queued edge in the next PreUpdate before advancing.\n                    return;\n                }",
    "debugger key step frame boundary",
)
# The key mapping now lives in synthetic_input; remove the duplicate debugger mapping.
text = sub_once(
    text,
    r"\nfn parse_keycode\(key: &str\) -> Option<KeyCode> \{.*?\n\}\n\nfn push_result",
    "\nfn push_result",
    "remove debugger parse_keycode",
)
write(path, text)

# Validate hierarchy changes completely before mutating ChildOf. Inserting a new ChildOf lets
# Bevy's relationship hooks update the old/new Children collections atomically.
path = "crates/bevy-mcp-host/src/systems/ecs_mutate.rs"
text = read(path)
new_reparent = r'''pub(crate) fn entity_reparent(
    world: &mut World,
    entity_handle: &bevy_mcp_core::entity_handle::EntityHandle,
    parent_handle: Option<&bevy_mcp_core::entity_handle::EntityHandle>,
) -> McpResult {
    use bevy::ecs::hierarchy::ChildOf;

    let entity = match resolve_entity(world, entity_handle) {
        Some(entity) => entity,
        None => {
            return McpResult::error(
                "ENTITY_NOT_FOUND",
                format!("Entity {entity_handle} not found"),
            );
        }
    };

    let parent = match parent_handle {
        Some(parent_handle) => match resolve_entity(world, parent_handle) {
            Some(parent) => Some(parent),
            None => {
                return McpResult::error(
                    "ENTITY_NOT_FOUND",
                    format!("Parent entity {parent_handle} not found"),
                );
            }
        },
        None => None,
    };

    if let Some(parent) = parent {
        if parent == entity {
            return McpResult::error(
                "INVALID_HIERARCHY",
                "An entity cannot be its own parent",
            );
        }

        let mut cursor = Some(parent);
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = cursor {
            if current == entity {
                return McpResult::error(
                    "INVALID_HIERARCHY",
                    "Reparenting would create a hierarchy cycle",
                );
            }
            if !visited.insert(current) {
                return McpResult::error(
                    "INVALID_HIERARCHY",
                    "Existing parent chain contains a hierarchy cycle",
                );
            }
            cursor = world.get::<ChildOf>(current).map(ChildOf::parent);
        }

        let Ok(mut entity_ref) = world.get_entity_mut(entity) else {
            return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {entity_handle} not found"));
        };
        entity_ref.insert(ChildOf(parent));
        McpResult::success(json!({
            "reparented": entity_to_uri(world, entity),
            "new_parent": entity_to_uri(world, parent)
        }))
    } else {
        let Ok(mut entity_ref) = world.get_entity_mut(entity) else {
            return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {entity_handle} not found"));
        };
        entity_ref.remove::<ChildOf>();
        McpResult::success(
            json!({ "reparented": entity_to_uri(world, entity), "new_parent": null }),
        )
    }
}
'''
text = sub_once(
    text,
    r"pub\(crate\) fn entity_reparent\(.*?\n\}\n\npub\(crate\) fn entity_duplicate",
    new_reparent + "\npub(crate) fn entity_duplicate",
    "safe entity reparent",
)

hierarchy_tests = r'''

#[cfg(test)]
mod bevy_019_hierarchy_tests {
    use super::*;
    use bevy::ecs::hierarchy::{ChildOf, Children};
    use crate::instance::McpInstanceId;

    fn handle(entity: Entity) -> bevy_mcp_core::entity_handle::EntityHandle {
        bevy_mcp_core::entity_handle::EntityHandle::new(
            "test",
            "main",
            entity.index().index() as u64,
            entity.generation().to_bits() as u64,
        )
    }

    #[test]
    fn invalid_parent_preserves_existing_relationship() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let old_parent = world.spawn_empty().id();
        let child = world.spawn(ChildOf(old_parent)).id();
        let missing = world.spawn_empty().id();
        let missing_handle = handle(missing);
        world.despawn(missing);

        let result = entity_reparent(&mut world, &handle(child), Some(&missing_handle));
        assert!(matches!(result, McpResult::Error { ref code, .. } if code == "ENTITY_NOT_FOUND"));
        assert_eq!(world.get::<ChildOf>(child).map(ChildOf::parent), Some(old_parent));
        assert!(world.get::<Children>(old_parent).is_some_and(|children| children.contains(&child)));
    }

    #[test]
    fn reparent_rejects_self_and_cycles() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let root = world.spawn_empty().id();
        let child = world.spawn(ChildOf(root)).id();

        let self_result = entity_reparent(&mut world, &handle(root), Some(&handle(root)));
        assert!(matches!(self_result, McpResult::Error { ref code, .. } if code == "INVALID_HIERARCHY"));

        let cycle_result = entity_reparent(&mut world, &handle(root), Some(&handle(child)));
        assert!(matches!(cycle_result, McpResult::Error { ref code, .. } if code == "INVALID_HIERARCHY"));
        assert_eq!(world.get::<ChildOf>(child).map(ChildOf::parent), Some(root));
        assert!(world.get::<ChildOf>(root).is_none());
    }

    #[test]
    fn valid_reparent_uses_bevy_relationship_hooks() {
        let mut world = World::new();
        world.insert_resource(McpInstanceId::new("test"));
        let old_parent = world.spawn_empty().id();
        let new_parent = world.spawn_empty().id();
        let child = world.spawn(ChildOf(old_parent)).id();

        let result = entity_reparent(&mut world, &handle(child), Some(&handle(new_parent)));
        assert!(matches!(result, McpResult::Success(_)));
        assert_eq!(world.get::<ChildOf>(child).map(ChildOf::parent), Some(new_parent));
        assert!(world.get::<Children>(new_parent).is_some_and(|children| children.contains(&child)));
        assert!(world.get::<Children>(old_parent).is_none_or(|children| !children.contains(&child)));
    }
}
'''
text += hierarchy_tests
write(path, text)

# Keep schedule documentation honest: the Apply set contains runtime/input control, while reflected
# world mutations are deliberately deferred to Update.
path = "crates/bevy-mcp-host/src/schedule.rs"
text = read(path)
text = text.replace(
    "///   McpIngress → McpValidate → McpApply\n///\n/// PostUpdate:",
    "///   McpIngress → McpValidate → McpApply (runtime/input coordination)\n///\n/// Reflected ECS/resource mutations are applied in `Update` after `PreUpdate` input settles.\n///\n/// PostUpdate:",
)
write(path, text)

# Deterministically target the lowest entity-id gamepad until the wire protocol gains an explicit selector.
path = "crates/bevy-mcp-host/src/synthetic_input.rs"
text = read(path)
text = replace_once(
    text,
    "    let Some(entity) = world\n        .iter_entities()\n        .find(|entity| entity.contains::<Gamepad>())\n        .map(|entity| entity.id())",
    "    let Some(entity) = world\n        .iter_entities()\n        .filter(|entity| entity.contains::<Gamepad>())\n        .map(|entity| entity.id())\n        .min_by_key(|entity| entity.index().index())",
    "deterministic gamepad selection",
)
write(path, text)

# Rust 1.98 Clippy cleanup surfaced by the stricter repository gate.
path = "crates/bevy-mcp-host/src/operations.rs"
text = read(path)
text = replace_once(
    text,
    "            if let Some(process) = &op.process {\n                if let Ok(mut proc) = process.lock() {\n                    let _ = proc.kill();\n                }\n            }",
    "            if let Some(process) = &op.process\n                && let Ok(mut proc) = process.lock()\n            {\n                let _ = proc.kill();\n            }",
    "operations collapsible if",
)
write(path, text)

path = "crates/bevy-mcp-host/src/plugin.rs"
text = read(path)
text = replace_once(
    text,
    "        if let Some(config) = self.supervisor_bridge.clone() {\n            if let Err(error) = spawn_supervisor_bridge(\n                config,\n                ingress.inner().clone(),\n                results.inner().clone(),\n                supervisor_shutdown,\n            ) {\n                tracing::error!(%error, \"failed to start bevy-mcp supervisor bridge\");\n            }\n        }",
    "        if let Some(config) = self.supervisor_bridge.clone()\n            && let Err(error) = spawn_supervisor_bridge(\n                config,\n                ingress.inner().clone(),\n                results.inner().clone(),\n                supervisor_shutdown,\n            )\n        {\n            tracing::error!(%error, \"failed to start bevy-mcp supervisor bridge\");\n        }",
    "plugin collapsible if",
)
write(path, text)

path = "crates/bevy-mcp-host/src/systems/procedural.rs"
text = read(path)
old_block = '''        if let Some(registration) = registration {
            if let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            {
                if let Some(reflected) = reflect_component.reflect(entity_ref) {
                    let serializer = bevy::reflect::serde::ReflectSerializer::new(
                        reflected.as_reflect(),
                        &registry,
                    );
                    if let Ok(value) = serde_json::to_value(&serializer) {
                        if let Some(obj) = value.as_object() {
                            if let Some(inner) = obj.values().next() {
                                components_json.insert(short_name, inner.clone());
                                continue;
                            }
                        }
                        components_json.insert(short_name, value);
                    }
                }
            }
        }'''
new_block = '''        if let Some(registration) = registration
            && let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            && let Some(reflected) = reflect_component.reflect(entity_ref)
        {
            let serializer = bevy::reflect::serde::ReflectSerializer::new(
                reflected.as_reflect(),
                &registry,
            );
            if let Ok(value) = serde_json::to_value(&serializer) {
                if let Some(obj) = value.as_object()
                    && let Some(inner) = obj.values().next()
                {
                    components_json.insert(short_name, inner.clone());
                    continue;
                }
                components_json.insert(short_name, value);
            }
        }'''
text = replace_once(text, old_block, new_block, "procedural reflect block")
text = replace_once(
    text,
    '''    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return McpResult::error(
                "IO_ERROR",
                format!("Failed to create directory {}: {e}", parent.display()),
            );
        }
    }''',
    '''    if let Some(parent) = std::path::Path::new(&file_path).parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return McpResult::error(
            "IO_ERROR",
            format!("Failed to create directory {}: {e}", parent.display()),
        );
    }''',
    "procedural create dir",
)
text = replace_once(
    text,
    '''    if let Some((x, y, z)) = position {
        if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(Transform::from_xyz(x, y, z));
        }
    }''',
    '''    if let Some((x, y, z)) = position
        && let Ok(mut entity_ref) = world.get_entity_mut(entity)
    {
        entity_ref.insert(Transform::from_xyz(x, y, z));
    }''',
    "procedural position",
)
write(path, text)

path = "crates/bevy-mcp-host/src/systems/runtime.rs"
text = read(path)
text = replace_once(
    text,
    "        let mut registry = McpRegistry::default();\n        registry.time_scale = 1.0;",
    "        let mut registry = McpRegistry {\n            time_scale: 1.0,\n            ..Default::default()\n        };",
    "runtime test default init",
)
write(path, text)

# Regression-test Bevy's ambiguity semantics directly at our central lookup boundary.
path = "crates/bevy-mcp-host/src/systems.rs"
text = read(path)
reflection_tests = r'''

#[cfg(test)]
mod bevy_019_type_registry_tests {
    use super::*;
    use bevy::reflect::{Reflect, TypePath, TypeRegistry};

    mod left {
        use bevy::reflect::Reflect;
        #[derive(Reflect)]
        pub struct Duplicate;
    }

    mod right {
        use bevy::reflect::Reflect;
        #[derive(Reflect)]
        pub struct Duplicate;
    }

    #[test]
    fn ambiguous_short_paths_require_a_full_type_path() {
        let mut registry = TypeRegistry::default();
        registry.register::<left::Duplicate>();
        registry.register::<right::Duplicate>();

        assert!(registry.is_ambiguous("Duplicate"));
        assert!(find_type_registration(&registry, "Duplicate").is_none());
        assert!(find_type_registration(
            &registry,
            <left::Duplicate as TypePath>::type_path(),
        )
        .is_some());
    }
}
'''
text += reflection_tests
write(path, text)

print("Applied Bevy 0.19 correctness patches")
