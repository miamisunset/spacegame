//! Shared headless harness — extracted from `headless_mining_loop.rs`.
//!
//! Provides deterministic `WyRand` helpers, `headless_app()` seam (`MinimalPlugins + SimPlugin`
//! with `TimeUpdateStrategy::ManualDuration`), `world_hash`, and `miner.ron` template helpers.
//! All integration tests (`crates/spacegame_sim/tests/*.rs`) should import `common` via
//! `mod common;` and use these helpers instead of copy-pasting WyRand.

use bevy::prelude::*;
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use spacegame_data::parse_ship_ron;
use spacegame_sim::{Asteroid, MiningLaser, ShipStats, SimPlugin};

// ---------------------------------------------------------------------------
// Data-driven RON — single source for miner stats
// ---------------------------------------------------------------------------

const MINER_RON: &str = include_str!("../../../../assets/data/ships/miner.ron");

/// Parse `miner.ron` from `assets/data/ships/miner.ron`.
#[must_use]
pub fn miner_template() -> spacegame_data::ShipTemplate {
    parse_ship_ron(MINER_RON).expect("miner ron parses")
}

/// `ShipStats` derived from `miner.ron` template.
#[must_use]
pub fn miner_stats() -> ShipStats {
    ShipStats::from_template(&miner_template())
}

/// `MiningLaser` derived from `miner.ron` template.
#[must_use]
pub fn miner_laser() -> MiningLaser {
    MiningLaser::from_template(&miner_template())
}

// ---------------------------------------------------------------------------
// Deterministic WyRand — byte-identical to `spacegame_sim::rng::wyrand_next`
// ---------------------------------------------------------------------------

/// Splitmix64 / WyRand step — deterministic, no `thread_rng`.
#[inline]
pub fn wyrand_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Deterministic `Vec3` in `[-half_extent, half_extent]` from `seed` and `idx`.
pub fn wyrand_vec3(seed: u64, idx: u64, half_extent: f32) -> Vec3 {
    let mut s = seed ^ idx.wrapping_mul(0x9e3779b97f4a7c15);
    let r1 = wyrand_next(&mut s);
    let r2 = wyrand_next(&mut s);
    let r3 = wyrand_next(&mut s);
    let to_f = |r: u64| -> f32 {
        let u = (r & 0xffffffff) as f32 / (u32::MAX as f32);
        u * 2.0 * half_extent - half_extent
    };
    Vec3::new(to_f(r1), to_f(r2), to_f(r3))
}

/// Assert `pos` lies within `[-half_extent, half_extent]` on all axes.
pub fn assert_within_system(pos: Vec3, half_extent: f32) {
    assert!(
        pos.x.abs() <= half_extent + f32::EPSILON
            && pos.y.abs() <= half_extent + f32::EPSILON
            && pos.z.abs() <= half_extent + f32::EPSILON,
        "wyrand position {pos:?} must be within half_extent {half_extent} (System 10km box)"
    );
}

/// Two WyRand-seeded asteroid positions within `half_extent`, asserted in-bounds.
pub fn seeded_asteroid_positions(seed: u64, half_extent: f32) -> [Vec3; 2] {
    let positions = [
        wyrand_vec3(seed, 0, half_extent),
        wyrand_vec3(seed, 1, half_extent),
    ];
    for pos in &positions {
        assert_within_system(*pos, half_extent);
    }
    positions
}

/// Spawn two `Asteroid` entities at WyRand positions within `half_extent`.
pub fn spawn_seeded_asteroids(app: &mut App, seed: u64, half_extent: f32) -> [Entity; 2] {
    let positions = seeded_asteroid_positions(seed, half_extent);
    let a = app
        .world_mut()
        .spawn((
            Asteroid::new(1000, 1000),
            Transform::from_translation(positions[0]),
        ))
        .id();
    let b = app
        .world_mut()
        .spawn((
            Asteroid::new(1000, 1000),
            Transform::from_translation(positions[1]),
        ))
        .id();
    [a, b]
}

// ---------------------------------------------------------------------------
// Headless App seam
// ---------------------------------------------------------------------------

/// Headless app — `MinimalPlugins + SimPlugin` with manual `Fixed` timestep.
///
/// No `bevy_render`/`bevy_pbr`, no window/GPU. Gated by `in_state(GameState::Simulating)`.
/// `SimPlugin` wires `FixedUpdate` `EconomySet→AiSet→MovementSet→MiningSet→CombatSet` via
/// `StatesPlugin` idempotent install.
pub fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SimPlugin));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        Time::<Fixed>::default().timestep(),
    ));
    app
}

/// Tick `n` times (`app.update()` per tick).
pub fn tick_n(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

// ---------------------------------------------------------------------------
// Deterministic world hash
// ---------------------------------------------------------------------------

/// Deterministic hash of world — paired `(Transform, Asteroid)` plus ship transforms.
///
/// Hashes `(x_bits, y_bits, z_bits, ore)` tuples for asteroids to preserve
/// (pos, ore) pairing, plus ship `Transform`s separately. Sorted tuples make
/// hash insertion-order independent. Uses `wrapping_add`/`wrapping_mul` per
/// `num-overflow-explicit`.
pub fn world_hash(app: &mut App) -> u64 {
    // Paired asteroid data: (pos bits, ore) — preserves (pos, ore) pairing.
    let mut asteroid_tuples: Vec<(u32, u32, u32, u32)> = {
        let mut query = app.world_mut().query::<(&Transform, &Asteroid)>();
        query
            .iter(app.world())
            .map(|(tf, ast)| {
                (
                    tf.translation.x.to_bits(),
                    tf.translation.y.to_bits(),
                    tf.translation.z.to_bits(),
                    ast.ore_remaining,
                )
            })
            .collect()
    };
    asteroid_tuples.sort_unstable();

    // Ship transforms (non-asteroid entities) — insertion-order independent.
    let mut ship_positions: Vec<(u32, u32, u32)> = {
        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, Without<Asteroid>>();
        query
            .iter(app.world())
            .map(|tf| {
                (
                    tf.translation.x.to_bits(),
                    tf.translation.y.to_bits(),
                    tf.translation.z.to_bits(),
                )
            })
            .collect()
    };
    ship_positions.sort_unstable();

    let mut hash: u64 = 0;
    for (x, y, z, ore) in asteroid_tuples {
        hash = hash.wrapping_add(x as u64);
        hash = hash.wrapping_add(y as u64);
        hash = hash.wrapping_add(z as u64);
        hash = hash.wrapping_add(ore as u64);
        hash = hash.wrapping_mul(0x9e3779b97f4a7c15);
        hash ^= hash >> 33;
    }
    for (x, y, z) in ship_positions {
        hash = hash.wrapping_add(x as u64);
        hash = hash.wrapping_add(y as u64);
        hash = hash.wrapping_add(z as u64);
        hash = hash.wrapping_mul(0x9e3779b97f4a7c15);
        hash ^= hash >> 33;
    }
    hash
}
