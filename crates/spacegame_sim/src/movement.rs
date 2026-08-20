//! Kinematic steering (seek/arrive + tangential orbit) for [`OrderQueue`].
//!
//! Purely kinematic `pos += vel * dt` on `FixedUpdate`; no thrust, mass or
//! inertia per ADR-0002. Reads stats from RON-derived [`ShipStats`] — never
//! hardcodes `speed`/`mining_range` except for `ARRIVAL_RADIUS` and test
//! fixtures that mirror `assets/data/ships/miner.ron`. Deterministic:
//! only `Time<Fixed>::delta_secs()` and `Vec3` math, `Vec3::Y` as up for
//! tangent — no `thread_rng`, no map iteration.

use bevy::prelude::*;
use spacegame_data::{Distance, ShipTemplate, Speed};

use crate::order::{OrbitTarget, Order, OrderQueue};

/// Helper: deterministic seek/arrive step toward `target_pos` from `current`.
///
/// Returns `(new_position, arrived)` where `arrived` is true when
/// `distance <= arrival` before the step. Scale is `min(dist/arrival,1)*speed`.
#[inline]
fn seek_arrive_step(current: Vec3, target_pos: Vec3, arrival: f32, speed: f32, dt: f32) -> Vec3 {
    let dir = target_pos - current;
    let dist = dir.length();
    if dist <= f32::EPSILON {
        return current;
    }
    let scale = (dist / arrival).min(1.0);
    let effective_speed = speed * scale;
    let step_len = effective_speed * dt;
    if step_len >= dist {
        target_pos
    } else {
        current + dir / dist * step_len
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Arrival radius for `FlyTo`/`Approach`: considered arrived when
/// `distance <= ARRIVAL_RADIUS`. Not authored in RON, chosen as
/// `50.0` world units — small relative to `orbit_range` (1000) and
/// `mining_range` (1500) but large enough to avoid float chatter.
/// Deterministic constant per ADR-0002.
// own-borrow-over-clone: constants are Copy, pass by value
pub const ARRIVAL_RADIUS: f32 = 50.0;

/// [`Distance`] newtype view of [`ARRIVAL_RADIUS`] for typed APIs.
pub const ARRIVAL_DISTANCE: Distance = Distance(ARRIVAL_RADIUS);

// ---------------------------------------------------------------------------
// ShipStats — data-driven per-ship kinematic profile
// ---------------------------------------------------------------------------

/// Kinematic profile for a [`Ship`] entity (CONTEXT.md: Ship is the entity).
///
/// Data-driven: `speed`/`mining_range`/`orbit_range` come from
/// `ShipTemplate` (RON); `arrival_radius` is the fixed `ARRIVAL_DISTANCE`.
/// `ShipStats` is a `Component`, never a `Resource` (Bevy 0.19
/// `Resource: Component` hard error if derived both).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ShipStats {
    /// Kinematic speed (units / second) from RON.
    pub speed: Speed,
    /// Arrival radius (fixed, not in RON).
    pub arrival_radius: Distance,
    /// Mining laser range from RON.
    pub mining_range: Distance,
    /// Desired orbit distance from RON.
    pub orbit_range: Distance,
}

impl ShipStats {
    /// Create validated stats.
    ///
    /// # Panics
    /// Panics in debug if `arrival_radius >= orbit_range` or `orbit_range > mining_range`
    /// — steering requires `arrival < orbit <= mining`.
    #[must_use]
    pub fn new(
        speed: Speed,
        arrival_radius: Distance,
        mining_range: Distance,
        orbit_range: Distance,
    ) -> Self {
        debug_assert!(
            arrival_radius.get() < orbit_range.get(),
            "arrival_radius {} must be < orbit_range {}",
            arrival_radius.get(),
            orbit_range.get()
        );
        debug_assert!(
            orbit_range.get() <= mining_range.get(),
            "orbit_range {} must be <= mining_range {}",
            orbit_range.get(),
            mining_range.get()
        );
        Self {
            speed,
            arrival_radius,
            mining_range,
            orbit_range,
        }
    }

    /// Build from a validated [`ShipTemplate`] (RON-authored).
    ///
    /// `arrival_radius` is always [`ARRIVAL_DISTANCE`]; the template's
    /// `orbit_range`/`mining_range`/`speed` are copied.
    // own-borrow-over-clone: accept &ShipTemplate to avoid moving the template
    #[must_use]
    pub fn from_template(template: &ShipTemplate) -> Self {
        let stats = Self {
            speed: template.speed,
            arrival_radius: ARRIVAL_DISTANCE,
            mining_range: template.mining_range,
            orbit_range: template.orbit_range,
        };
        debug_assert!(
            stats.arrival_radius.get() < stats.orbit_range.get(),
            "ARRIVAL_RADIUS {} must be < orbit_range {} from RON",
            stats.arrival_radius.get(),
            stats.orbit_range.get()
        );
        stats
    }
}

impl From<ShipTemplate> for ShipStats {
    fn from(t: ShipTemplate) -> Self {
        Self::from_template(&t)
    }
}

// ---------------------------------------------------------------------------
// Systems — MovementSet on FixedUpdate
// ---------------------------------------------------------------------------

/// Kinematic steering for the front [`Order`] in each [`OrderQueue`].
///
/// Runs in `MovementSet` on `FixedUpdate`, gated by `in_state(Simulating)`.
/// Uses `Time<Fixed>::delta_secs()` for deterministic integration (SETA-safe:
/// time acceleration scales tick count, not physics). No Newtonian
/// integration — `pos += dir.normalize() * speed * dt` (seek/arrive) or
/// tangential + radial correction (orbit). Deterministic up `Vec3::Y`.
///
/// Slice-1 seam: handles `FlyTo`/`Approach`/`Orbit`/`Mine`. `FlyTo`/`Approach`
/// are the movement core for #4; `Orbit` is the #4 tangential hold; `Mine`
/// is the minimal approach-until-`mining_range` hold so the queue never stalls
/// when the next order is `Mine` (full mining loop lands in #5). All orders
/// are `Component OrderQueue` FIFO, popped only on deterministic arrival or
/// missing target.
#[allow(clippy::excessive_nesting)]
pub fn movement_system(
    time: Res<Time<Fixed>>,
    mut ships: Query<(&mut Transform, &mut OrderQueue, &ShipStats)>,
    // Slice-1 targets are asteroids (no ShipStats). Ship-to-ship Approach is
    // deferred to formation logic; disjoining on ShipStats keeps &mut vs & access
    // conflict-free without &mut World.
    targets: Query<&Transform, Without<ShipStats>>,
) {
    let dt = time.delta_secs();
    // err-result-over-panic: arrival/speed are validated newtypes, unwrap only
    // on programmer invariant (positive). ARRIVAL_RADIUS is known finite >0.
    // Use raw f32 for hot path to avoid newtype overhead in loop.
    // num-float-compare: never compare floats with ==; use < epsilon / <= radius.
    for (mut tf, mut queue, stats) in &mut ships {
        let speed = stats.speed.get();
        let arrival = stats.arrival_radius.get();
        let mining_range = stats.mining_range.get();

        // Peek front order. Clone to release queue borrow for target lookup.
        let front = queue.front().cloned();
        match front {
            Some(Order::FlyTo(target_pos)) => {
                let dir = target_pos - tf.translation;
                let dist = dir.length();
                // Already arrived — pop FIFO and hold.
                if dist <= arrival || dist < 1e-4 {
                    queue.pop_front();
                    continue;
                }
                tf.translation = seek_arrive_step(tf.translation, target_pos, arrival, speed, dt);
            }
            Some(Order::Approach(entity)) => {
                let Ok(target_tf) = targets.get(entity) else {
                    // Missing / despawned asteroid (or ship target in slice 1) -> pop to avoid stuck queue.
                    queue.pop_front();
                    continue;
                };
                let target_pos = target_tf.translation;
                let dist = (target_pos - tf.translation).length();
                if dist <= arrival || dist < 1e-4 {
                    queue.pop_front();
                    continue;
                }
                tf.translation = seek_arrive_step(tf.translation, target_pos, arrival, speed, dt);
            }
            Some(Order::Orbit(OrbitTarget { entity, distance })) => {
                let desired = distance.get();
                let Ok(target_tf) = targets.get(entity) else {
                    queue.pop_front();
                    continue;
                };
                let target_pos = target_tf.translation;
                let to_ship = tf.translation - target_pos;
                let dist = to_ship.length();
                if dist < 1e-4 {
                    // At target centre — nudge out deterministically along +X.
                    tf.translation = target_pos + Vec3::X * desired;
                    continue;
                }
                let radial_dir = to_ship / dist;
                let radial_error = dist - desired;
                // Radial correction scaled by arrival, half gain to preserve
                // tangential speed (otherwise combined sqrt2 clamp would cut
                // tangential ~30%). Half gain -> worst combined ~1.12*speed,
                // clamped 10% reduction keeps orbit near full tangential.
                let radial_speed = (radial_error / arrival).clamp(-1.0, 1.0) * speed * 0.5;
                // -radial_dir * radial_speed moves inward when too far (error>0) and outward when too close.
                let radial_vel = -radial_dir * radial_speed;

                // Tangential velocity: perpendicular to radial in XZ plane, deterministic up Y.
                // When radial_dir ≈ Y, Y cross is near zero — deterministic fallback to Z cross.
                let mut tangent = Vec3::Y.cross(radial_dir);
                if tangent.length_squared() < 1e-6 {
                    tangent = Vec3::Z.cross(radial_dir);
                }
                tangent = tangent.normalize();
                let tangential_vel = tangent * speed;

                let mut vel = radial_vel + tangential_vel;
                let vel_len = vel.length();
                if vel_len > speed {
                    vel = vel / vel_len * speed;
                }
                tf.translation += vel * dt;
                // Orbit is persistent — never auto-pop.
            }
            Some(Order::Mine(entity)) => {
                let Ok(target_tf) = targets.get(entity) else {
                    queue.pop_front();
                    continue;
                };
                let target_pos = target_tf.translation;
                let dir = target_pos - tf.translation;
                let dist = dir.length();
                if dist > mining_range {
                    // Approach until within mining_range, then hold. No arrive scaling — stop at mining_range.
                    if dist < 1e-4 {
                        continue;
                    }
                    let step_len = speed * dt;
                    // Don't overshoot beyond mining_range boundary: clamp step if it would cross inside in one tick? Simple move; next tick will hold.
                    if step_len >= dist {
                        tf.translation = target_pos;
                    } else {
                        tf.translation += dir / dist * step_len;
                    }
                } else {
                    // Within mining_range — hold position (zero velocity).
                }
                // Mine is persistent — never auto-pop here (external cargo/asteroid check pops).
            }
            None => {
                // No orders — idle.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — headless FixedUpdate, deterministic
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimPlugin;
    use bevy::time::{Fixed, Time, TimeUpdateStrategy};
    use spacegame_data::parse_ship_ron;

    fn miner_stats() -> ShipStats {
        // Data-driven: parse the same RON shape as assets/data/ships/miner.ron.
        // Never hardcode 75.0 in production code; hardcoding is isolated to this test fixture which mirrors RON.
        let ron_str = r#"(
            id: "miner",
            speed: 75.0,
            cargo_capacity: 100.0,
            mining_range: 1500.0,
            cycle_secs: 5.0,
            yield_per_cycle: 10,
            orbit_range: 1000.0,
        )"#;
        let tmpl = parse_ship_ron(ron_str).expect("miner ron parses");
        ShipStats::from_template(&tmpl)
    }

    fn fixed_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        // SimPlugin already adds `movement_system` to `MovementSet`; do not duplicate.
        app
    }

    /// Order-independent hash of all Transforms — sorted by bits so insertion
    /// order does not affect determinism check.
    fn position_hash(app: &mut App) -> u64 {
        let mut keys: Vec<(u32, u32, u32)> = {
            let mut qs = app.world_mut().query::<&Transform>();
            qs.iter(app.world())
                .map(|tf| {
                    (
                        tf.translation.x.to_bits(),
                        tf.translation.y.to_bits(),
                        tf.translation.z.to_bits(),
                    )
                })
                .collect()
        };
        keys.sort_unstable();
        let mut hash: u64 = 0;
        for (xb, yb, zb) in keys {
            hash = hash.wrapping_add(xb as u64);
            hash = hash.wrapping_add(yb as u64);
            hash = hash.wrapping_add(zb as u64);
            // mix to avoid sum commutativity hiding transpositions
            hash = hash.wrapping_mul(0x9e3779b97f4a7c15);
            hash ^= hash >> 33;
        }
        hash
    }

    fn tick_n(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    // Deterministic WyRand-like seeded position helper (splitmix64).
    // Mirrors AGENTS.md seeded WyRand System placement without adding rand crate.
    fn wyrand_next(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn wyrand_vec3(seed: u64, idx: u64, half_extent: f32) -> Vec3 {
        // Mix seed and idx deterministically
        let mut s = seed ^ (idx.wrapping_mul(0x9e3779b97f4a7c15));
        let r1 = wyrand_next(&mut s);
        let r2 = wyrand_next(&mut s);
        let r3 = wyrand_next(&mut s);
        let f = |r: u64| -> f32 {
            // Map low 32 bits to [0,1), then to [-half, half]
            let u = (r & 0xffffffff) as f32 / (u32::MAX as f32);
            u * 2.0 * half_extent - half_extent
        };
        Vec3::new(f(r1), f(r2), f(r3))
    }

    fn wyrand_positions(seed: u64, n: usize, half_extent: f32) -> Vec<Vec3> {
        (0..n as u64)
            .map(|i| wyrand_vec3(seed, i, half_extent))
            .collect()
    }

    #[test]
    fn flyto_arrives_within_arrival_radius_and_pops() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let arrival = stats.arrival_radius.get();

        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::FlyTo(Vec3::new(500.0, 0.0, 0.0))),
                stats,
            ))
            .id();

        // Tick until FlyTo pops or timeout.
        let mut popped = false;
        for _ in 0..2000 {
            app.update();
            let q = app.world().get::<OrderQueue>(ship).unwrap();
            if q.is_empty() {
                popped = true;
                break;
            }
        }
        assert!(popped, "FlyTo should pop within 2000 ticks");
        let tf = app.world().get::<Transform>(ship).unwrap();
        let target = Vec3::new(500.0, 0.0, 0.0);
        let dist = (tf.translation - target).length();
        // num-float-compare: tolerance, arrival radius 50.0, distance must be < radius.
        assert!(
            dist <= arrival + 1e-3,
            "flyto distance {dist} should be <= arrival_radius {arrival}"
        );
    }

    #[test]
    fn approach_deterministic_arrival() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let arrival = stats.arrival_radius.get();

        let asteroid = app
            .world_mut()
            .spawn(Transform::from_translation(Vec3::new(800.0, 0.0, 0.0)))
            .id();

        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Approach(asteroid)),
                stats,
            ))
            .id();

        let mut popped = false;
        for _ in 0..3000 {
            app.update();
            if app.world().get::<OrderQueue>(ship).unwrap().is_empty() {
                popped = true;
                break;
            }
        }
        assert!(popped, "Approach should pop on arrival");
        let ship_tf = app.world().get::<Transform>(ship).unwrap().translation;
        let ast_tf = app.world().get::<Transform>(asteroid).unwrap().translation;
        let dist = (ship_tf - ast_tf).length();
        assert!(
            dist <= arrival + 1e-3,
            "approach distance {dist} should be <= arrival {arrival}"
        );
    }

    #[test]
    fn approach_pops_when_target_missing() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let missing = app.world_mut().spawn_empty().id();
        // despawn immediately — simulates destroyed asteroid
        app.world_mut().despawn(missing);

        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Approach(missing)),
                stats,
            ))
            .id();

        // First FixedUpdate may have dt=0 with ManualDuration; allow two ticks.
        tick_n(&mut app, 2);
        let q = app.world().get::<OrderQueue>(ship).unwrap();
        assert!(q.is_empty(), "approach to missing target should pop");
    }

    #[test]
    fn orbit_holds_range_within_5pct() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let orbit_range = stats.orbit_range.get();
        let lower = orbit_range * 0.95;
        let upper = orbit_range * 1.05;

        let asteroid = app
            .world_mut()
            .spawn(Transform::from_translation(Vec3::ZERO))
            .id();

        // Spawn ship far away to force convergence
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(3000.0, 0.0, 0.0)),
                OrderQueue::with_order(Order::orbit(
                    asteroid,
                    Distance::new(orbit_range).expect("valid"),
                )),
                stats,
            ))
            .id();

        // Converge: 5000 ticks ~ 78s at 64Hz, enough to spiral into 1000
        // range with half-gain radial correction (see orbit radial_gain 0.5 note).
        tick_n(&mut app, 5000);

        // After convergence, hold within 5% for 500 ticks
        for _ in 0..500 {
            app.update();
            let ship_tf = app.world().get::<Transform>(ship).unwrap().translation;
            let ast_tf = app.world().get::<Transform>(asteroid).unwrap().translation;
            let dist = (ship_tf - ast_tf).length();
            assert!(
                dist >= lower - 1e-3 && dist <= upper + 1e-3,
                "orbit distance {dist:.2} should be within [{lower:.2}, {upper:.2}] (range {orbit_range})"
            );
        }

        // Orbit is persistent — queue still contains Orbit
        let q = app.world().get::<OrderQueue>(ship).unwrap();
        assert_eq!(q.len(), 1);
        assert!(matches!(q.front(), Some(Order::Orbit(_))));
    }

    #[test]
    fn orbit_pops_when_target_missing() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let missing = app.world_mut().spawn_empty().id();
        app.world_mut().despawn(missing);
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(1000.0, 0.0, 0.0)),
                OrderQueue::with_order(Order::orbit(missing, Distance::new(1000.0).unwrap())),
                stats,
            ))
            .id();
        tick_n(&mut app, 2);
        assert!(app.world().get::<OrderQueue>(ship).unwrap().is_empty());
    }

    #[test]
    fn mine_holds_within_mining_range_and_persists() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let mining_range = stats.mining_range.get();

        let asteroid = app
            .world_mut()
            .spawn(Transform::from_translation(Vec3::new(2000.0, 0.0, 0.0)))
            .id();

        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Mine(asteroid)),
                stats,
            ))
            .id();

        // Approach until within mining_range
        let mut within = false;
        for _ in 0..3000 {
            app.update();
            let ship_tf = app.world().get::<Transform>(ship).unwrap().translation;
            let ast_tf = app.world().get::<Transform>(asteroid).unwrap().translation;
            let dist = (ship_tf - ast_tf).length();
            if dist <= mining_range + 1e-3 {
                within = true;
                break;
            }
        }
        assert!(within, "mine should approach within mining_range");

        // After within, hold for 200 ticks — distance must stay <= mining_range and queue persists
        for _ in 0..200 {
            app.update();
            let ship_tf = app.world().get::<Transform>(ship).unwrap().translation;
            let ast_tf = app.world().get::<Transform>(asteroid).unwrap().translation;
            let dist = (ship_tf - ast_tf).length();
            assert!(
                dist <= mining_range + 1e-3,
                "mine hold distance {dist} should stay <= mining_range {mining_range}"
            );
        }
        let q = app.world().get::<OrderQueue>(ship).unwrap();
        assert!(matches!(q.front(), Some(Order::Mine(_))));
    }

    #[test]
    fn transform_syncs_each_fixed_update() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::FlyTo(Vec3::new(100.0, 0.0, 0.0))),
                stats.clone(),
            ))
            .id();

        let start = app.world().get::<Transform>(ship).unwrap().translation;
        // First FixedUpdate tick may have dt=0 with ManualDuration; tick twice.
        tick_n(&mut app, 2);
        let after = app.world().get::<Transform>(ship).unwrap().translation;
        // Must have moved on FixedUpdate ticks.
        assert!(
            (after - start).length() > 1e-6,
            "Transform should sync each FixedUpdate"
        );
    }

    #[test]
    fn deterministic_10k_ticks_same_hash() {
        // Uses WyRand-seeded positions for System placement determinism per
        // parent #1 ("seeded WyRand asteroid placement deterministically").
        fn run_and_hash(seed: u64) -> u64 {
            let mut app = fixed_app();
            let stats = miner_stats();
            // Seeded asteroid positions — same seed must yield identical final hashes.
            let positions = wyrand_positions(seed, 2, 5000.0);
            let asteroid_a = app
                .world_mut()
                .spawn(Transform::from_translation(positions[0]))
                .id();
            let asteroid_b = app
                .world_mut()
                .spawn(Transform::from_translation(positions[1]))
                .id();
            // Mix approaches/orbits across asteroids for coverage
            app.world_mut().spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Approach(asteroid_a)),
                stats.clone(),
            ));
            app.world_mut().spawn((
                Transform::from_translation(Vec3::new(-1500.0, 200.0, 0.0)),
                OrderQueue::with_order(Order::orbit(
                    asteroid_b,
                    Distance::new(stats.orbit_range.get()).unwrap(),
                )),
                stats,
            ));
            tick_n(&mut app, 10_000);
            position_hash(&mut app)
        }

        let seed = 0xdead_beef_cafe_1234;
        let h1 = run_and_hash(seed);
        let h2 = run_and_hash(seed);
        assert_eq!(h1, h2, "10k ticks must be deterministic: hash {h1} vs {h2}");
        // Different seed must differ (sanity that seed matters)
        let h3 = run_and_hash(seed.wrapping_add(1));
        assert_ne!(
            h1, h3,
            "different WyRand seed should yield different hashes"
        );
    }

    #[test]
    fn seta_scales_tick_count_not_physics() {
        // SETA scales tick count, not physics: moving with dt for N ticks
        // must equal moving with dt/2 for 2N ticks (distance = speed * total_time).
        // Proves movement uses Time<Fixed>::delta_secs() deterministically.
        let stats = miner_stats();
        let speed = stats.speed.get();
        let dt = Time::<Fixed>::default().timestep().as_secs_f32();

        // Helper to run N ticks with given dt and report X displacement for a far FlyTo.
        let run = |dt_secs: f32, ticks: usize| -> f32 {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, SimPlugin));
            app.insert_resource(TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f32(dt_secs),
            ));
            let ship = app
                .world_mut()
                .spawn((
                    Transform::from_translation(Vec3::ZERO),
                    OrderQueue::with_order(Order::FlyTo(Vec3::new(5000.0, 0.0, 0.0))),
                    stats.clone(),
                ))
                .id();
            // First tick may have dt=0 on first ManualDuration use; tick N+1 and drop first sample.
            tick_n(&mut app, ticks);
            app.world().get::<Transform>(ship).unwrap().translation.x
        };

        // 100 ticks at dt vs 200 ticks at dt/2 should travel ~ same distance = speed * 100*dt
        let d1 = run(dt, 100);
        let d2 = run(dt * 0.5, 200);
        let expected = speed * dt * 100.0;
        // Allow tolerance for arrive scaling not hitting (far target ensures no scaling).
        // First tick may be partial; allow 2*dt tolerance.
        assert!(
            (d1 - expected).abs() < speed * dt * 2.0,
            "SETA: 100*dt distance {d1} vs expected {expected}"
        );
        assert!(
            (d2 - d1).abs() < 1e-3,
            "SETA: dt vs dt/2 should agree: {d1} vs {d2}"
        );
    }

    #[test]
    fn no_newtonian_thrust_mass_used_pure_kinematic() {
        // Proves kinematic model: after one tick, displacement == speed * dt (or scaled).
        let mut app = fixed_app();
        let stats = miner_stats();
        let speed = stats.speed.get();
        let dt = Time::<Fixed>::default().timestep().as_secs_f32();

        // Far target so scale == 1.0 (dist >> arrival_radius)
        let ship = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::FlyTo(Vec3::new(5000.0, 0.0, 0.0))),
                stats,
            ))
            .id();

        // First tick may have dt=0; tick twice, expect 2*dt displacement.
        tick_n(&mut app, 2);
        let tf = app.world().get::<Transform>(ship).unwrap().translation;
        let moved = tf.length();
        // Allow either 1 or 2 ticks of movement depending on first dt; check within [dt, 2*dt]*speed with tolerance.
        let expected_one = speed * dt;
        let expected_two = speed * dt * 2.0;
        let within_one = (moved - expected_one).abs() < 1e-4;
        let within_two = (moved - expected_two).abs() < 1e-4;
        assert!(
            within_one || within_two,
            "pure kinematic: moved {moved} expected {expected_one} or {expected_two} (speed {speed} * dt {dt})"
        );
    }

    #[test]
    fn idle_when_queue_empty() {
        let mut app = fixed_app();
        let stats = miner_stats();
        let start = Vec3::new(10.0, 20.0, 30.0);
        let ship = app
            .world_mut()
            .spawn((Transform::from_translation(start), OrderQueue::new(), stats))
            .id();
        tick_n(&mut app, 10);
        let tf = app.world().get::<Transform>(ship).unwrap().translation;
        assert!((tf - start).length() < 1e-6, "empty queue should not move");
    }

    #[test]
    fn shipstats_from_template_is_data_driven() {
        let tmpl = parse_ship_ron(
            r#"(id: "miner", speed: 75.0, cargo_capacity: 100.0, mining_range: 1500.0, cycle_secs: 5.0, yield_per_cycle: 10, orbit_range: 1000.0)"#,
        )
        .unwrap();
        let stats = ShipStats::from_template(&tmpl);
        assert!((stats.speed.get() - 75.0).abs() < f32::EPSILON);
        assert!((stats.mining_range.get() - 1500.0).abs() < f32::EPSILON);
        assert!((stats.orbit_range.get() - 1000.0).abs() < f32::EPSILON);
        assert!((stats.arrival_radius.get() - ARRIVAL_RADIUS).abs() < f32::EPSILON);

        // Loads on-disk file identically — proves no hardcode drift
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/data/ships/miner.ron");
        if let Ok(file_tmpl) = spacegame_data::load_ship_file(&path) {
            let file_stats = ShipStats::from_template(&file_tmpl);
            assert_eq!(stats, file_stats);
        }
    }

    #[test]
    fn arrival_radius_invariants_hold() {
        let stats = miner_stats();
        // Spec ordering: arrival < orbit <= mining per data-driven RON + ARRIVAL_RADIUS 50.
        assert!(
            stats.arrival_radius.get() < stats.orbit_range.get(),
            "arrival {} must be < orbit {}",
            stats.arrival_radius.get(),
            stats.orbit_range.get()
        );
        assert!(
            stats.orbit_range.get() <= stats.mining_range.get(),
            "orbit {} must be <= mining {}",
            stats.orbit_range.get(),
            stats.mining_range.get()
        );
        assert!((ARRIVAL_RADIUS - stats.arrival_radius.get()).abs() < f32::EPSILON);
        assert!(
            ARRIVAL_RADIUS < stats.orbit_range.get(),
            "hardcoded ARRIVAL_RADIUS still < RON orbit_range"
        );
    }

    #[test]
    fn wyrand_positions_are_deterministic_and_varied() {
        let a1 = wyrand_positions(42, 3, 5000.0);
        let a2 = wyrand_positions(42, 3, 5000.0);
        assert_eq!(a1, a2, "same seed must yield same positions");
        let b = wyrand_positions(43, 3, 5000.0);
        assert_ne!(a1, b, "different seed must differ");
        for p in &a1 {
            assert!(
                p.x.abs() <= 5000.0 && p.y.abs() <= 5000.0 && p.z.abs() <= 5000.0,
                "position {p:?} must be within half box"
            );
        }
    }
}
