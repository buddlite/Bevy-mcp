from pathlib import Path
import re


def read(path: str) -> str:
    return Path(path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    Path(path).write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected exactly one literal match, found {count}")
    write(path, text.replace(old, new, 1))


def sub_once(path: str, pattern: str, replacement: str) -> None:
    text = read(path)
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"{path}: expected exactly one regex match, found {count}")
    write(path, updated)


CORE = "crates/bevy-mcp-core/src/command.rs"
DEFERRED = "crates/bevy-mcp-host/src/deferred.rs"
SYSTEMS = "crates/bevy-mcp-host/src/systems.rs"
TOOLS = "crates/bevy-mcp-server/src/tools.rs"
TESTS = "crates/bevy-mcp-host/tests/interaction.rs"

replace_once(
    CORE,
    """    CameraFrameEntity {\n        entity: EntityHandle,\n    },""",
    """    CameraFrameEntity {\n        entity: EntityHandle,\n        margin: f64,\n    },""",
)

replace_once(
    DEFERRED,
    """    CameraFrameEntity {\n        entity: EntityHandle,\n        result_id: u64,\n    },""",
    """    CameraFrameEntity {\n        entity: EntityHandle,\n        margin: f64,\n        result_id: u64,\n    },""",
)

replace_once(
    TOOLS,
    """pub struct CameraFrameParams {\n    #[schemars(description = \"Entity handle URI to frame\")]\n    pub entity: String,\n}""",
    """pub struct CameraFrameParams {\n    #[schemars(description = \"Entity handle URI to frame\")]\n    pub entity: String,\n    #[schemars(description = \"Fractional framing margin around the aggregate bounds (default 0.15)\")]\n    pub margin: Option<f64>,\n}""",
)

replace_once(
    TOOLS,
    """    #[tool(\n        description = \"Frame an entity with the active camera while preserving the current camera-target distance.\"\n    )]\n    async fn camera_frame_entity(\n        &self,\n        Parameters(params): Parameters<CameraFrameParams>,\n    ) -> String {\n        let entity = match parse_entity_handle(&params.entity) {\n            Ok(handle) => handle,\n            Err(message) => return error(\"INVALID_HANDLE\", message),\n        };\n        self.state\n            .call(McpCommand::CameraFrameEntity { entity })\n            .await\n    }""",
    """    #[tool(\n        description = \"Fit an entity and its descendant render bounds in the active perspective or orthographic camera.\"\n    )]\n    async fn camera_frame_entity(\n        &self,\n        Parameters(params): Parameters<CameraFrameParams>,\n    ) -> String {\n        let entity = match parse_entity_handle(&params.entity) {\n            Ok(handle) => handle,\n            Err(message) => return error(\"INVALID_HANDLE\", message),\n        };\n        let margin = params.margin.unwrap_or(0.15);\n        if !margin.is_finite() || !(0.0..=2.0).contains(&margin) {\n            return error(\n                \"INVALID_MARGIN\",\n                \"margin must be a finite value between 0.0 and 2.0\",\n            );\n        }\n        self.state\n            .call(McpCommand::CameraFrameEntity { entity, margin })\n            .await\n    }""",
)

replace_once(
    SYSTEMS,
    """            McpCommand::CameraFrameEntity { entity } => {\n                world.resource_mut::<DeferredMcpCommands>().pending.push(\n                    DeferredCommand::CameraFrameEntity {\n                        entity: entity.clone(),\n                        result_id: entry.request_id,\n                    },\n                );\n            }""",
    """            McpCommand::CameraFrameEntity { entity, margin } => {\n                world.resource_mut::<DeferredMcpCommands>().pending.push(\n                    DeferredCommand::CameraFrameEntity {\n                        entity: entity.clone(),\n                        margin: *margin,\n                        result_id: entry.request_id,\n                    },\n                );\n            }""",
)

replace_once(
    SYSTEMS,
    """            DeferredCommand::CameraFrameEntity { entity, result_id } => {\n                let result = camera_frame_entity_apply(world, &entity);\n                world.resource::<McpResultQueue>().push(McpResponse {\n                    request_id: result_id,\n                    result,\n                });\n            }""",
    """            DeferredCommand::CameraFrameEntity {\n                entity,\n                margin,\n                result_id,\n            } => {\n                let result = camera_frame_entity_apply(world, &entity, margin);\n                world.resource::<McpResultQueue>().push(McpResponse {\n                    request_id: result_id,\n                    result,\n                });\n            }""",
)

