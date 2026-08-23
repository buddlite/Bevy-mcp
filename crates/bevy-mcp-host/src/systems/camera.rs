use super::*;

pub(crate) fn active_camera_entity(world: &World) -> Option<Entity> {
    world
        .iter_entities()
        .find(|entity| {
            entity
                .get::<bevy::prelude::Camera>()
                .is_some_and(|camera| camera.is_active)
                && entity.get::<Transform>().is_some()
        })
        .map(|entity| entity.id())
        .or_else(|| {
            world
                .iter_entities()
                .find(|entity| {
                    entity.get::<bevy::prelude::Camera>().is_some()
                        && entity.get::<Transform>().is_some()
                })
                .map(|entity| entity.id())
        })
}

pub(crate) fn ui_type_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    text: &str,
) -> McpResult {
    let entity = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let Some(mut editable) = world.get_mut::<bevy::text::EditableText>(entity) else {
        return McpResult::error(
            "NOT_EDITABLE_TEXT",
            format!("Entity {handle} does not have an EditableText component"),
        );
    };
    editable.queue_edit(bevy::text::TextEdit::Insert(text.into()));
    McpResult::success(json!({
        "entity": entity_to_uri(entity),
        "status": "queued",
        "text": text,
    }))
}

pub(crate) fn target_position(world: &World, entity: Entity) -> Option<Vec3> {
    current_global_transform(world, entity).map(|transform| transform.translation())
}

pub(crate) fn current_global_transform(world: &World, entity: Entity) -> Option<GlobalTransform> {
    fn resolve(world: &World, entity: Entity, depth: u32) -> Option<GlobalTransform> {
        if depth > 128 {
            return None;
        }
        if let Some(local) = world.get::<Transform>(entity).copied() {
            if let Some(parent) = world.get::<bevy::ecs::hierarchy::ChildOf>(entity) {
                let parent_global = resolve(world, parent.parent(), depth + 1)?;
                Some(parent_global.mul_transform(local))
            } else {
                Some(GlobalTransform::from(local))
            }
        } else {
            world.get::<GlobalTransform>(entity).copied()
        }
    }

    resolve(world, entity, 0)
}

pub(crate) fn set_world_transform(
    world: &mut World,
    entity: Entity,
    desired_world: Transform,
) -> Result<(), McpResult> {
    let parent = world
        .get::<bevy::ecs::hierarchy::ChildOf>(entity)
        .map(|parent| parent.parent());
    let local = if let Some(parent) = parent {
        let Some(parent_global) = current_global_transform(world, parent) else {
            return Err(McpResult::error(
                "PARENT_TRANSFORM_NOT_READY",
                "Camera parent transform could not be resolved",
            ));
        };
        GlobalTransform::from(desired_world).reparented_to(&parent_global)
    } else {
        desired_world
    };

    let Some(mut transform) = world.get_mut::<Transform>(entity) else {
        return Err(McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera has no Transform",
        ));
    };
    *transform = local;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AggregateBounds {
    min: Vec3,
    max: Vec3,
    bounded_entities: usize,
}

impl AggregateBounds {
    fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    fn corners(self) -> [Vec3; 8] {
        let min = self.min;
        let max = self.max;
        [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(max.x, max.y, max.z),
        ]
    }
}

