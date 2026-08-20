//! Bevy render plugins, meshes, materials, lighting.
//!
//! Slice 1 placeholder visuals: ship as [`Cuboid`] and asteroids as
//! [`Sphere`] icospheres, with a detached strategic camera (orbit/pan/zoom,
//! WASD moves camera, not ship). All meshes sync via [`Transform`] written
//! by `MovementSet` on `FixedUpdate`; this crate only spawns visuals and
//! drives the camera on `Update`.
//!
//! Determinism: rendering is never on the `FixedUpdate` path; only `SimPlugin`
//! ticks deterministically. `WyRand` seeding for asteroid positions mirrors
//! `spacegame_sim::rng` so visual and headless seeds agree.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use spacegame_data::{Distance, Secs, Speed, Volume};
use spacegame_sim::{Asteroid, Crew, CrewRole, Inventory, MiningLaser, OrderQueue, ShipStats};

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

/// Placeholder render plugin for slice 1.
///
/// Spawns cube ship + icosphere asteroids from `Transform` sync,
/// detached strategic camera (orbit/pan/zoom, WASD moves camera),
/// directional light, and ensures respawned asteroids gain a mesh.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
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

/// Spawn slice-1 scene: light, camera, ship cube, two asteroid icospheres.
///
/// Data-driven: attempts to load `assets/data/ships/miner.ron`; falls back to
/// an inline parse that mirrors that file so the binary never hardcodes stats
/// without a RON parse path. Asteroid positions use `wyrand_next`-style
/// seeding to match headless tests.
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Directional light + ambient handled via `DirectionalLight` entity.
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_xyz(4000.0, 6000.0, 2000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Camera: StrategicCamera state drives transform; spawn at derived pos.
    let cam = StrategicCamera::default();
    let cam_pos = camera_position(&cam);
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(cam_pos).looking_at(cam.target, Vec3::Y),
        // Spec future: Camera3d order 0, Camera2d order 1 overlay already in UiPlugin.
        // Keep Ui camera clear colour transparent so 3d scene shows.
    ));

    // Load miner template data-driven. Try file then fallback.
    let ship_template = load_miner_template();
    let stats = ShipStats::from_template(&ship_template);
    let laser = MiningLaser::from_template(&ship_template);

    // Ship: cube placeholder, size ~40×10×60 (long forward Z).
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

    // Crew child per CONTEXT.md skeleton.
    commands.spawn((Crew::new(CrewRole::Miner, 0.6, 0.0), ChildOf(ship_entity)));

    // Two asteroids via deterministic WyRand positions (seed 0x9a7b_c3d1, half_extent 5000).
    let positions = seeded_positions(0x9a7b_c3d1_5e2f_8a01, 2, 5000.0);
    for pos in positions {
        let asteroid_mesh = meshes.add(Sphere::new(80.0).mesh().ico(3).unwrap());
        let asteroid_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.68, 0.52, 0.32),
            perceptual_roughness: 0.85,
            metallic: 0.0,
            ..default()
        });
        commands.spawn((
            AsteroidVisual,
            Asteroid::new(800, 1200),
            Mesh3d(asteroid_mesh),
            MeshMaterial3d(asteroid_mat),
            Transform::from_translation(pos),
        ));
    }
}

/// Load `miner.ron` from disk if present, otherwise parse inline fallback
/// that mirrors `assets/data/ships/miner.ron`. Keeps data pipeline typed via
/// `spacegame_data` and avoids hardcoding outside RON parse.
fn load_miner_template() -> spacegame_data::ShipTemplate {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // `spacegame_render` crate manifest is `crates/spacegame_render`; backtrack to workspace root.
    let candidate = manifest_dir.join("../../assets/data/ships/miner.ron");
    if let Ok(tmpl) = spacegame_data::load_ship_file(&candidate) {
        return tmpl;
    }
    // Fallback: parse same shape as `miner.ron`.
    spacegame_data::parse_ship_ron(
        r#"(
            id: "miner",
            speed: 75.0,
            cargo_capacity: 100.0,
            mining_range: 1500.0,
            cycle_secs: 5.0,
            yield_per_cycle: 10,
            orbit_range: 1000.0,
        )"#,
    )
    .expect("fallback miner ron parses")
}

/// Deterministic WyRand-seeded positions matching `spacegame_sim::rng::wyrand_next`
/// (splitmix64 variant). Used for visual seeding; do not use `thread_rng`.
fn seeded_positions(seed: u64, n: usize, half_extent: f32) -> Vec<Vec3> {
    (0..n as u64)
        .map(|idx| wyrand_vec3(seed, idx, half_extent))
        .collect()
}