replace_once(
    SYSTEMS,
    """        McpCommand::CameraFrameEntity { entity } => camera_frame_entity(world, entity),""",
    """        McpCommand::CameraFrameEntity { .. } => {\n            McpResult::error(\"INTERNAL\", \"Camera framing should be deferred\")\n        },""",
)

replace_once(
    SYSTEMS,
    """    let camera_available = active_camera_entity(world).is_some();""",
    """    let camera_available = active_camera_entity(world).is_some();\n    let camera_frame_available = active_camera_entity(world).is_some_and(|camera| {\n        matches!(\n            world.get::<bevy::camera::Projection>(camera),\n            Some(\n                bevy::camera::Projection::Perspective(_)\n                    | bevy::camera::Projection::Orthographic(_)\n            )\n        )\n    });""",
)

replace_once(
    SYSTEMS,
    """            \"frame_entity\": capability(true, camera_available, can_runtime),""",
    """            \"frame_entity\": capability(true, camera_frame_available, can_runtime),""",
)

CAMERA_IMPL = r'''fn target_position(world: &World, entity: Entity) -> Option<Vec3> {
    current_global_transform(world, entity).map(|transform| transform.translation())
}

fn current_global_transform(world: &World, entity: Entity) -> Option<GlobalTransform> {
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

fn set_world_transform(
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

fn aggregate_world_bounds(world: &World, root: Entity) -> Result<AggregateBounds, McpResult> {
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

fn framing_basis(camera_world: GlobalTransform, center: Vec3) -> (Vec3, Vec3, Vec3, f32) {
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
    let up = right
        .cross(forward)
        .try_normalize()
        .unwrap_or(fallback_up);
    (direction, right, up, current_distance)
}

fn camera_set_transform_apply(world: &mut World, x: f64, y: f64, z: f64) -> McpResult {
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

fn camera_look_at_apply(
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

fn camera_frame_entity_apply(
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
                distance = distance.max(relative.dot(right).abs() * padding / tan_horizontal + z_offset);
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
            let distance = current_distance.max(0.001).clamp(minimum_distance, maximum_distance);
            let scaled_area_center = (area.min + area.max) * 0.5 * ratio;

            let mut desired = camera_world.compute_transform();
            desired.translation = center
                - right * scaled_area_center.x
                - up * scaled_area_center.y
                + direction * distance;
            desired.look_at(center - right * scaled_area_center.x - up * scaled_area_center.y, up);
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
'''

sub_once(
    SYSTEMS,
    r"fn target_position\(world: &World, entity: Entity\) -> Option<Vec3> \{.*?\n\}\n\nfn playtest_run",
    CAMERA_IMPL + "\nfn playtest_run",
)

sub_once(
    SYSTEMS,
    r"\nfn camera_frame_entity\(\n    world: &World,.*?\n\}\n\nfn camera_inspect",
    "\nfn camera_inspect",
)

