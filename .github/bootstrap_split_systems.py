from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates/bevy-mcp-host/src/systems.rs"
OUT = ROOT / "crates/bevy-mcp-host/src/systems"

source = SRC.read_text(encoding="utf-8")


def at(anchor: str) -> int:
    try:
        return source.index(anchor)
    except ValueError as exc:
        raise SystemExit(f"missing systems.rs anchor: {anchor!r}") from exc


anchors = {
    "command_allowed": at("fn command_allowed("),
    "runtime_system": at("pub fn runtime_system("),
    "deferred_apply": at("pub fn deferred_apply_system("),
    "mutation_operation_name": at("fn mutation_operation_name("),
    "execute_command": at("fn execute_command("),
    "capability": at("fn capability("),
    "world_summary": at("fn world_summary("),
    "resource_list": at("fn resource_list("),
    "entity_spawn": at("fn entity_spawn("),
    "runtime_pause": at("fn runtime_pause("),
    "hierarchy": at("fn hierarchy("),
    "observe_events": at("fn observe_events("),
    "ui_query": at("fn ui_query("),
    "active_camera": at("fn active_camera_entity("),
    "playtest_run": at("fn playtest_run("),
    "list_plugins": at("fn list_plugins("),
    "asset_list": at("fn asset_list("),
    "capture_game": at("fn capture_game("),
    "parse_keycode": at("fn parse_keycode("),
    "mesh_spawn": at("fn mesh_spawn_apply("),
}

order = list(anchors.values())
if order != sorted(order):
    raise SystemExit("systems.rs anchors are no longer in the expected order")

header = source[: anchors["command_allowed"]]

parts = {
    "dispatch": [
        source[anchors["command_allowed"] : anchors["runtime_system"]],
        source[anchors["deferred_apply"] : anchors["mutation_operation_name"]],
        source[anchors["execute_command"] : anchors["capability"]],
    ],
    "runtime": [
        source[anchors["runtime_system"] : anchors["deferred_apply"]],
        source[anchors["runtime_pause"] : anchors["hierarchy"]],
        source[anchors["observe_events"] : anchors["ui_query"]],
        source[anchors["list_plugins"] : anchors["asset_list"]],
    ],
    "ecs_mutate": [
        source[anchors["mutation_operation_name"] : anchors["execute_command"]],
        source[anchors["entity_spawn"] : anchors["runtime_pause"]],
    ],
    "capabilities": [source[anchors["capability"] : anchors["world_summary"]]],
    "ecs_inspect": [
        source[anchors["world_summary"] : anchors["resource_list"]],
        source[anchors["hierarchy"] : anchors["observe_events"]],
    ],
    "resources": [source[anchors["resource_list"] : anchors["entity_spawn"]]],
    "ui": [source[anchors["ui_query"] : anchors["active_camera"]]],
    "camera": [
        source[anchors["active_camera"] : anchors["playtest_run"]],
        source[anchors["capture_game"] : anchors["parse_keycode"]],
    ],
    "assertions": [source[anchors["playtest_run"] : anchors["list_plugins"]]],
    "assets": [source[anchors["asset_list"] : anchors["capture_game"]]],
    "input": [source[anchors["parse_keycode"] : anchors["mesh_spawn"]]],
    "procedural": [source[anchors["mesh_spawn"] :]],
}

# Every byte after the shared import header must belong to exactly one extracted
# range. This guards the refactor against silently dropping future functions.
covered = sum(len(chunk) for group in parts.values() for chunk in group)
if covered != len(source) - len(header):
    raise SystemExit(
        f"split coverage mismatch: covered {covered} bytes, expected {len(source) - len(header)}"
    )


def internalize_top_level_functions(text: str) -> str:
    # Nested helper functions are indented and deliberately remain private.
    return re.sub(r"(?m)^fn ([A-Za-z_][A-Za-z0-9_]*)\(", r"pub(crate) fn \1(", text)


cross_imports = {
    "dispatch": """use super::assertions::*;
use super::assets::*;
use super::camera::*;
use super::capabilities::*;
use super::ecs_inspect::*;
use super::ecs_mutate::*;
use super::input::*;
use super::procedural::*;
use super::resources::*;
use super::runtime::*;
use super::ui::*;
""",
    "ecs_mutate": "use super::resources::resource_update;\n",
    "capabilities": "use super::camera::active_camera_entity;\n",
    "resources": "use super::ecs_inspect::component_schema;\n",
    "procedural": "use super::ecs_mutate::insert_component_by_reflect;\n",
}

OUT.mkdir(parents=True, exist_ok=True)
for name, chunks in parts.items():
    body = "\n".join(chunks)
    body = internalize_top_level_functions(body)
    # Preserve the four externally visible schedule functions at their existing
    # public API path through re-exports from systems.rs.
    if name == "dispatch":
        body = body.replace("pub(crate) fn ingress_system(", "pub fn ingress_system(")
        body = body.replace("pub(crate) fn deferred_apply_system(", "pub fn deferred_apply_system(")
    if name == "runtime":
        body = body.replace("pub(crate) fn runtime_system(", "pub fn runtime_system(")
        body = body.replace("pub(crate) fn diagnostics_system(", "pub fn diagnostics_system(")

    module = "use super::*;\n" + cross_imports.get(name, "") + "\n" + body.lstrip()
    (OUT / f"{name}.rs").write_text(module, encoding="utf-8")

module_names = [
    "assertions",
    "assets",
    "camera",
    "capabilities",
    "dispatch",
    "ecs_inspect",
    "ecs_mutate",
    "input",
    "procedural",
    "resources",
    "runtime",
    "ui",
]

facade = header.rstrip() + "\n\n"
facade += "\n".join(f"mod {name};" for name in module_names)
facade += "\n\npub use dispatch::{deferred_apply_system, ingress_system};\n"
facade += "pub use runtime::{diagnostics_system, runtime_system};\n"
SRC.write_text(facade, encoding="utf-8")

# Structural guards: this PR should eliminate the monolith, not move it whole.
if SRC.stat().st_size > 5000:
    raise SystemExit(f"systems.rs facade is unexpectedly large: {SRC.stat().st_size} bytes")
for path in OUT.glob("*.rs"):
    if path.stat().st_size > 70000:
        raise SystemExit(f"split module is still too large: {path.name} = {path.stat().st_size} bytes")

expected = {f"{name}.rs" for name in module_names}
actual = {path.name for path in OUT.glob("*.rs")}
if actual != expected:
    raise SystemExit(f"unexpected systems module set: {sorted(actual)}")

print("split systems.rs into:")
print(f"  systems.rs: {SRC.stat().st_size} bytes")
for path in sorted(OUT.glob("*.rs")):
    print(f"  systems/{path.name}: {path.stat().st_size} bytes")
