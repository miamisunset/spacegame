//! Bevy render plugins, meshes, materials, lighting.
//!
//! Draws ship as cube and asteroids as icospheres from `Transform` sync;
//! provides detached strategic orbit/pan/zoom camera where WASD moves
//! camera target, not ship. All meshes are placeholder per slice 1.
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::picking::prelude::{MeshPickingPlugin, Pickable};
use bevy::prelude::*;

use spacegame_sim::{Asteroid, ShipStats};
use spacegame_ui::ContextState;

/// Marker for ship mesh entities (shares `ShipStats` entity).
#[derive(Component)]
struct ShipMesh;

/// Marker for asteroid mesh entities.
#[derive(Component)]
struct AsteroidMesh;

/// Detached strategic camera — orbit/pan/zoom, WASD moves camera.
///
/// `target` is the look-at point on the XZ plane. Camera `Transform`
/// is derived each frame as `target + rotation * (0,0,distance)`.
#[derive(Component, Debug)]
pub struct StrategicCamera {
    /// Look-at target in world space.
    pub target: Vec3,
    /// Distance from target along camera forward.
    pub distance: f32,
    /// Yaw around world Y (radians).
    pub yaw: f32,
    /// Pitch above horizon (radians), clamped to avoid gimbal.
    pub pitch: f32,
}

impl Default for StrategicCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 2500.0,
            yaw: 0.4,
            pitch: 0.6,
        }
    }
}

/// SystemSet for camera — ordered after [`spacegame_ui::UiSet`] so
/// context-menu visibility is settled before orbit gating (`own-borrow-over-clone`).
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct CameraSet;

/// Bevy render plugin — placeholder cube ship + icosphere asteroids + strategic camera.
///
/// Depends on `DefaultPlugins` for `Assets<Mesh>` / `Assets<StandardMaterial>`.
/// `SimPlugin` should be added alongside so `Transform` updates from
/// `MovementSet` are visible here in `Update`.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin);
        app.add_systems(Startup, setup_scene);
        // Mesh sync runs in Update after FixedUpdate so fresh Transforms are visible.
        // Use Without<Mesh3d> to backfill entities spawned before render init.
        app.add_systems(Update, (sync_ship_mesh, sync_asteroid_mesh));
        app.add_systems(Update, strategic_camera_system.in_set(CameraSet));
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground/grid visual reference — large plane at y=0.
    let ground_mesh = meshes.add(Plane3d::default().mesh().size(20000.0, 20000.0));
    let ground_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.08, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_mat),
        Transform::from_translation(Vec3::ZERO),
        Pickable::IGNORE,
    ));

    // Lighting — directional sun plus ambient.
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.6, 0.9, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        ..default()
    });

    // Strategic camera — detached, looks at origin initially.
    let cam = StrategicCamera::default();
    let offset = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0) * Vec3::Z * cam.distance;
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(cam.target + offset).looking_at(cam.target, Vec3::Y),
        cam,
    ));
}

// own-borrow-over-clone: mesh handles are cloned cheaply (Arc)
#[allow(clippy::type_complexity)]
fn sync_ship_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    query: Query<(Entity, &Transform), (With<ShipStats>, Without<Mesh3d>)>,
) {
    for (entity, _tf) in &query {
        // Placeholder cube ship — 80x20x120 world units, distinct blue.
        let mesh = meshes.add(Cuboid::new(80.0, 20.0, 120.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.6, 0.95),
            emissive: LinearRgba::new(0.02, 0.05, 0.12, 1.0),
            perceptual_roughness: 0.7,
            ..default()
        });
        commands.entity(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            ShipMesh,
            Pickable::default(),
        ));
    }
}