pub(crate) fn aggregate_world_bounds(
    world: &World,
    root: Entity,
) -> Result<AggregateBounds, McpResult> {
    use bevy::camera::primitives::Aabb;

    let mut stack = vec![root];
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut bounded_entities = 0usize;

    while let Some(entity) = stack.pop() {
        if let Some(children) = world.get::<bevy::ecs::hierarchy::Children>(entity) {
            stack.extend(children.iter());
        }

        let Some(aabb) = world.get::<Aabb>(entity) else {
            continue;
        };
        let Some(global) = current_global_transform(world, entity) else {
            return Err(McpResult::error(
                "BOUNDS_NOT_READY",
                format!(
                    "Entity {} has an Aabb but its world transform is not available",
                    entity_to_uri(entity)
                ),
            ));
        };

        let center: Vec3 = aabb.center.into();
        let half_extents: Vec3 = aabb.half_extents.into();
        for x in [-half_extents.x, half_extents.x] {
            for y in [-half_extents.y, half_extents.y] {
                for z in [-half_extents.z, half_extents.z] {
                    let point = global.transform_point(center + Vec3::new(x, y, z));
                    if !point.is_finite() {
                        return Err(McpResult::error(
                            "INVALID_BOUNDS",
                            "Encountered a non-finite transformed Aabb corner",
                        ));
                    }
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                }
            }
        }
        bounded_entities += 1;
    }

    if bounded_entities == 0 {
        return Err(McpResult::error(
            "NO_BOUNDS",
            "Target entity and its descendants do not contain any Aabb components",
        ));
    }

    Ok(AggregateBounds {
        min: minimum,
        max: maximum,
        bounded_entities,
    })
}

pub(crate) fn framing_basis(
    camera_world: GlobalTransform,
    center: Vec3,
) -> (Vec3, Vec3, Vec3, f32) {
    let camera_position = camera_world.translation();
    let offset = camera_position - center;
    let current_distance = offset.length();
    let direction = offset.try_normalize().unwrap_or_else(|| {
        (camera_world.rotation() * Vec3::Z)
            .try_normalize()
            .unwrap_or(Vec3::Z)
    });
    let forward = -direction;
    let preferred_up = camera_world.rotation() * Vec3::Y;
    let fallback_up = if forward.dot(Vec3::Y).abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = forward
        .cross(preferred_up)
        .try_normalize()
        .or_else(|| forward.cross(fallback_up).try_normalize())
        .unwrap_or(Vec3::X);
    let up = right.cross(forward).try_normalize().unwrap_or(fallback_up);
    (direction, right, up, current_distance)
}

pub(crate) fn camera_set_transform_apply(world: &mut World, x: f64, y: f64, z: f64) -> McpResult {
    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(global) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    let mut desired = global.compute_transform();
    desired.translation = Vec3::new(x as f32, y as f32, z as f32);
    if let Err(error) = set_world_transform(world, camera, desired) {
        return error;
    }
    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "position": {"x": x, "y": y, "z": z},
        "space": "world",
    }))
}

pub(crate) fn camera_look_at_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
) -> McpResult {
    let target = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let Some(target_position) = target_position(world, target) else {
        return McpResult::error(
            "NO_TRANSFORM",
            format!("Entity {handle} has no resolvable world transform"),
        );
    };
    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(global) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    if global.translation().distance_squared(target_position) <= f32::EPSILON {
        return McpResult::error(
            "CAMERA_AT_TARGET",
            "Camera and target occupy the same position",
        );
    }
    let mut desired = global.compute_transform();
    desired.look_at(target_position, Vec3::Y);
    if let Err(error) = set_world_transform(world, camera, desired) {
        return error;
    }
    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "target": entity_to_uri(target),
    }))
}

