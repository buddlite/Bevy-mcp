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

print("Applied Bevy 0.19 correctness patches")
