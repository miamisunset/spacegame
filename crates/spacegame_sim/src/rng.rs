//! Deterministic RNG helpers — single source for splitmix64 `wyrand_next`.

use bevy::prelude::Vec3;

/// Splitmix64 / WyRand step — deterministic, no `thread_rng`.
///
/// Shared by `asteroid` and `movement` (avoids Duplicated Code).
#[inline]
pub fn wyrand_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Deterministic 3-D position inside `[-half_extent, half_extent]` from `seed`+`idx`.
///
/// Mirrors test harness helpers (`wyrand_vec3`, `seeded_positions`) so visual
/// and headless seeds agree.
#[must_use]
pub fn wyrand_vec3(seed: u64, idx: u64, half_extent: f32) -> Vec3 {
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

/// Deterministic seeded positions for a bounded system.
#[must_use]
pub fn seeded_positions(seed: u64, n: usize, half_extent: f32) -> Vec<Vec3> {
    (0..n as u64)
        .map(|idx| wyrand_vec3(seed, idx, half_extent))
        .collect()
}
