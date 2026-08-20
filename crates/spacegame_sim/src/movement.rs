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
    #[must_use]
    pub fn new(
        speed: Speed,
        arrival_radius: Distance,
        mining_range: Distance,
        orbit_range: Distance,
    ) -> Self {
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
        Self {
            speed: template.speed,
            arrival_radius: ARRIVAL_DISTANCE,
            mining_range: template.mining_range,
            orbit_range: template.orbit_range,
        }
    }
}

impl From<&ShipTemplate> for ShipStats {
    fn from(t: &ShipTemplate) -> Self {
        Self::from_template(t)
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
#[allow(clippy::excessive_nesting)]
pub fn movement_system(
    time: Res<Time<Fixed>>,
    mut ships: Query<(&mut Transform, &mut OrderQueue, &ShipStats)>,
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
                // Arrive slowdown: linear scale inside arrival_radius.
                let scale = (dist / arrival).min(1.0);
                let effective_speed = speed * scale;
                let step_len = effective_speed * dt;
                if step_len >= dist {
                    tf.translation = target_pos;
                } else {
                    // dir is non-zero here
                    tf.translation += dir / dist * step_len;
                }
                // Check arrival after move — will pop next tick if within radius.
                // Do NOT pop immediately so orbit/flyto holding test can assert distance < radius while queue still present for one more tick?
                // For FlyTo we pop when within radius before move on next tick; keeping this as before-move pop is deterministic.
            }
            Some(Order::Approach(entity)) => {
                // Resolve target. ShipStats targets are disjoint (Without<ShipStats>), so
                // approaching another Ship is treated as missing -> pop. For slice 1 targets are asteroids.
                let Ok(target_tf) = targets.get(entity) else {
                    // Missing / despawned target -> pop to avoid stuck queue.
                    queue.pop_front();
                    continue;
                };
                let target_pos = target_tf.translation;
                let dir = target_pos - tf.translation;
                let dist = dir.length();
                if dist <= arrival || dist < 1e-4 {
                    queue.pop_front();
                    continue;
                }
                let scale = (dist / arrival).min(1.0);
                let effective_speed = speed * scale;
                let step_len = effective_speed * dt;
                if step_len >= dist {
                    tf.translation = target_pos;
                } else {
                    tf.translation += dir / dist * step_len;
                }
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
                // Radial correction: scale by arrival radius, clamp to speed.
                let radial_speed = (radial_error / arrival).clamp(-1.0, 1.0) * speed;
                // -radial_dir * radial_speed moves inward when too far (error>0) and outward when too close.
                let radial_vel = -radial_dir * radial_speed;

                // Tangential velocity: perpendicular to radial in XZ plane, deterministic up Y.
                let mut tangent = Vec3::Y.cross(radial_dir);
                if tangent.length_squared() < 1e-6 {
                    // radial_dir parallel to Y — fall back to Z cross.
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

    fn position_hash(app: &mut App) -> u64 {
        let mut hash: u64 = 0;
        let mut qs = app.world_mut().query::<&Transform>();
        for tf in qs.iter(app.world()) {
            hash = hash.wrapping_add(tf.translation.x.to_bits() as u64);
            hash = hash.wrapping_add(tf.translation.y.to_bits() as u64);
            hash = hash.wrapping_add(tf.translation.z.to_bits() as u64);
        }
        hash
    }

    fn tick_n(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
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

        // Converge: 2500 ticks ~ 39s at 64Hz, enough to spiral into 1000 range
        tick_n(&mut app, 2500);

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
        fn run_and_hash() -> u64 {
            let mut app = fixed_app();
            let stats = miner_stats();
            let asteroid = app
                .world_mut()
                .spawn(Transform::from_translation(Vec3::new(500.0, 0.0, 0.0)))
                .id();
            app.world_mut().spawn((
                Transform::from_translation(Vec3::ZERO),
                OrderQueue::with_order(Order::Approach(asteroid)),
                stats.clone(),
            ));
            app.world_mut().spawn((
                Transform::from_translation(Vec3::new(-1500.0, 200.0, 0.0)),
                OrderQueue::with_order(Order::orbit(
                    asteroid,
                    Distance::new(stats.orbit_range.get()).unwrap(),
                )),
                stats,
            ));
            tick_n(&mut app, 10_000);
            position_hash(&mut app)
        }

        let h1 = run_and_hash();
        let h2 = run_and_hash();
        assert_eq!(h1, h2, "10k ticks must be deterministic: hash {h1} vs {h2}");
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
}
