//! Bevy render plugins, meshes, materials, lighting.
//!
//! Slice 1 placeholder visuals: ship as [`Cuboid`] and asteroids as
//! [`Sphere`] icospheres, with a detached strategic camera (orbit/pan/zoom,
//! WASD moves camera, not ship). All meshes sync via [`Transform`] written
//! by `MovementSet` on `FixedUpdate`; this crate only spawns visuals and
//! drives the camera on `Update`.
//!
//! Determinism: rendering is never on the `FixedUpdate` path; only `SimPlugin`
//! ticks deterministically. `WyRand` seeding for asteroid positions reuses
//! `spacegame_sim::rng` so visual and headless seeds agree.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use spacegame_sim::{
    Asteroid, ContextMenuState, Crew, CrewRole, GroundPlane, Inventory, MiningLaser, OrderQueue,
    ShipStats,
};

/// Marker for the player ship mesh entity (also carries sim components).
#[derive(Component, Debug, Clone, Copy)]
pub struct ShipVisual;

/// Marker for asteroid mesh entities.
#[derive(Component, Debug, Clone, Copy)]
pub struct AsteroidVisual;

/// Strategic camera state — detached orbit camera per spec.
///
/// `target` is look-at point on the XZ plane. `yaw`/`pitch` are in radians,
/// `distance` is orbital radius from target. `Update` system drives
/// `Transform` from this state; `WASD` moves `target`, scroll changes
/// `distance`, right-drag orbits.
#[derive(Resource, Debug, Clone)]
pub struct StrategicCamera {
    /// Look-at target (world units).
    pub target: Vec3,
    /// Yaw around world Y (radians).
    pub yaw: f32,
    /// Pitch above horizon (radians, clamped).
    pub pitch: f32,
    /// Distance from target.
    pub distance: f32,
}

impl Default for StrategicCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: -0.35, // ~ -20°
            distance: 3500.0,
        }
    }
}

/// Camera orbit limits — extracted to avoid Data Clumps / Repeated Switches.
const CAM_MIN_DISTANCE: f32 = 400.0;
const CAM_MAX_DISTANCE: f32 = 12000.0;
const CAM_MIN_PITCH: f32 = -1.45;
const CAM_MAX_PITCH: f32 = 1.30;

/// Placeholder render plugin for slice 1.
///
/// Spawns cube ship + icosphere asteroids from `Transform` sync,
/// detached strategic camera (orbit/pan/zoom, WASD moves camera),
/// directional light, and ensures respawned asteroids gain a mesh.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        // `MeshPickingPlugin` provides the ray-cast backend for `Pointer<Click>`
        // on `Mesh3d` entities. `DefaultPlugins` (binary) already adds
        // `DefaultPickingPlugins` (`PointerInputPlugin` + `PickingPlugin` +
        // `InteractionPlugin`) when `bevy_picking` is enabled; `bevy_ui` adds
        // `UiPickingPlugin` automatically. Only the mesh backend is missing
        // without this line — without it `Pointer<Click>` never fires for 3D meshes.
        app.add_plugins(bevy::picking::mesh_picking::MeshPickingPlugin);
        app.init_resource::<StrategicCamera>();
        app.add_systems(Startup, setup_scene);
        // Camera runs on `Update` (render/input), never `FixedUpdate`.
        app.add_systems(Update, camera_controller_system);
        // Respawns from `MiningSet` add bare `Asteroid + Transform` without visuals.
        // This system hydrates a placeholder icosphere mesh for any `Asteroid`
        // entity that lacks a `Mesh3d`.
        app.add_systems(Update, ensure_asteroid_mesh_system);
    }
}

/// Shared asteroid material helper — avoids Duplicated Code between
/// `setup_scene` and `ensure_asteroid_mesh_system` (`mem-with-capacity` not needed).
fn asteroid_material(assets: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    assets.add(StandardMaterial {
        base_color: Color::srgb(0.68, 0.52, 0.32),
        perceptual_roughness: 0.85,
        metallic: 0.0,
        ..default()
    })
}