NEW_CAMERA_TEST = r'''#[test]
fn camera_controls_mutate_active_camera() {
    use bevy::camera::primitives::Aabb;

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            bevy::camera::Projection::Perspective(bevy::camera::PerspectiveProjection {
                fov: std::f32::consts::FRAC_PI_2,
                aspect_ratio: 1.0,
                near: 0.1,
                far: 1000.0,
            }),
            Transform::from_xyz(0.0, 0.0, 10.0),
        ))
        .id();
    let target = app
        .world_mut()
        .spawn((
            Transform::from_xyz(1.0, 2.0, 3.0),
            Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
        ))
        .id();

    ingress.push(
        2,
        McpCommand::CameraSetTransform {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        },
    );
    app.update();
    let _ = success_for(&results, 2);
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation,
        Vec3::new(4.0, 5.0, 6.0)
    );

    ingress.push(
        3,
        McpCommand::CameraLookAt {
            entity: handle(target),
        },
    );
    app.update();
    let look = success_for(&results, 3);
    assert!(look["camera"].is_string());

    ingress.push(
        4,
        McpCommand::CameraFrameEntity {
            entity: handle(target),
            margin: 0.15,
        },
    );
    app.update();
    let frame = success_for(&results, 4);
    assert_eq!(frame["framing"]["projection"], "perspective");
    assert!(frame["framing"]["distance"].as_f64().unwrap() > 0.0);
}

#[test]
fn camera_frame_aggregates_descendant_bounds_for_perspective_camera() {
    use bevy::camera::primitives::Aabb;
    use bevy::ecs::hierarchy::ChildOf;

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    app.world_mut().spawn((
        Camera::default(),
        bevy::camera::Projection::Perspective(bevy::camera::PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_2,
            aspect_ratio: 1.0,
            near: 0.1,
            far: 1000.0,
        }),
        Transform::from_xyz(0.0, 0.0, 20.0),
    ));
    let root = app.world_mut().spawn(Transform::default()).id();
    let unit_bounds = Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
    app.world_mut().spawn((
        Transform::from_xyz(-4.0, 0.0, 0.0),
        unit_bounds,
        ChildOf(root),
    ));
    app.world_mut().spawn((
        Transform::from_xyz(4.0, 0.0, 0.0),
        unit_bounds,
        ChildOf(root),
    ));

    ingress.push(
        30,
        McpCommand::CameraFrameEntity {
            entity: handle(root),
            margin: 0.2,
        },
    );
    app.update();
    let frame = success_for(&results, 30);
    assert_eq!(frame["bounded_entities"], 2);
    assert_eq!(frame["bounds"]["min"]["x"], -5.0);
    assert_eq!(frame["bounds"]["max"]["x"], 5.0);
    let distance = frame["framing"]["distance"].as_f64().unwrap();
    assert!((distance - 7.0).abs() < 0.01, "distance={distance}, frame={frame}");
}

#[test]
fn camera_frame_updates_orthographic_scale() {
    use bevy::camera::primitives::Aabb;

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    let mut orthographic = bevy::camera::OrthographicProjection::default_3d();
    orthographic.scale = 1.0;
    orthographic.area = Rect::new(-4.0, -3.0, 4.0, 3.0);
    orthographic.near = 0.0;
    orthographic.far = 100.0;
    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            bevy::camera::Projection::Orthographic(orthographic),
            Transform::from_xyz(0.0, 0.0, 10.0),
        ))
        .id();
    let target = app
        .world_mut()
        .spawn((
            Transform::default(),
            Aabb::from_min_max(Vec3::new(-2.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)),
        ))
        .id();

    ingress.push(
        31,
        McpCommand::CameraFrameEntity {
            entity: handle(target),
            margin: 0.25,
        },
    );
    app.update();
    let frame = success_for(&results, 31);
    assert_eq!(frame["framing"]["projection"], "orthographic");
    let scale = frame["framing"]["scale"].as_f64().unwrap();
    assert!((scale - 0.625).abs() < 0.001, "scale={scale}, frame={frame}");
    let projection = app
        .world()
        .get::<bevy::camera::Projection>(camera)
        .unwrap();
    let bevy::camera::Projection::Orthographic(value) = projection else {
        panic!("expected orthographic projection");
    };
    assert!((value.scale - 0.625).abs() < 0.001);
}

#[test]
fn camera_frame_preserves_world_space_for_parented_camera() {
    use bevy::camera::primitives::Aabb;
    use bevy::ecs::hierarchy::ChildOf;

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    let rig = app.world_mut().spawn(Transform::from_xyz(10.0, 0.0, 0.0)).id();
    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            bevy::camera::Projection::Perspective(bevy::camera::PerspectiveProjection {
                fov: std::f32::consts::FRAC_PI_2,
                aspect_ratio: 1.0,
                near: 0.1,
                far: 1000.0,
            }),
            Transform::from_xyz(-10.0, 0.0, 10.0),
            ChildOf(rig),
        ))
        .id();
    let target = app
        .world_mut()
        .spawn((
            Transform::default(),
            Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
        ))
        .id();

    ingress.push(
        32,
        McpCommand::CameraFrameEntity {
            entity: handle(target),
            margin: 0.15,
        },
    );
    app.update();
    let frame = success_for(&results, 32);
    assert_eq!(frame["framing"]["projection"], "perspective");
    let local = app.world().get::<Transform>(camera).unwrap();
    assert!((local.translation.x + 10.0).abs() < 0.001, "local={local:?}");
}
'''

sub_once(
    TESTS,
    r"#\[test\]\nfn camera_controls_mutate_active_camera\(\) \{.*?\n\}\n\n#\[test\]\nfn pointer_capabilities",
    NEW_CAMERA_TEST + "\n#[test]\nfn pointer_capabilities",
)

# Ensure no stale command constructors remain in the files this feature owns.
for path in (SYSTEMS, TOOLS, TESTS):
    text = read(path)
    if "McpCommand::CameraFrameEntity { entity }" in text:
        raise RuntimeError(f"{path}: stale CameraFrameEntity constructor remains")

print("bounds-aware camera framing source transformation complete")
