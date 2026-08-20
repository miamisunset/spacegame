//! Asteroid lifecycle — depletable [`Asteroid`] + deterministic respawn.
//!
//! Asteroids hold `ore_remaining` up to `max_ore`. When depleted to `0` they
//! despawn and are pushed to [`RespawnQueue`] with `pos`, `seed`, `respawn_tick`.
//! Respawn is deterministic via `WyRand` (`wyrand_next` splitmix64) at the same
//! `pos` with a new amount in `[max/2, max]`.

use bevy::prelude::*;

use crate::rng::wyrand_next;

/// Deterministic respawn delay in `FixedUpdate` ticks (500 ticks ~7.8s at 64Hz).
pub const RESPAWN_DELAY_TICKS: u64 = 500;

/// Derive respawn ore amount deterministically from `seed` and `max_ore`.
///
/// Returns value in `[max/2, max]` inclusive, clamped to `[1, max]`. Uses
/// `wyrand_next` so same `seed` yields same amount (deterministic).
#[must_use]
pub fn deterministic_respawn_amount(mut seed: u64, max_ore: u32) -> u32 {
    let max = max_ore.max(1);
    let half = (max / 2).max(1);
    let range = max - half + 1; // e.g. max 1000 => half 500 => range 501
    let r = wyrand_next(&mut seed);
    half + (r % range as u64) as u32
}

/// Asteroid — depletable ore source with a `Transform` position.
///
/// `ore_remaining` is clamped `0..=max_ore`; never negative (u32).
#[derive(Debug, Clone, PartialEq, Component)]
pub struct Asteroid {
    /// Remaining ore units.
    pub ore_remaining: u32,
    /// Maximum ore (template cap, used for respawn range).
    pub max_ore: u32,
}

impl Asteroid {
    /// Create validated asteroid.
    ///
    /// `ore_remaining` is clamped to `max_ore`. Panics in debug if `max_ore==0`.
    #[must_use]
    pub fn new(ore_remaining: u32, max_ore: u32) -> Self {
        assert!(max_ore > 0, "max_ore must be >0");
        let clamped = ore_remaining.min(max_ore);
        Self {
            ore_remaining: clamped,
            max_ore,
        }
    }
}

/// Entry in the respawn queue.
#[derive(Debug, Clone, PartialEq)]
pub struct RespawnEntry {
    /// Original asteroid position (respawn at same `pos`).
    pub pos: Vec3,
    /// Seed for deterministic amount derivation.
    pub seed: u64,
    /// Tick at which to respawn (`current_tick + RESPAWN_DELAY_TICKS`).
    pub respawn_tick: u64,
    /// Max ore for the respawned asteroid.
    pub max_ore: u32,
}

/// Deterministic tick counter incremented each `FixedUpdate`.
///
/// Separate from [`RespawnQueue`] so ticking (`EconomySet`) and respawn
/// (`MiningSet`) stay in their ordered sets. `RespawnQueue` stores
/// `respawn_tick` computed from this tick, so ordering via
/// `FixedUpdate` chain keeps determinism.
#[derive(Debug, Clone, PartialEq, Eq, Resource, Default)]
pub struct SimulationTick {
    /// Current tick.
    pub tick: u64,
}

/// Resource queue of asteroids awaiting respawn.
///
/// Headless `Resource` — in Bevy 0.19 `Resource: Component` but we never
/// derive `Component` on this type (hard error if both).
/// `respawn_tick` values are derived from [`SimulationTick`] at despawn time.
#[derive(Debug, Clone, PartialEq, Resource, Default)]
pub struct RespawnQueue {
    /// Pending respawns.
    pub queue: Vec<RespawnEntry>,
}

impl RespawnQueue {
    /// Push an entry.
    pub fn push(&mut self, entry: RespawnEntry) {
        self.queue.push(entry);
    }