/// Spawn slice-1 scene: light, camera, ship cube, two asteroid icospheres.
///
/// Data-driven: loads `assets/data/ships/miner.ron` via `spacegame_data`
/// (no hard-coded stats fallback). Asteroid positions reuse
/// `spacegame_sim::rng::wyrand_vec3` so visual and headless seeds agree.
/// `err-no-unwrap-prod` / `err-result-over-panic`: no `unwrap`/`expect` —
///
/// failures log via `bevy::log::error!` and early-return without panicking.
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_xyz(4000.0, 6000.0, 2000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let cam = StrategicCamera::default();
    let cam_pos = camera_position(&cam);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        Transform::from_translation(cam_pos).looking_at(cam.target, Vec3::Y),
    ));

    let Some(ship_template) = load_miner_template() else {
        bevy::log::error!(
            "miner template missing — ship not spawned; check assets/data/ships/miner.ron"
        );
        return;
    };
    let stats = ShipStats::from_template(&ship_template);
    let laser = MiningLaser::from_template(&ship_template);

    let ship_mesh = meshes.add(Cuboid::new(40.0, 12.0, 60.0));
    let ship_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.62, 0.92),
        perceptual_roughness: 0.6,
        metallic: 0.1,
        ..default()
    });

    let ship_entity = commands
        .spawn((
            ShipVisual,
            Mesh3d(ship_mesh),
            MeshMaterial3d(ship_mat),
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            stats.clone(),
            laser,
            Inventory::new(),
            OrderQueue::new(),
        ))
        .id();

    commands.spawn((Crew::new(CrewRole::Miner, 0.6, 0.0), ChildOf(ship_entity)));

    // Ground plane for `FlyTo Here` world-position picking. Large `Plane3d`
    // at y=0, pickable via `MeshPickingPlugin` (`GroundPlane` marker lets
    // the UI observer extract `HitData::position` without a `bevy_render`
    // ray-cast). Visible as subtle floor for strategic camera reference.
    let ground_mesh = meshes.add(Plane3d::default().mesh().size(20000.0, 20000.0));
    let ground_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        GroundPlane,
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_mat),
        Transform::from_translation(Vec3::new(0.0, -250.0, 0.0)),
    ));

    let positions = spacegame_sim::rng::seeded_positions(0x9a7b_c3d1_5e2f_8a01, 2, 5000.0);
    let shared_asteroid_mat = asteroid_material(&mut materials);
    for pos in positions {
        let asteroid_mesh = match Sphere::new(80.0).mesh().ico(3) {
            Ok(mesh) => meshes.add(mesh),
            Err(err) => {
                bevy::log::error!("failed to build icosphere mesh: {err}");
                continue;
            }
        };
        commands.spawn((
            AsteroidVisual,
            Asteroid::new(800, 1200),
            Mesh3d(asteroid_mesh),
            MeshMaterial3d(shared_asteroid_mat.clone()),
            Transform::from_translation(pos),
        ));
    }
}

/// Load `miner.ron` data-driven via `spacegame_data`.
///
/// Compile-time `include_str!` so the binary never bakes an absolute
/// `CARGO_MANIFEST_DIR` path (which would `Io NotFound` on CI/installed
/// binaries). Parse via [`spacegame_data::parse_ship_ron`] and log on
/// failure instead of panicking (`err-no-unwrap-prod`).
fn load_miner_template() -> Option<spacegame_data::ShipTemplate> {
    const MINER_RON: &str = include_str!("../../../assets/data/ships/miner.ron");
    match spacegame_data::parse_ship_ron(MINER_RON) {
        Ok(tmpl) => Some(tmpl),
        Err(err) => {
            bevy::log::error!("failed to load miner template: {err}");
            None
        }
    }
}

/// Compute camera world position from orbit state.
fn camera_position(cam: &StrategicCamera) -> Vec3 {
    let pitch = cam.pitch.clamp(CAM_MIN_PITCH, CAM_MAX_PITCH);
    let dist = cam.distance.clamp(CAM_MIN_DISTANCE, CAM_MAX_DISTANCE);
    let yaw = cam.yaw;
    let x = dist * pitch.cos() * yaw.sin();
    let y = dist * pitch.sin();
    let z = dist * pitch.cos() * yaw.cos();
    cam.target + Vec3::new(x, y, z)
}

