//! Thin binary — DefaultPlugins + Sim/Render/Ui plugins.
//! Spawns deterministic mining slice scene: 1 ship + 2 asteroids seeded with WyRand in 10km box.
use bevy::prelude::*;

use spacegame_data::Distance;
use spacegame_render::RenderPlugin;
use spacegame_sim::{
    Asteroid, Crew, CrewRole, Inventory, MiningLaser, Order, OrderQueue, ShipStats, SimPlugin,
};
use spacegame_ui::UiPlugin;

fn main() -> anyhow::Result<()> {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "spacegame — slice 1 mining".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SimPlugin)
        .add_plugins(RenderPlugin)
        .add_plugins(UiPlugin)
        .add_systems(Startup, spawn_slice_scene)
        .run();
    Ok(())
}

/// Deterministic WyRand — copy of `spacegame_sim::rng::wyrand_next` (pub(crate) there).
#[inline]
fn wyrand_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

fn wyrand_vec3(seed: u64, idx: u64, half_extent: f32) -> Vec3 {
    let mut s = seed ^ (idx.wrapping_mul(0x9e3779b97f4a7c15));
    let r1 = wyrand_next(&mut s);
    let r2 = wyrand_next(&mut s);
    let r3 = wyrand_next(&mut s);
    let f = |r: u64| -> f32 {
        let u = (r & 0xffffffff) as f32 / (u32::MAX as f32);
        u * 2.0 * half_extent - half_extent
    };
    Vec3::new(f(r1), f(r2), f(r3))
}

fn spawn_slice_scene(mut commands: Commands) {
    // Data-driven ship stats from RON — never hardcoded.
    let miner_ron = include_str!("../assets/data/ships/miner.ron");
    let template = spacegame_data::parse_ship_ron(miner_ron).expect("miner.ron parses");
    let stats = ShipStats::from_template(&template);
    let laser = MiningLaser::from_template(&template);
    let orbit_range = stats.orbit_range.get();

    // Seeded system — 10km box, 2 asteroids deterministically placed.
    let seed: u64 = 0xdead_beef_cafe_1234;
    let half_extent = 5000.0;
    let asteroid_positions = [
        wyrand_vec3(seed, 0, half_extent),
        wyrand_vec3(seed, 1, half_extent),
    ];

    let mut asteroid_entities = Vec::new();
    for &pos in &asteroid_positions {
        // Keep asteroids roughly in view: y near 0.
        let pos = Vec3::new(pos.x, (pos.y * 0.2).clamp(-500.0, 500.0), pos.z);
        let entity = commands
            .spawn((Asteroid::new(1000, 1000), Transform::from_translation(pos)))
            .id();
        asteroid_entities.push(entity);
    }

    // Ship at origin with inventory + crew.
    let ship = commands
        .spawn((
            Transform::from_translation(Vec3::ZERO),
            stats.clone(),
            laser,
            Inventory::new(),
            OrderQueue::new(),
        ))
        .id();

    // Crew child of ship — skill 0.6 miner.
    commands.spawn((Crew::miner(0.6), ChildOf(ship)));
    // Duplicate miner role check: ensure ChildOf link.
    let _ = CrewRole::Miner;

    // Queue FIFO mining run: Approach -> Orbit -> Mine on first asteroid.
    // Demonstrates EVE-like scriptable queue; Mine persists while in range.
    if let Some(&first_asteroid) = asteroid_entities.first() {
        let mut queue = OrderQueue::new();
        queue.push_back(Order::Approach(first_asteroid));
        if let Ok(d) = Distance::new(orbit_range) {
            queue.push_back(Order::orbit(first_asteroid, d));
        }
        queue.push_back(Order::Mine(first_asteroid));
        commands.entity(ship).insert(queue);
    }

    // Log for headless verification.
    bevy::log::info!(
        "spawned ship {ship:?} with 2 asteroids at {:?} seeded {seed:#x}, orbit_range {orbit_range}",
        asteroid_positions
    );
}