pub(crate) fn camera_frame_entity_apply(
    world: &mut World,
    handle: &bevy_mcp_core::entity_handle::EntityHandle,
    margin: f64,
) -> McpResult {
    if !margin.is_finite() || !(0.0..=2.0).contains(&margin) {
        return McpResult::error(
            "INVALID_MARGIN",
            "margin must be a finite value between 0.0 and 2.0",
        );
    }

    let target = match resolve_entity(world, handle) {
        Some(entity) => entity,
        None => return McpResult::error("ENTITY_NOT_FOUND", format!("Entity {handle} not found")),
    };
    let bounds = match aggregate_world_bounds(world, target) {
        Ok(bounds) => bounds,
        Err(error) => return error,
    };
    let center = bounds.center();

    let Some(camera) = active_camera_entity(world) else {
        return McpResult::error("NO_CAMERA", "No camera with a Transform was found");
    };
    let Some(camera_world) = current_global_transform(world, camera) else {
        return McpResult::error(
            "NO_CAMERA_TRANSFORM",
            "Active camera world transform could not be resolved",
        );
    };
    let Some(projection) = world.get::<bevy::camera::Projection>(camera) else {
        return McpResult::error(
            "CAMERA_PROJECTION_NOT_AVAILABLE",
            "Active camera does not have a Projection component",
        );
    };

    #[derive(Clone, Copy)]
    enum ProjectionData {
        Perspective {
            fov: f32,
            aspect_ratio: f32,
            near: f32,
            far: f32,
        },
        Orthographic {
            near: f32,
            far: f32,
            scale: f32,
            area: Rect,
        },
        Custom,
    }

    let projection = match projection {
        bevy::camera::Projection::Perspective(value) => ProjectionData::Perspective {
            fov: value.fov,
            aspect_ratio: value.aspect_ratio,
            near: value.near,
            far: value.far,
        },
        bevy::camera::Projection::Orthographic(value) => ProjectionData::Orthographic {
            near: value.near,
            far: value.far,
            scale: value.scale,
            area: value.area,
        },
        bevy::camera::Projection::Custom(_) => ProjectionData::Custom,
    };

    if matches!(projection, ProjectionData::Custom) {
        return McpResult::error(
            "UNSUPPORTED_PROJECTION",
            "camera_frame_entity does not support custom camera projections",
        );
    }

    let (direction, right, up, current_distance) = framing_basis(camera_world, center);
    let padding = 1.0 + margin as f32;
    let corners = bounds.corners();

    let response = match projection {
        ProjectionData::Perspective {
            fov,
            aspect_ratio,
            near,
            far,
        } => {
            if !fov.is_finite()
                || fov <= 0.0
                || fov >= std::f32::consts::PI
                || !aspect_ratio.is_finite()
                || aspect_ratio <= f32::EPSILON
                || !near.is_finite()
                || near < 0.0
                || !far.is_finite()
                || far <= near
            {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Perspective projection has invalid FOV, aspect ratio, or clip planes",
                );
            }
            let tan_vertical = (fov * 0.5).tan();
            let tan_horizontal = tan_vertical * aspect_ratio;
            if tan_vertical <= f32::EPSILON || tan_horizontal <= f32::EPSILON {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Perspective projection produces a degenerate field of view",
                );
            }

            let mut distance = near.max(0.001);
            for corner in corners {
                let relative = corner - center;
                let z_offset = relative.dot(direction);
                distance =
                    distance.max(relative.dot(right).abs() * padding / tan_horizontal + z_offset);
                distance = distance.max(relative.dot(up).abs() * padding / tan_vertical + z_offset);
                distance = distance.max(near + z_offset + 0.001);
            }
            let farthest_depth = corners
                .iter()
                .map(|corner| distance - (*corner - center).dot(direction))
                .fold(0.0_f32, f32::max);
            if farthest_depth > far {
                return McpResult::error(
                    "BOUNDS_OUTSIDE_CLIP_RANGE",
                    format!(
                        "Framed bounds require depth {farthest_depth:.3}, beyond camera far plane {far:.3}"
                    ),
                );
            }

            let mut desired = camera_world.compute_transform();
            desired.translation = center + direction * distance;
            desired.look_at(center, up);
            if let Err(error) = set_world_transform(world, camera, desired) {
                return error;
            }

            json!({
                "projection": "perspective",
                "distance": distance,
                "fov": fov,
                "aspect_ratio": aspect_ratio,
            })
        }
        ProjectionData::Orthographic {
            near,
            far,
            scale,
            area,
        } => {
            let area_size = area.max - area.min;
            if !near.is_finite()
                || !far.is_finite()
                || far <= near
                || !scale.is_finite()
                || scale <= 0.0
                || !area_size.is_finite()
                || area_size.x.abs() <= f32::EPSILON
                || area_size.y.abs() <= f32::EPSILON
            {
                return McpResult::error(
                    "CAMERA_PROJECTION_NOT_READY",
                    "Orthographic projection area/scale or clip planes are not ready for framing",
                );
            }

            let mut extent_x = 0.0_f32;
            let mut extent_y = 0.0_f32;
            let mut max_z = f32::NEG_INFINITY;
            let mut min_z = f32::INFINITY;
            for corner in corners {
                let relative = corner - center;
                extent_x = extent_x.max(relative.dot(right).abs());
                extent_y = extent_y.max(relative.dot(up).abs());
                let z = relative.dot(direction);
                min_z = min_z.min(z);
                max_z = max_z.max(z);
            }

            let required_width = (extent_x * 2.0 * padding).max(0.001);
            let required_height = (extent_y * 2.0 * padding).max(0.001);
            let ratio = (required_width / area_size.x.abs())
                .max(required_height / area_size.y.abs())
                .max(0.000001);
            let new_scale = scale * ratio;
            if !new_scale.is_finite() || new_scale <= 0.0 {
                return McpResult::error(
                    "INVALID_PROJECTION",
                    "Calculated orthographic scale is invalid",
                );
            }

            let minimum_distance = near + max_z + 0.001;
            let maximum_distance = far + min_z - 0.001;
            if minimum_distance > maximum_distance {
                return McpResult::error(
                    "BOUNDS_OUTSIDE_CLIP_RANGE",
                    "Aggregate bounds are deeper than the orthographic camera clip range",
                );
            }
            let distance = current_distance
                .max(0.001)
                .clamp(minimum_distance, maximum_distance);
            let scaled_area_center = (area.min + area.max) * 0.5 * ratio;

            let mut desired = camera_world.compute_transform();
            desired.translation = center - right * scaled_area_center.x - up * scaled_area_center.y
                + direction * distance;
            desired.look_at(
                center - right * scaled_area_center.x - up * scaled_area_center.y,
                up,
            );
            if let Err(error) = set_world_transform(world, camera, desired) {
                return error;
            }

            let Some(mut projection) = world.get_mut::<bevy::camera::Projection>(camera) else {
                return McpResult::error(
                    "CAMERA_PROJECTION_NOT_AVAILABLE",
                    "Active camera Projection disappeared during framing",
                );
            };
            let bevy::camera::Projection::Orthographic(value) = &mut *projection else {
                return McpResult::error(
                    "CAMERA_PROJECTION_CHANGED",
                    "Active camera projection changed during framing",
                );
            };
            value.scale = new_scale;

            json!({
                "projection": "orthographic",
                "distance": distance,
                "scale": new_scale,
                "previous_scale": scale,
            })
        }
        ProjectionData::Custom => unreachable!(),
    };

    McpResult::success(json!({
        "camera": entity_to_uri(camera),
        "target": entity_to_uri(target),
        "margin": margin,
        "bounded_entities": bounds.bounded_entities,
        "bounds": {
            "min": {"x": bounds.min.x, "y": bounds.min.y, "z": bounds.min.z},
            "max": {"x": bounds.max.x, "y": bounds.max.y, "z": bounds.max.z},
            "center": {"x": center.x, "y": center.y, "z": center.z},
        },
        "framing": response,
    }))
}

pub(crate) fn capture_game(world: &World) -> McpResult {
    if world
        .get_resource::<bevy::render::renderer::RenderDevice>()
        .is_none()
    {
        return McpResult::error(
            "RENDER_NOT_AVAILABLE",
            "Screenshot capture requires RenderPlugin. Add DefaultPlugins to your app.",
        );
    }
    McpResult::error("NOT_IMPLEMENTED", "Screenshot capture is not implemented")
}

pub(crate) fn camera_inspect(world: &World) -> McpResult {
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        if let Some(camera) = world.get::<bevy::prelude::Camera>(entity) {
            let mut info = json!({
                "handle": entity_to_uri(entity),
                "id": entity.index().index(),
                "is_active": camera.is_active,
            });
            if let Some(transform) = world.get::<Transform>(entity) {
                info["position"] = json!({
                    "x": transform.translation.x,
                    "y": transform.translation.y,
                    "z": transform.translation.z,
                });
            }
            return McpResult::success(info);
        }
    }
    McpResult::error("NO_CAMERA", "No camera found in the scene")
}
