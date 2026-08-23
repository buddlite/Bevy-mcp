# Agent Interaction

The MCP uses a dedicated software pointer that feeds Bevy's native picking pipeline instead of mutating UI `Interaction` state directly.

## Pointer tools

- `pick_at(x, y)` moves the MCP pointer and returns the ordered Bevy picking hits.
- `pointer_move(x, y)` moves the MCP pointer.
- `pointer_click(x, y, button?)` performs move → press → release across frames.
- `pointer_drag(...)` performs move → press → interpolated moves → release.
- `pointer_scroll(x, y, delta_y, delta_x?)` emits a native pointer scroll.
- legacy `input_mouse` uses the same software-pointer path.

Coordinates are logical pixels on the primary window. Picking backends decide what those coordinates hit, so the same tools can target Bevy UI, meshes, or other installed picking backends.

A single persistent MCP pointer is intentionally serialized: pointer press/drag state is sequential by nature. Other MCP requests remain concurrently correlated by the shared response dispatcher.

## UI

`ui_click(entity)` computes the UI node's center, moves the software pointer there, verifies that the requested node or one of its descendants is actually among Bevy's resolved picks, then sends native press/release input. This preserves Bevy event bubbling while preventing a click from silently landing on an unrelated entity.

`ui_type(entity, text)` requires Bevy's `EditableText` and queues `TextEdit::Insert`, allowing Bevy's normal text-edit system to apply Unicode-aware edits.

## Camera

`camera_set_transform`, `camera_look_at`, and `camera_frame_entity` mutate the active camera through the deferred world-mutation path. Framing preserves the current distance from camera to target and points the camera at the target.

## Runtime requirements

Pointer interaction is operational only when Bevy picking input is installed/enabled and a primary window exists. The live `capabilities` response reports runtime availability separately from MCP permission allowance.