/// Detached strategic camera controller.
///
/// `WASD` moves `target` on the XZ plane (camera-relative forward/right),
/// `Q`/`E` move vertically, scroll zooms `distance`, right-drag orbits
/// `yaw`/`pitch`. Pan via middle-drag, zoom via wheel — all on `Update`,
/// never `FixedUpdate`, and never moves the ship.
#[allow(clippy::too_many_arguments)]
fn camera_controller_system(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut cam: ResMut<StrategicCamera>,
    mut camera_q: Query<&mut Transform, With<Camera3d>>,
    context_menu: Res<ContextMenuState>,
) {
    let dt = time.delta_secs();
    let move_speed = (cam.distance * 0.0006 + 120.0) * dt * 60.0;
    let mut delta_target = Vec3::ZERO;

    let yaw = cam.yaw;
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());

    if keyboard.pressed(KeyCode::KeyW) {
        delta_target += forward * move_speed * 1.5;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        delta_target -= forward * move_speed * 1.5;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        delta_target -= right * move_speed * 1.5;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        delta_target += right * move_speed * 1.5;
    }
    if keyboard.pressed(KeyCode::KeyQ) {
        delta_target.y -= move_speed;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        delta_target.y += move_speed;
    }
    if keyboard.pressed(KeyCode::ArrowUp) {
        delta_target += forward * move_speed;
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        delta_target -= forward * move_speed;
    }
    if keyboard.pressed(KeyCode::ArrowLeft) {
        delta_target -= right * move_speed;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        delta_target += right * move_speed;
    }
    cam.target += delta_target;

    for ev in mouse_wheel.read() {
        let delta = ev.y * cam.distance * 0.08;
        cam.distance = (cam.distance - delta).clamp(CAM_MIN_DISTANCE, CAM_MAX_DISTANCE);
    }

    let mut yaw_delta: f32 = 0.0;
    let mut pitch_delta: f32 = 0.0;
    let mut pan_delta = Vec2::ZERO;
    // Skip right-drag orbit when context menu is visible — right-click
    // opens the menu, not the camera. Menu hides on left-click/Escape.
    let orbit_enabled = *context_menu == ContextMenuState::Hidden;
    for ev in mouse_motion.read() {
        if orbit_enabled && mouse_button.pressed(MouseButton::Right) {
            yaw_delta -= ev.delta.x * 0.003;
            pitch_delta -= ev.delta.y * 0.003;
        } else if mouse_button.pressed(MouseButton::Middle) {
            pan_delta += ev.delta;
        }
    }
    cam.yaw += yaw_delta;
    cam.pitch = (cam.pitch + pitch_delta).clamp(CAM_MIN_PITCH, CAM_MAX_PITCH);

    if pan_delta != Vec2::ZERO {
        let pan_scale = cam.distance * 0.0012;
        cam.target -= right * pan_delta.x * pan_scale;
        cam.target += Vec3::Y * pan_delta.y * pan_scale;
    }

    let pos = camera_position(&cam);
    for mut tf in &mut camera_q {
        tf.translation = pos;
        tf.look_at(cam.target, Vec3::Y);
    }
}

/// Ensure any `Asteroid` entity lacking a mesh gains a placeholder icosphere.
///
/// Respawned asteroids from `asteroid_respawn_system` spawn bare `Asteroid +
/// Transform`; this hydrates them with `Mesh3d + MeshMaterial3d + AsteroidVisual`
/// so the visual and simulation stay in sync. Runs on `Update` — one-frame
/// visual lag after `FixedUpdate` respawn is acceptable for slice 1.
#[allow(clippy::type_complexity)]
fn ensure_asteroid_mesh_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asteroids: Query<(Entity, &Asteroid), (Without<Mesh3d>, Without<AsteroidVisual>)>,
) {
    // Create one material and reuse via `Handle::clone` — avoids per-asteroid
    // `Assets::add` duplication flagged in review (each call allocated a new handle).
    let has_asteroids = asteroids.iter().next().is_some();
    let shared_mat = has_asteroids.then(|| asteroid_material(&mut materials));
    for (entity, asteroid) in &asteroids {
        let radius = 60.0 + (asteroid.max_ore as f32 / 1200.0) * 40.0;
        let mesh = match Sphere::new(radius).mesh().ico(3) {
            Ok(m) => meshes.add(m),
            Err(err) => {
                bevy::log::error!("failed to build asteroid icosphere radius {radius}: {err}");
                continue;
            }
        };
        let mat = shared_mat
            .clone()
            .unwrap_or_else(|| asteroid_material(&mut materials));
        commands
            .entity(entity)
            .insert((AsteroidVisual, Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
