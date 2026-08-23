use bevy::prelude::*;
use bevy_mcp_core::command::{McpCommand, McpResult};
use bevy_mcp_core::entity_handle::EntityHandle;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::{BevyMcpPlugin, McpPermissions};
use serde_json::Value;

fn success_for(results: &McpResultQueue, request_id: u64) -> Value {
    let response = results
        .drain()
        .into_iter()
        .find(|response| response.request_id == request_id)
        .expect("expected MCP response");
    match response.result {
        McpResult::Success(value) => value,
        McpResult::Error { code, message } => panic!("unexpected MCP error {code}: {message}"),
    }
}

fn handle(entity: Entity) -> EntityHandle {
    EntityHandle::from_uri(&format!(
        "entity://default/main/{}/{}",
        entity.index().index(),
        entity.generation()
    ))
    .unwrap()
}

#[test]
fn ui_type_queues_native_editable_text_edit() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    let entity = app
        .world_mut()
        .spawn(bevy::text::EditableText::new("hello"))
        .id();
    ingress.push(
        1,
        McpCommand::UiType {
            entity: handle(entity),
            text: " world".into(),
        },
    );
    app.update();
    let result = success_for(&results, 1);
    assert_eq!(result["status"], "queued");
    let editable = app.world().get::<bevy::text::EditableText>(entity).unwrap();
    assert!(
        editable.pending_edits.iter().any(|edit| {
            matches!(edit, bevy::text::TextEdit::Insert(value) if value.as_ref() == " world")
        }),
        "expected queued native TextEdit insert of requested text, got {:?}",
        editable.pending_edits
    );
}

#[test]
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
                ..default()
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
            ..default()
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
    assert!(
        (distance - 7.0).abs() < 0.01,
        "distance={distance}, frame={frame}"
    );
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
    assert!(
        (scale - 0.625).abs() < 0.001,
        "scale={scale}, frame={frame}"
    );
    let projection = app.world().get::<bevy::camera::Projection>(camera).unwrap();
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

    let rig = app
        .world_mut()
        .spawn(Transform::from_xyz(10.0, 0.0, 0.0))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            bevy::camera::Projection::Perspective(bevy::camera::PerspectiveProjection {
                fov: std::f32::consts::FRAC_PI_2,
                aspect_ratio: 1.0,
                near: 0.1,
                far: 1000.0,
                ..default()
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
    assert!(
        (local.translation.x + 10.0).abs() < 0.001,
        "local={local:?}"
    );
}

#[test]
fn pointer_capabilities_are_truthful_without_picking_or_window() {
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(
        BevyMcpPlugin::new()
            .with_queues(ingress.clone(), results.clone())
            .with_permissions(McpPermissions::full()),
    );

    ingress.push(5, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 5);
    let pointer = &capabilities["input"]["mouse_move"];
    assert_eq!(pointer["implemented"], true, "capabilities={capabilities}");
    assert_eq!(pointer["available"], false, "capabilities={capabilities}");
    assert_eq!(pointer["allowed"], true, "capabilities={capabilities}");
    assert_eq!(pointer["operational"], false, "capabilities={capabilities}");

    let ui_click = &capabilities["ui"]["click"];
    assert_eq!(ui_click["implemented"], true, "capabilities={capabilities}");
    assert_eq!(ui_click["available"], false, "capabilities={capabilities}");
    assert_eq!(ui_click["allowed"], true, "capabilities={capabilities}");
    assert_eq!(
        ui_click["operational"], false,
        "capabilities={capabilities}"
    );

    assert_eq!(
        capabilities["interaction"]["pointer_click"]["implemented"],
        true
    );
    assert_eq!(
        capabilities["interaction"]["pointer_click"]["available"],
        false
    );
}

#[test]
fn configured_picking_pointer_is_available_before_first_move_and_can_move() {
    use bevy::picking::{InteractionPlugin, PickingPlugin};
    use bevy::window::{PrimaryWindow, Window};

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();
    let mut app = App::new();
    // The MCP supplies its own PointerInput stream. Install only Bevy's core picking state
    // and interaction event processing, deliberately excluding PointerInputPlugin's OS readers.
    app.add_plugins(MinimalPlugins)
        .add_plugins((PickingPlugin, InteractionPlugin))
        .add_plugins(
            BevyMcpPlugin::new()
                .with_queues(ingress.clone(), results.clone())
                .with_permissions(McpPermissions::full()),
        );
    app.world_mut().spawn((Window::default(), PrimaryWindow));

    // Allow Startup to register the MCP custom pointer before querying live capabilities.
    app.update();
    results.drain();

    ingress.push(20, McpCommand::Capabilities);
    app.update();
    let capabilities = success_for(&results, 20);
    let pointer = &capabilities["input"]["mouse_move"];
    assert_eq!(pointer["implemented"], true, "capabilities={capabilities}");
    assert_eq!(pointer["available"], true, "capabilities={capabilities}");
    assert_eq!(pointer["allowed"], true, "capabilities={capabilities}");
    assert_eq!(pointer["operational"], true, "capabilities={capabilities}");
    assert_eq!(capabilities["interaction"]["pick_at"]["operational"], true);

    ingress.push(21, McpCommand::InputMouseMove { x: 32.0, y: 48.0 });
    app.update();
    let moved = success_for(&results, 21);
    assert_eq!(moved["pointer"], "mcp");
    assert_eq!(moved["position"]["x"], 32.0);
    assert_eq!(moved["position"]["y"], 48.0);
}
