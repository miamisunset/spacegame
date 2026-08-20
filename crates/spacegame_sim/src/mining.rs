//! Stub [`MiningLaser`] module + deterministic mining.
//!
//! Range-checked mining adds Ore to [`Inventory`] only if volume fits in
//! `cargo_capacity`. Crew `skill_mining`/`fatigue` linearly scale
//! `yield_per_cycle`/`cycle_secs` (see [`crate::crew`]). No `unwrap` in prod,
//! no negative wares.

use bevy::prelude::*;
use spacegame_data::{Secs, ShipTemplate, Volume};
use std::collections::HashMap;

use crate::asteroid::Asteroid;
use crate::crew::{Crew, effective_cycle_secs, effective_yield, update_fatigue};
use crate::inventory::Inventory;
use crate::movement::ShipStats;
use crate::order::{Order, OrderQueue};

/// Data-driven ore volume resource — mirrors `assets/data/wares.ron` (`ore: 1.0`).
///
/// Slice-1 fallback is `Volume(1.0)` matching the RON default; slice-2 may load
/// `WaresRegistry` from disk and derive this value. Mining never hardcodes
/// `const VOLUME_PER_UNIT` — it reads this resource.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct OreVolume(pub Volume);

impl Default for OreVolume {
    fn default() -> Self {
        Self(Volume::new(1.0).expect("1.0 is valid volume"))
    }
}

/// Stub mining laser module on a ship.
///
/// `cycle_secs`/`yield_per_cycle` come from `ShipTemplate` (RON), `progress`
/// is `0..1` within the current cycle. Data-driven, never hard-coded.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct MiningLaser {
    /// Seconds per mining cycle (base, before crew scaling).
    pub cycle_secs: Secs,
    /// Base ore units per cycle (before crew scaling).
    pub yield_per_cycle: u32,
    /// Progress `0..1` toward next yield.
    pub progress: f32,
}

impl MiningLaser {
    /// Create from validated values.
    #[must_use]
    pub fn new(cycle_secs: Secs, yield_per_cycle: u32) -> Self {
        Self {
            cycle_secs,
            yield_per_cycle,
            progress: 0.0,
        }
    }

    /// Build from a [`ShipTemplate`] (data-driven).
    // own-borrow-over-clone: &ShipTemplate avoids move
    #[must_use]
    pub fn from_template(template: &ShipTemplate) -> Self {
        Self::new(template.cycle_secs, template.yield_per_cycle)
    }
}

/// Helper — deduplicates the 4x fatigue recovery pattern.
#[inline]
fn set_crew_fatigue(
    crews: &mut Query<(Entity, &mut Crew, &ChildOf)>,
    crew_entity_opt: Option<Entity>,
    is_mining: bool,
    dt: f32,
) {
    if let Some(c_ent) = crew_entity_opt
        && let Ok((_, mut crew_mut, _)) = crews.get_mut(c_ent)
    {
        crew_mut.fatigue = update_fatigue(crew_mut.fatigue, is_mining, dt);
    }
}