// own-borrow-over-clone: sphere mesh builder
fn sync_asteroid_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    query: Query<(Entity, &Transform, &Asteroid), Without<Mesh3d>>,
) {
    for (entity, _tf, ast) in &query {
        // Radius loosely scaled to max_ore for visual variety, bounded.
        let t = (ast.max_ore as f32 / 1000.0).clamp(0.5, 1.5);
        let radius = 40.0 + t * 30.0;
        // Sphere::new(radius).mesh().ico(3) => ~642 verts cheap LOD.
        let sphere_mesh = Sphere::new(radius).mesh().ico(3).unwrap_or_else(|_| {
            // Fallback to uv sphere if ico fails (num-float-compare: never panic)
            Sphere::new(radius).mesh().uv(16, 12)
        });
        let mesh = meshes.add(sphere_mesh);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.42, 0.22),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.entity(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            AsteroidMesh,
            Pickable::default(),
        ));
    }
}

/// Strategic camera controller — WASD pan, Q/E yaw, R/F pitch, scroll zoom, RMB drag orbit.
///
/// Runs in `Update` only (never `FixedUpdate`) per AGENTS.md. WASD moves
/// `StrategicCamera.target` on the XZ plane relative to camera yaw; ship
/// `Transform` is never mutated here.
///
/// EVE-style RMB: orbit only when hovering empty space (`HoverMap` has no
/// pickable hit) and context menu is not visible. Right-click on a pickable
/// (asteroid/ship) opens the menu instead — see `spacegame_ui::handle_right_click_spawn_menu`.
#[allow(clippy::too_many_arguments)]
fn strategic_camera_system(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut scroll: MessageReader<bevy::input::mouse::MouseWheel>,
    mut mouse_motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut query: Query<(&mut Transform, &mut StrategicCamera)>,
    hover: Res<HoverMap>,
    ctx: Res<ContextState>,
    window_q: Query<Entity, With<Window>>,
) {
    let Ok((mut tf, mut cam)) = query.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let pan_speed = 800.0;
    let yaw_speed = 0.9;
    let pitch_speed = 0.7;
    let zoom_speed = 120.0;

    // Derive forward/right on XZ plane from current yaw (ignore pitch for pan).
    let yaw_rot = Quat::from_rotation_y(cam.yaw);
    let forward = yaw_rot * Vec3::NEG_Z;
    let right = yaw_rot * Vec3::X;
    // Project to XZ plane
    let forward_xz = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let right_xz = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();

    let mut pan = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        pan += forward_xz;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        pan -= forward_xz;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        pan -= right_xz;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        pan += right_xz;
    }
    if pan.length_squared() > f32::EPSILON {
        // num-float-compare: normalize only when length > epsilon
        pan = pan.normalize() * pan_speed * dt;
        cam.target += pan;
    }

    if keyboard.pressed(KeyCode::KeyQ) {
        cam.yaw -= yaw_speed * dt;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        cam.yaw += yaw_speed * dt;
    }
    if keyboard.pressed(KeyCode::KeyR) {
        cam.pitch = (cam.pitch + pitch_speed * dt).clamp(0.1, 1.45);
    }
    if keyboard.pressed(KeyCode::KeyF) {
        cam.pitch = (cam.pitch - pitch_speed * dt).clamp(0.1, 1.45);
    }

    // EVE-style gating: orbit only on empty space and when menu not visible.
    let hovering_pickable = hover
        .get(&PointerId::Mouse)
        .and_then(|map| {
            let window_ent = window_q.single().ok();
            map.iter()
                .find(|(e, _)| Some(**e) != window_ent)
                .map(|(e, _)| e)
        })
        .is_some();
    let should_orbit =
        mouse_button.pressed(MouseButton::Right) && !ctx.visible && !hovering_pickable;

    if should_orbit {
        for ev in mouse_motion.read() {
            cam.yaw -= ev.delta.x * 0.003;
            cam.pitch = (cam.pitch - ev.delta.y * 0.003).clamp(0.1, 1.45);
        }
    } else {
        // consume motion when not orbiting to avoid stale deltas
        for _ in mouse_motion.read() {}
    }

    for ev in scroll.read() {
        cam.distance = (cam.distance - ev.y * zoom_speed).clamp(200.0, 8000.0);
    }

    // Recompute camera transform
    let rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let offset = rotation * Vec3::Z * cam.distance;
    tf.translation = cam.target + offset;
    tf.look_at(cam.target, Vec3::Y);
}
