from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}")
    file.write_text(text.replace(old, new, 1))


interaction_path = "crates/bevy-mcp-host/src/interaction.rs"
replace_once(
    interaction_path,
    '''fn ui_location(world: &World, entity: Entity) -> Result<Location, McpResult> {
    picking_available(world)?;
    let transform = world
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .ok_or_else(|| {
            McpResult::error(
                "UI_LAYOUT_NOT_READY",
                "UI entity has no UiGlobalTransform; wait for UI layout to run",
            )
        })?;
    let position = transform.transform_point2(Vec2::ZERO);
''',
    '''fn logical_ui_center(
    transform: &bevy::ui::UiGlobalTransform,
    computed: &bevy::ui::ComputedNode,
) -> Vec2 {
    transform.transform_point2(Vec2::ZERO) * computed.inverse_scale_factor()
}

fn ui_location(world: &World, entity: Entity) -> Result<Location, McpResult> {
    picking_available(world)?;
    let transform = world
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .ok_or_else(|| {
            McpResult::error(
                "UI_LAYOUT_NOT_READY",
                "UI entity has no UiGlobalTransform; wait for UI layout to run",
            )
        })?;
    let computed = world.get::<bevy::ui::ComputedNode>(entity).ok_or_else(|| {
        McpResult::error(
            "UI_LAYOUT_NOT_READY",
            "UI entity has no ComputedNode; wait for UI layout to run",
        )
    })?;
    let position = logical_ui_center(transform, computed);
''',
)

interaction = Path(interaction_path)
text = interaction.read_text()
if "fn ui_center_converts_physical_layout_to_logical_pointer_coordinates()" not in text:
    text += r'''

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_center_converts_physical_layout_to_logical_pointer_coordinates() {
        let transform = bevy::ui::UiGlobalTransform::from_xy(240.0, 160.0);
        let mut computed = bevy::ui::ComputedNode::default();
        computed.inverse_scale_factor = 0.5;

        assert_eq!(
            logical_ui_center(&transform, &computed),
            Vec2::new(120.0, 80.0)
        );
    }
}
'''
interaction.write_text(text)

replace_once(
    "docs/agent-interaction.md",
    "`ui_click(entity)` computes the UI node's center, moves the software pointer there, verifies that the requested node or one of its descendants is actually among Bevy's resolved picks, then sends native press/release input. This preserves Bevy event bubbling while preventing a click from silently landing on an unrelated entity.",
    "`ui_click(entity)` computes the UI node's center, converts Bevy UI's physical-pixel layout coordinates to logical pointer coordinates using the node's inverse scale factor, moves the software pointer there, verifies that the requested node or one of its descendants is actually among Bevy's resolved picks, then sends native press/release input. This preserves Bevy event bubbling, works correctly with HiDPI/window scaling, and prevents a click from silently landing on an unrelated entity.",
)

replace_once(
    "docs/tool-capabilities.md",
    "Known interaction surfaces reserved for the next Agent Interaction work—mouse motion, UI click/type, camera framing/transform/look-at—report `implemented: false` instead of being advertised as working. Asset inspection/reload and embedded cargo build/test surfaces likewise report false.",
    "Native pointer motion/picking, UI click/type, and camera framing/transform/look-at are implemented by the Agent Interaction layer; their `available`, `allowed`, and `operational` fields still reflect the live app and permission state. Asset inspection/reload and embedded cargo build/test surfaces remain unimplemented and report false.",
)