    /// Number of pending entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Whether empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Increment [`SimulationTick`] each `FixedUpdate`. Ordered in `EconomySet` or `MiningSet`.
///
/// Deterministic — no `Time` dependency, just `+1`.
pub(crate) fn tick_increment_system(mut tick: ResMut<SimulationTick>) {
    tick.tick = tick.tick.wrapping_add(1);
}

/// Despawn asteroids at `ore_remaining == 0` and push to [`RespawnQueue`].
///
/// Reads `Transform` for `pos`; seed is derived deterministically from
/// `pos` bits mixed with `max_ore` only (not `tick`) so the same world
/// position always yields the same respawn amount (stable seed).
pub(crate) fn asteroid_despawn_system(
    mut commands: Commands,
    mut queue: ResMut<RespawnQueue>,
    tick: Res<SimulationTick>,
    asteroids: Query<(Entity, &Asteroid, &Transform)>,
) {
    for (entity, ast, tf) in &asteroids {
        if ast.ore_remaining == 0 {
            // Derive seed deterministically from pos + max only (no tick) for stable world seed.
            let mut seed_mix = 0x9e3779b97f4a7c15u64;
            seed_mix ^= tf.translation.x.to_bits() as u64;
            seed_mix =
                seed_mix.wrapping_mul(0xbf58476d1ce4e5b9) ^ tf.translation.y.to_bits() as u64;
            seed_mix =
                seed_mix.wrapping_mul(0x94d049bb133111eb) ^ tf.translation.z.to_bits() as u64;
            seed_mix ^= (ast.max_ore as u64).wrapping_mul(0x9e3779b97f4a7c15);
            // Final mix via wyrand
            let mut s = seed_mix;
            let seed = wyrand_next(&mut s);

            queue.push(RespawnEntry {
                pos: tf.translation,
                seed,
                respawn_tick: tick.tick.wrapping_add(RESPAWN_DELAY_TICKS),
                max_ore: ast.max_ore,
            });
            commands.entity(entity).despawn();
        }
    }
}

/// Respawn asteroids when `tick` has reached `respawn_tick`.
///
/// Spawns `Transform` at `pos` with `Asteroid` amount derived via
/// [`deterministic_respawn_amount`]. Uses wrapping-aware comparison
/// (`num-overflow-explicit`): `tick.wrapping_sub(respawn_tick) < 2^63` is true
/// iff `tick` is at or past `respawn_tick` in wrapping `u64` order, so the
/// check remains correct after `u64::MAX` wrap. Production ticks are
/// practically bounded (`< 10k` in tests, ~2B/yr at 64 Hz), but the wrap-safe
/// form documents the bound and avoids a latent bug.
pub(crate) fn asteroid_respawn_system(
    mut commands: Commands,
    mut queue: ResMut<RespawnQueue>,
    tick: Res<SimulationTick>,
) {
    let mut i = 0;
    while i < queue.queue.len() {
        let entry = queue.queue[i].clone();
        // `num-overflow-explicit`: wrap-aware "tick >= respawn_tick" without branch on overflow.
        // `wrapping_sub` distance < 2^63 means tick is at/past respawn_tick.
        let is_due = tick.tick.wrapping_sub(entry.respawn_tick) < (1u64 << 63);
        if is_due {
            let amount = deterministic_respawn_amount(entry.seed, entry.max_ore);
            commands.spawn((
                Asteroid::new(amount, entry.max_ore),
                Transform::from_translation(entry.pos),
            ));
            queue.queue.remove(i);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimPlugin;
    use bevy::time::{Fixed, Time, TimeUpdateStrategy};

    fn fixed_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        app
    }

    fn tick_n(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    #[test]
    fn asteroid_new_clamps() {
        let a = Asteroid::new(2000, 1000);
        assert_eq!(a.ore_remaining, 1000);
        assert_eq!(a.max_ore, 1000);
        let b = Asteroid::new(500, 1000);
        assert_eq!(b.ore_remaining, 500);
    }

    #[test]
    #[should_panic]
    fn asteroid_new_panics_on_zero_max() {
        let _ = Asteroid::new(0, 0);
    }

    #[test]
    fn deterministic_respawn_amount_in_range() {
        for max in [1, 2, 10, 500, 1000] {
            for seed in [0, 1, 42, 0xdeadbeef] {
                let amt = deterministic_respawn_amount(seed, max);
                assert!(
                    amt >= (max / 2).max(1) && amt <= max,
                    "amt {amt} for max {max} seed {seed}"
                );
            }
        }
        // same seed => same amount
        assert_eq!(
            deterministic_respawn_amount(123, 1000),
            deterministic_respawn_amount(123, 1000)
        );
        assert_ne!(
            deterministic_respawn_amount(123, 1000),
            deterministic_respawn_amount(124, 1000)
        );
    }

    #[test]
    fn despawn_pushes_queue_and_respawns_after_delay() {
        let mut app = fixed_app();
        // Spawn asteroid with 0 ore — should despawn next tick
        let pos = Vec3::new(100.0, 0.0, 0.0);
        let asteroid = app
            .world_mut()
            .spawn((Asteroid::new(0, 1000), Transform::from_translation(pos)))
            .id();

        // Need at least 2 ticks for FixedUpdate to run with ManualDuration
        tick_n(&mut app, 2);
        // Entity should be despawned
        assert!(
            app.world().get_entity(asteroid).is_err(),
            "asteroid at 0 should despawn"
        );
        let queue = app.world().resource::<RespawnQueue>();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.queue[0].pos, pos);

        // Fast-forward to respawn
        let respawn_tick = queue.queue[0].respawn_tick;
        let current = app.world().resource::<SimulationTick>().tick;
        let remaining = (respawn_tick - current) as usize;
        tick_n(&mut app, remaining + 2);

        let queue_after = app.world().resource::<RespawnQueue>();
        assert!(queue_after.is_empty(), "queue should drain after respawn");

        // Check respawned asteroid exists at same pos with valid amount
        let mut q = app.world_mut().query::<(&Asteroid, &Transform)>();
        let found = q.iter(app.world()).find(|(_, tf)| tf.translation == pos);
        assert!(found.is_some(), "respawned asteroid should be at same pos");
        let (ast, _) = found.unwrap();
        assert!(ast.ore_remaining >= 500 && ast.ore_remaining <= 1000);
        assert_eq!(ast.max_ore, 1000);
        // no negative wares
        assert!(ast.ore_remaining <= ast.max_ore);
    }

    #[test]
    fn asteroid_with_remaining_does_not_despawn() {
        let mut app = fixed_app();
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(500, 1000),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        tick_n(&mut app, 5);
        assert!(app.world().get_entity(asteroid).is_ok());
        assert!(app.world().resource::<RespawnQueue>().is_empty());
    }

    #[test]
    fn respawn_queue_deterministic_amount() {
        // Same seed + max yields same spawn amount via system
        let seed = 0x1234_5678_9abc_def0;
        let max = 1000;
        let amt1 = deterministic_respawn_amount(seed, max);
        let amt2 = deterministic_respawn_amount(seed, max);
        assert_eq!(amt1, amt2);
    }

    #[test]
    fn no_negative_wares_after_respawn_cycle() {
        let mut app = fixed_app();
        // Spawn, deplete via direct mutation, respawn, repeat 3 cycles
        for _ in 0..3 {
            let pos = Vec3::new(10.0, 0.0, 0.0);
            app.world_mut()
                .spawn((Asteroid::new(1, 100), Transform::from_translation(pos)));
            tick_n(&mut app, 2);
            // Mine manually: set to 0
            {
                let mut qs = app.world_mut().query::<&mut Asteroid>();
                for mut a in qs.iter_mut(app.world_mut()) {
                    a.ore_remaining = 0;
                }
            }
            tick_n(&mut app, 2);
            // should be despawned and queued
            // fast-forward respawn
            let rt = app.world().resource::<RespawnQueue>().queue[0].respawn_tick;
            let cur = app.world().resource::<SimulationTick>().tick;
            tick_n(&mut app, (rt - cur) as usize + 2);
            // verify no negative and within bounds
            let mut qs = app.world_mut().query::<&Asteroid>();
            for a in qs.iter(app.world()) {
                assert!(a.ore_remaining <= a.max_ore);
                assert!(a.ore_remaining >= 1);
            }
        }
    }
}