/// Mining system — runs in `MiningSet` after `MovementSet` (chain ensures
/// `Transform`/`distance` are fresh). Deterministic via `Time<Fixed>`.
///
/// For each ship whose front order is `Mine(asteroid)`:
/// - If asteroid missing/despawned: pop `Mine`.
/// - If out of range (`dist > mining_range`): recover fatigue, hold.
/// - If cargo full: pop `Mine`.
/// - Else: accrue `progress += dt / effective_cycle`, apply fatigue gain,
///   and on cycle completion add `effective_yield` (clamped to `min(remaining, free)`)
///   to `Inventory` and subtract from `Asteroid`. Pops `Mine` if cargo full.
///   Never negative.
///
/// err-result-over-panic / err-no-unwrap-prod: all `Option`/`Result` handled gracefully.
#[allow(clippy::excessive_nesting)]
pub(crate) fn mining_system(
    time: Res<Time<Fixed>>,
    ore_volume: Res<OreVolume>,
    mut ships: Query<(
        Entity,
        &Transform,
        &mut OrderQueue,
        &ShipStats,
        &mut MiningLaser,
        &mut Inventory,
    )>,
    mut asteroids: Query<(&mut Asteroid, &Transform)>,
    mut crews: Query<(Entity, &mut Crew, &ChildOf)>,
) {
    let dt = time.delta_secs();
    let volume_per_unit = ore_volume.0.get();

    // Feature Envy / O(n*m) fix: batched HashMap O(n+m) instead of per-ship linear scan.
    let mut crew_by_ship: HashMap<Entity, Entity> = HashMap::new();
    for (c_ent, _, child_of) in &crews {
        crew_by_ship.entry(child_of.parent()).or_insert(c_ent);
    }

    for (ship_entity, ship_tf, mut queue, stats, mut laser, mut inv) in &mut ships {
        let crew_entity_opt = crew_by_ship.get(&ship_entity).copied();

        let front = queue.front().cloned();
        let mine_target = match front {
            Some(Order::Mine(e)) => e,
            _ => {
                // Not mining — recover fatigue
                set_crew_fatigue(&mut crews, crew_entity_opt, false, dt);
                continue;
            }
        };

        let Ok((mut asteroid, ast_tf)) = asteroids.get_mut(mine_target) else {
            queue.pop_front();
            set_crew_fatigue(&mut crews, crew_entity_opt, false, dt);
            continue;
        };

        let dist = (ast_tf.translation - ship_tf.translation).length();
        let mining_range = stats.mining_range.get();
        let cargo_capacity = stats.cargo_capacity.get();

        if dist > mining_range + 1e-3 {
            // Out of range — recover fatigue, no progress
            set_crew_fatigue(&mut crews, crew_entity_opt, false, dt);
            continue;
        }

        // Check cargo full before cycling
        if inv.is_full(cargo_capacity, volume_per_unit) {
            queue.pop_front();
            set_crew_fatigue(&mut crews, crew_entity_opt, false, dt);
            continue;
        }

        // In range and has space — mining: gain fatigue first, then read fresh values.
        set_crew_fatigue(&mut crews, crew_entity_opt, true, dt);

        // Re-read skill/fatigue after update to avoid 1-tick lag.
        let (skill, fatigue_val) = crew_entity_opt
            .and_then(|c_ent| {
                crews
                    .get(c_ent)
                    .ok()
                    .map(|(_, c, _)| (c.skill_mining, c.fatigue))
            })
            .unwrap_or((0.0, 0.0));

        let effective_cycle = effective_cycle_secs(laser.cycle_secs.get(), skill, fatigue_val);
        laser.progress += dt / effective_cycle;

        if laser.progress >= 1.0 - f32::EPSILON {
            laser.progress -= 1.0;
            if laser.progress < 0.0 {
                laser.progress = 0.0;
            }
            if laser.progress >= 1.0 {
                laser.progress = laser.progress.fract();
            }

            let base_yield = laser.yield_per_cycle;
            let eff_yield = effective_yield(base_yield, skill, fatigue_val);
            let free = inv.free_capacity(cargo_capacity, volume_per_unit);
            let max_by_capacity = (free / volume_per_unit).floor() as u32;
            let actual = eff_yield.min(asteroid.ore_remaining).min(max_by_capacity);
            if actual == 0 {
                if inv.is_full(cargo_capacity, volume_per_unit) {
                    queue.pop_front();
                }
                continue;
            }
            let added = inv.try_add("ore", actual, volume_per_unit, cargo_capacity);
            asteroid.ore_remaining = asteroid.ore_remaining.saturating_sub(added);
            if inv.is_full(cargo_capacity, volume_per_unit) {
                queue.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimPlugin;
    use crate::asteroid::Asteroid;
    use crate::crew::{Crew, CrewRole};
    use crate::inventory::Inventory;
    use crate::movement::ShipStats;
    use crate::order::{Order, OrderQueue};
    use bevy::time::{Fixed, Time, TimeUpdateStrategy};
    use spacegame_data::{Distance, Secs, Speed, Volume};

    fn miner_stats() -> ShipStats {
        ShipStats::new(
            Speed::new(75.0).unwrap(),
            Distance::new(50.0).unwrap(),
            Distance::new(1500.0).unwrap(),
            Distance::new(1000.0).unwrap(),
            Volume::new(100.0).unwrap(),
        )
    }

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
    fn mining_laser_from_template() {
        let tmpl = spacegame_data::parse_ship_ron(
            r#"(id: "miner", speed: 75.0, cargo_capacity: 100.0, mining_range: 1500.0, cycle_secs: 5.0, yield_per_cycle: 10, orbit_range: 1000.0)"#,
        )
        .unwrap();
        let laser = MiningLaser::from_template(&tmpl);
        assert!((laser.cycle_secs.get() - 5.0).abs() < f32::EPSILON);
        assert_eq!(laser.yield_per_cycle, 10);
        assert!((laser.progress - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn miner_stats_has_cargo() {
        let s = miner_stats();
        assert!((s.cargo_capacity.get() - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn range_checked_mining_adds_ore_in_range() {
        let mut app = fixed_app();
        let stats = miner_stats();
        // Use short cycle for test speed
        let mut laser = MiningLaser::new(Secs::new(0.2).unwrap(), 10);
        laser.progress = 0.0;
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(1000, 1000),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO), // in range
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats.clone(),
                laser,
                Inventory::new(),
            ))
            .id();

        // Tick enough for ~3 cycles (0.2 sec each, dt 0.015625 => ~13 ticks per cycle)
        tick_n(&mut app, 50);
        let inv = app.world().get::<Inventory>(ship).unwrap();
        assert!(inv.get("ore") > 0, "should have mined ore in range");
        let ast = app.world().get::<Asteroid>(asteroid).unwrap();
        assert!(ast.ore_remaining < 1000);
        // No negative wares
        assert!(ast.ore_remaining <= 1000);
    }

    #[test]
    fn out_of_range_no_mining() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(1000, 1000),
                Transform::from_translation(Vec3::new(5000.0, 0.0, 0.0)),
            ))
            .id();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO), // 5000 away > mining_range 1500
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats,
                MiningLaser::new(Secs::new(0.2).unwrap(), 10),
                Inventory::new(),
            ))
            .id();

        tick_n(&mut app, 50);
        let inv = app.world().get::<Inventory>(ship).unwrap();
        assert_eq!(inv.get("ore"), 0, "out of range should not mine");
        let ast = app.world().get::<Asteroid>(asteroid).unwrap();
        assert_eq!(ast.ore_remaining, 1000);
    }

    #[test]
    fn cargo_full_pops_mine_and_no_overflow() {
        let mut app = fixed_app();
        let stats = ShipStats::new(
            Speed::new(75.0).unwrap(),
            Distance::new(50.0).unwrap(),
            Distance::new(1500.0).unwrap(),
            Distance::new(1000.0).unwrap(),
            Volume::new(5.0).unwrap(), // tiny cargo
        );
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(1000, 1000),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats,
                MiningLaser::new(Secs::new(0.05).unwrap(), 10),
                Inventory::new(),
            ))
            .id();

        tick_n(&mut app, 100);
        let inv = app.world().get::<Inventory>(ship).unwrap();
        // Should be capped at 5
        assert_eq!(inv.get("ore"), 5);
        assert!(inv.is_full(5.0, 1.0));
        // Mine should have popped
        let q = app.world().get::<OrderQueue>(ship).unwrap();
        assert!(q.is_empty(), "Mine should pop when cargo full");
        // No negative wares, asteroid not negative
        let ast = app.world().get::<Asteroid>(asteroid).unwrap();
        assert!(ast.ore_remaining <= 1000);
    }

    #[test]
    fn crew_skill_increases_yield() {
        // Two ships, same setup, different skill, same ticks
        let run = |skill: f32| -> u32 {
            let mut app = fixed_app();
            let stats = miner_stats();
            let asteroid = app
                .world_mut()
                .spawn((
                    Asteroid::new(10000, 10000),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id();
            let ship = app
                .world_mut()
                .spawn((
                    Transform::from_translation(Vec3::ZERO),
                    OrderQueue::with_order(Order::Mine(asteroid)),
                    stats,
                    MiningLaser::new(Secs::new(0.2).unwrap(), 10),
                    Inventory::new(),
                ))
                .id();
            let crew = app
                .world_mut()
                .spawn((Crew::new(CrewRole::Miner, skill, 0.0), ChildOf(ship)))
                .id();
            let _ = crew;
            tick_n(&mut app, 80);
            app.world().get::<Inventory>(ship).unwrap().get("ore")
        };

        let ore_low = run(0.0);
        let ore_high = run(1.0);
        assert!(
            ore_high > ore_low,
            "high skill {ore_high} should yield more than low {ore_low}"
        );
        // No negative
        assert!(ore_high <= 10000 && ore_low <= 10000);
    }

    #[test]
    fn fatigue_ticks_up_while_mining_and_scales_yield() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(10000, 10000),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats,
                MiningLaser::new(Secs::new(0.2).unwrap(), 10),
                Inventory::new(),
            ))
            .id();
        let crew_ent = app
            .world_mut()
            .spawn((Crew::new(CrewRole::Miner, 0.5, 0.0), ChildOf(ship)))
            .id();

        tick_n(&mut app, 200);
        let fatigue_after_mining = app.world().get::<Crew>(crew_ent).unwrap().fatigue;
        assert!(
            fatigue_after_mining > 0.0,
            "fatigue should tick up while mining, got {fatigue_after_mining}"
        );

        // Now remove Mine, idle should recover
        app.world_mut().get_mut::<OrderQueue>(ship).unwrap().clear();
        tick_n(&mut app, 200);
        let fatigue_after_idle = app.world().get::<Crew>(crew_ent).unwrap().fatigue;
        assert!(
            fatigue_after_idle < fatigue_after_mining,
            "fatigue should recover idle: before {fatigue_after_mining} after {fatigue_after_idle}"
        );
        // Linear scaling helper still holds
        let y_fresh = crate::crew::effective_yield(10, 0.5, 0.0);
        let y_tired = crate::crew::effective_yield(10, 0.5, 100.0);
        assert!(y_tired < y_fresh);
    }

    #[test]
    fn asteroid_depletion_clamped_no_negative() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let asteroid = app
            .world_mut()
            .spawn((
                Asteroid::new(5, 100),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats,
                MiningLaser::new(Secs::new(0.05).unwrap(), 10),
                Inventory::new(),
            ))
            .id();

        tick_n(&mut app, 50);
        // Asteroid should be depleted but not negative; either despawned or 0
        let ast_opt = app.world().get::<Asteroid>(asteroid);
        if let Some(ast) = ast_opt {
            assert_eq!(ast.ore_remaining, 0);
        } else {
            // despawned -> check queue
            assert!(
                !app.world()
                    .resource::<crate::asteroid::RespawnQueue>()
                    .is_empty()
            );
        }
        let inv = app.world().get::<Inventory>(ship).unwrap();
        // Only 5 ore existed, so at most 5 added (clamped to remaining)
        assert!(inv.get("ore") <= 5);
    }
}