fn wyrand_next(state: &mut u64) -> u64 {
    // Mirrors `spacegame_sim::rng::wyrand_next`.
    let mut s = *state;
    s = s.wrapping_add(0x60bee2bee120fc15);
    let mut t = s.wrapping_mul(0xa3b195354a39b70d);
    t ^= t >> 32;
    t = t.wrapping_mul(0xa511e9b123f3b8a7);
    t ^= t >> 32;
    *state = s;
    t
}

fn wyrand_vec3(seed: u64, idx: u64, half_extent: f32) -> Vec3 {
    let mut s = seed ^ idx.wrapping_mul(0x9e3779b97f4a7c15);
    let r1 = wyrand_next(&mut s);
    let r2 = wyrand_next(&mut s);
    let r3 = wyrand_next(&mut s);
    let f = |r: u64| -> f32 {
        let u = (r & 0xffffffff) as f32 / (u32::MAX as f32);
        u * 2.0 * half_extent - half_extent
    };
    Vec3::new(f(r1), f(r2), f(r3))
}

/// Compute camera world position from orbit state.
fn camera_position(cam: &StrategicCamera) -> Vec3 {
    let yaw = cam.yaw;
    let pitch = cam.pitch.clamp(-1.45, 1.30);
    let dist = cam.distance.clamp(400.0, 12000.0);
    // Spherical around target: use yaw (Y axis) then pitch.
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
fn camera_controller_system(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut cam: ResMut<StrategicCamera>,
    mut camera_q: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    // Movement speed scaled by distance so pan feels consistent zoomed out.
    let move_speed = (cam.distance * 0.0006 + 120.0) * dt * 60.0;
    let mut delta_target = Vec3::ZERO;

    // Derive forward/right from yaw only (XZ plane), ignore pitch for WASD.
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
    // Arrow keys also pan for accessibility.
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

    // Zoom via wheel.
    for ev in mouse_wheel.read() {
        // Bevy MouseWheel y is in lines; scale to world units.
        let delta = ev.y * cam.distance * 0.08;
        cam.distance = (cam.distance - delta).clamp(400.0, 12000.0);
    }

    // Orbit: right-drag rotates yaw/pitch. Pan: middle-drag translates target.
    let mut yaw_delta: f32 = 0.0;
    let mut pitch_delta: f32 = 0.0;
    let mut pan_delta = Vec2::ZERO;
    for ev in mouse_motion.read() {
        if mouse_button.pressed(MouseButton::Right) {
            yaw_delta -= ev.delta.x * 0.003;
            pitch_delta -= ev.delta.y * 0.003;
        } else if mouse_button.pressed(MouseButton::Middle) {
            pan_delta += ev.delta;
        }
    }
    cam.yaw += yaw_delta;
    cam.pitch = (cam.pitch + pitch_delta).clamp(-1.45, 1.30);

    if pan_delta != Vec2::ZERO {
        // Pan target in camera plane.
        let pan_scale = cam.distance * 0.0012;
        cam.target -= right * pan_delta.x * pan_scale;
        // Up on screen is world up minus forward component for orbit pitch.
        let cam_right = right;
        let cam_up = Vec3::Y;
        // Use screen Y to move along camera up-ish (approx world Y + pitch).
        cam.target += cam_up * pan_delta.y * pan_scale;
        // Compensate: remove this to keep simple xz+up pan (spec allows simple pan).
        let _ = cam_right;
    }

    // Apply to actual camera Transform.
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
/// so the visual and simulation stay in sync.
#[allow(clippy::type_complexity)]
fn ensure_asteroid_mesh_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asteroids: Query<(Entity, &Asteroid), (Without<Mesh3d>, Without<AsteroidVisual>)>,
) {
    for (entity, asteroid) in &asteroids {
        // Radius scales slightly with max_ore for visual variety.
        let radius = 60.0 + (asteroid.max_ore as f32 / 1200.0) * 40.0;
        let mesh = meshes.add(Sphere::new(radius).mesh().ico(3).unwrap());
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.68, 0.52, 0.32),
            perceptual_roughness: 0.85,
            ..default()
        });
        commands
            .entity(entity)
            .insert((AsteroidVisual, Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}

// Keep lints quiet for intentionally type-safe newtype imports not yet read
// in `setup_scene` fallback: `Distance`/`Secs` etc are re-exported for future sharding.
#[allow(unused_imports)]
fn _assert_newtype_imports(_d: Distance, _s: Speed, _v: Volume, _t: Secs) {}
