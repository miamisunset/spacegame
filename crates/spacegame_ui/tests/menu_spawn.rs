use bevy::camera::CameraProjection;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use spacegame_sim::SimPlugin;
use spacegame_sim::{Asteroid, ContextMenuState, OrderQueue};
use spacegame_ui::{ContextMenuRoot, UiPlugin};

#[test]
fn right_click_over_asteroid_spawns_menu_and_blocks_orbit() {
    let mut app = App::new();
    // Minimal + UI without GPU render – need Asset + Scene for bsn! spawn_scene
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        SimPlugin,
        UiPlugin,
    ));
    app.add_message::<bevy::input::mouse::MouseButtonInput>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    // Also need MouseMotion and MouseWheel messages for camera etc., but not for this test
    // Ensure Time resource exists (MinimalPlugins provides it)
    // Need window with cursor at center
    let window_entity = app
        .world_mut()
        .spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(800, 600),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    // Set cursor position to center (400,300) via method
    {
        let mut win = app.world_mut().get_mut::<Window>(window_entity).unwrap();
        win.set_cursor_position(Some(Vec2::new(400.0, 300.0)));
    }
    // Need a camera at strategic position looking at origin
    // Camera at (0,1200,3300) looking at 0,0,0 as in fixed render (pitch 0.35)
    let cam_transform =
        Transform::from_translation(Vec3::new(0.0, 1200.0, 3300.0)).looking_at(Vec3::ZERO, Vec3::Y);
    let cam_entity = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                order: 0,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection::default()),
            cam_transform,
            GlobalTransform::from(cam_transform),
        ))
        .id();
    // Manually init Camera.computed so viewport_to_world works headless (no CameraPlugin update)
    {
        let mut cam = app.world_mut().get_mut::<Camera>(cam_entity).unwrap();
        let proj = PerspectiveProjection::default();
        cam.computed.clip_from_view = proj.get_clip_from_view();
        cam.computed.target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::new(800, 600),
            scale_factor: 1.0,
        });
        cam.viewport = Some(bevy::camera::Viewport {
            physical_position: UVec2::ZERO,
            physical_size: UVec2::new(800, 600),
            ..default()
        });
    }
    // Spawn ship with OrderQueue
    app.world_mut().spawn(OrderQueue::new());
    // Spawn asteroid at origin (0,0,0) - should be under center cursor
    let asteroid_pos = Vec3::ZERO;
    app.world_mut().spawn((
        Asteroid::new(800, 1200),
        Transform::from_translation(asteroid_pos),
    ));

    app.update();

    // Ensure no menu initially
    assert_eq!(
        app.world().resource::<ContextMenuState>().clone(),
        ContextMenuState::Hidden
    );
    let count_before = app
        .world_mut()
        .query::<&ContextMenuRoot>()
        .iter(app.world())
        .count();
    assert_eq!(count_before, 0, "no menu before click");

    // Send right click press
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Right,
            state: bevy::input::ButtonState::Pressed,
            window: window_entity,
        });
    // Tick PreUpdate + Update
    app.update();
    // After one tick, menu should be spawned (PreUpdate detection)
    app.update();

    let state = app.world().resource::<ContextMenuState>().clone();
    eprintln!("state after right-click: {:?}", state);
    assert!(
        matches!(state, ContextMenuState::Shown(_)),
        "menu should be Shown after hit, got {:?}",
        state
    );

    let menu_count = app
        .world_mut()
        .query::<&ContextMenuRoot>()
        .iter(app.world())
        .count();
    eprintln!("menu_count {}", menu_count);
    assert_eq!(
        menu_count, 1,
        "menu entity with ContextMenuRoot should exist"
    );

    // Now send right click on empty space (move cursor to corner far from asteroid)
    // Update cursor to (10,10) which should miss asteroid at origin when looking from above
    {
        let mut win = app.world_mut().get_mut::<Window>(window_entity).unwrap();
        win.set_cursor_position(Some(Vec2::new(10.0, 10.0)));
    }
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Right,
            state: bevy::input::ButtonState::Pressed,
            window: window_entity,
        });
    app.update();
    app.update();
    let state2 = app.world().resource::<ContextMenuState>().clone();
    eprintln!("state after miss: {:?}", state2);
    assert_eq!(
        state2,
        ContextMenuState::Hidden,
        "miss should hide menu to re-enable orbit"
    );
    let menu_count2 = app
        .world_mut()
        .query::<&ContextMenuRoot>()
        .iter(app.world())
        .count();
    assert_eq!(menu_count2, 0, "menu should be despawned after miss");
}
