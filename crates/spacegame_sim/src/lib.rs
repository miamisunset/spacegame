//! Deterministic headless simulation — no bevy_render / bevy_pbr dependency.
//! Runs on FixedUpdate, seeded WyRand, data-driven via spacegame_data RON templates.
//!
//! Slice 1 tracer: FIFO [`OrderQueue`] ticking on [`FixedUpdate`] with
//! [`GameState::Simulating`] gating and ordered [`SystemSet`]s.

use bevy::prelude::*;
use spacegame_data::Distance;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// GameState
// ---------------------------------------------------------------------------

/// Global simulation state. The sim Systems only tick in [`GameState::Simulating`].
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Deterministic simulation is running on `FixedUpdate`.
    #[default]
    Simulating,
    /// Paused — used to prove `in_state` gating in tests.
    Paused,
}

// ---------------------------------------------------------------------------
// SystemSets — ordered on FixedUpdate per AGENTS.md Simulation Conventions
// ---------------------------------------------------------------------------

/// Economy tick (market, inventory). Ordered first.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EconomySet;

/// AI tick (orders, autonomy). After economy.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AiSet;

/// Kinematic steering (seek/arrive + orbit). After AI.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovementSet;

/// Mining cycles (range-checked, fatigue-scaled). After movement so range is fresh.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MiningSet;

/// Combat (deferred in slice 1). Ordered last.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

/// CEO-issued strategic instruction. FIFO in [`OrderQueue`]; only the front
/// order ticks on `FixedUpdate` until completion then pops. `Mine` loops
/// while in range until cargo full or asteroid destroyed.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// Move to a point in system space.
    FlyTo(Vec3),
    /// Close distance to an entity (e.g. asteroid).
    Approach(Entity),
    /// Hold tangential velocity at `orbit_range` around an entity.
    Orbit(Entity, Distance),
    /// Persistent looping mine of an entity while in `mining_range`.
    Mine(Entity),
}

/// FIFO queue of [`Order`]s on a ship. `Mine` persists until external
/// conditions (cargo full or asteroid despawned) pop it.
///
/// FIFO is compiler-enforced: the inner [`VecDeque`] is private; only
/// [`OrderQueue::push_back`] / [`OrderQueue::pop_front`] / [`OrderQueue::advance_if`]
/// can mutate ordering. Reads go through [`OrderQueue::front`] / [`OrderQueue::get`]
/// / [`OrderQueue::iter`].
#[derive(Debug, Clone, PartialEq, Component, Default)]
pub struct OrderQueue {
    orders: VecDeque<Order>,
}

impl OrderQueue {
    /// Empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            orders: VecDeque::new(),
        }
    }

    /// Queue with a single order.
    #[must_use]
    pub fn with_order(order: Order) -> Self {
        let mut q = Self::new();
        q.push_back(order);
        q
    }

    /// Push an order to the back (FIFO).
    pub fn push_back(&mut self, order: Order) {
        self.orders.push_back(order);
    }

    /// Pop and return the front order.
    pub fn pop_front(&mut self) -> Option<Order> {
        self.orders.pop_front()
    }

    /// Peek at the front order without popping.
    #[must_use]
    pub fn front(&self) -> Option<&Order> {
        self.orders.front()
    }

    /// Number of queued orders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Clear all orders.
    pub fn clear(&mut self) {
        self.orders.clear();
    }

    /// Whether the front order is a persistent `Mine`.
    #[must_use]
    pub fn is_mining(&self) -> bool {
        matches!(self.front(), Some(Order::Mine(_)))
    }

    /// Read-only access to the order at `index` (FIFO order).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Order> {
        self.orders.get(index)
    }

    /// Iterate over queued orders in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter()
    }

    /// Convenience: pop front if `predicate` returns true for the front order.
    /// `Mine` callers should gate on cargo/asteroid state before calling.
    pub fn advance_if(&mut self, predicate: impl FnOnce(&Order) -> bool) -> Option<Order> {
        if self.front().is_some_and(predicate) {
            self.pop_front()
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Deterministic sim plugin. Headless-friendly — no `bevy_render` dependency.
///
/// Registers [`GameState`] and configures ordered [`SystemSet`]s on
/// [`FixedUpdate`] gated by `in_state(GameState::Simulating)`.
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // `MinimalPlugins` does not include `StatesPlugin`; `init_state` requires the
        // `StateTransition` schedule. Install `StatesPlugin` idempotently — check the
        // schedule itself so transitive `DefaultPlugins` inclusion doesn't double-add.
        if app
            .get_schedule(bevy::state::prelude::StateTransition)
            .is_none()
        {
            app.add_plugins(bevy::state::app::StatesPlugin);
        }
        app.init_state::<GameState>();
        // Chain ordering per AGENTS.md: Economy -> Ai -> Movement -> Mining -> Combat.
        // All sets tick on FixedUpdate deterministically; SETA scales tick count.
        app.configure_sets(
            FixedUpdate,
            (EconomySet, AiSet, MovementSet, MiningSet, CombatSet)
                .chain()
                .run_if(in_state(GameState::Simulating)),
        );
    }
}

// ---------------------------------------------------------------------------
// Tests — headless queue transitions + gating
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_entities(n: usize, app: &mut App) -> Vec<Entity> {
        (0..n).map(|_| app.world_mut().spawn_empty().id()).collect()
    }

    #[test]
    fn order_queue_push_four_fifo_ordered() {
        // Arrange — push FlyTo -> Approach -> Orbit -> Mine
        let mut app = App::new();
        let entities = dummy_entities(3, &mut app);
        let mut q = OrderQueue::new();

        q.push_back(Order::FlyTo(Vec3::new(1.0, 0.0, 0.0)));
        q.push_back(Order::Approach(entities[0]));
        q.push_back(Order::Orbit(
            entities[1],
            Distance::new(1000.0).expect("valid distance"),
        ));
        q.push_back(Order::Mine(entities[2]));

        // Assert — FIFO order preserved (via read-only accessors; FIFO mutation is private)
        assert_eq!(q.len(), 4);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(1.0, 0.0, 0.0))));
        assert!(matches!(q.get(1), Some(Order::Approach(_))));
        assert!(matches!(q.get(2), Some(Order::Orbit(_, _))));
        assert!(matches!(q.get(3), Some(Order::Mine(_))));
        assert_eq!(q.iter().count(), 4);
        assert!(!q.is_mining(), "front is FlyTo, not Mine");
    }

    #[test]
    fn order_queue_pop_on_completion_advances_front() {
        let mut app = App::new();
        let entities = dummy_entities(2, &mut app);
        let mut q = OrderQueue::new();
        q.push_back(Order::FlyTo(Vec3::ZERO));
        q.push_back(Order::Approach(entities[0]));
        q.push_back(Order::Mine(entities[1]));

        // Simulate FlyTo completed -> pop front
        let popped = q.pop_front();
        assert_eq!(popped, Some(Order::FlyTo(Vec3::ZERO)));
        assert_eq!(q.front(), Some(&Order::Approach(entities[0])));
        assert_eq!(q.len(), 2);

        // Next completes -> pop
        q.pop_front();
        assert_eq!(q.front(), Some(&Order::Mine(entities[1])));
        assert!(q.is_mining());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn order_queue_mine_persists_until_external_pop() {
        let mut app = App::new();
        let e = dummy_entities(1, &mut app)[0];
        let mut q = OrderQueue::with_order(Order::Mine(e));

        // Mine persists across "ticks" — advance_if with non-matching predicate does not pop
        assert!(q.is_mining());
        let not_popped = q.advance_if(|o| matches!(o, Order::FlyTo(_)));
        assert!(not_popped.is_none());
        assert_eq!(q.len(), 1);
        assert!(q.is_mining());

        // External condition (cargo full or asteroid destroyed) pops it
        let popped = q.advance_if(|o| matches!(o, Order::Mine(_)));
        assert_eq!(popped, Some(Order::Mine(e)));
        assert!(q.is_empty());
        assert!(!q.is_mining());
    }

    #[test]
    fn order_queue_clear_and_is_empty() {
        let mut q = OrderQueue::new();
        q.push_back(Order::FlyTo(Vec3::ONE));
        q.push_back(Order::FlyTo(Vec3::ZERO));
        assert_eq!(q.len(), 2);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.front(), None);
    }

    #[test]
    fn gamestate_default_is_simulating_headless() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));

        // Initial state is Simulating per Default
        app.update();
        let state = app.world().resource::<State<GameState>>();
        assert_eq!(**state, GameState::Simulating);

        // Can transition to Paused via NextState
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update();
        let state = app.world().resource::<State<GameState>>();
        assert_eq!(**state, GameState::Paused);
    }

    #[test]
    fn fixed_update_sets_only_tick_in_simulating() {
        // Prove `in_state(GameState::Simulating)` gating on `FixedUpdate`.
        // Uses `TimeUpdateStrategy::ManualDuration` for deterministic SETA-like
        // ticking: each `app.update()` advances virtual time by exactly one
        // fixed timestep, so `FixedUpdate` runs once per update while Simulating
        // and zero times when Paused. Delta-based assertions tolerate the
        // initial startup tick.
        use bevy::time::{Fixed, Time, TimeUpdateStrategy};

        #[derive(Resource, Default, Debug, PartialEq, Eq)]
        struct TickCount(u32);

        fn counting_system(mut c: ResMut<TickCount>) {
            c.0 += 1;
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        app.init_resource::<TickCount>();
        // Put counting_system in MovementSet so it inherits the set's gating.
        app.add_systems(FixedUpdate, counting_system.in_set(MovementSet));
        // Warm up startup + state init; capture baseline.
        app.update();
        let base = app.world().resource::<TickCount>().0;

        // Simulating -> ticks (one FixedUpdate per app.update)
        for _ in 0..3 {
            app.update();
        }
        let count_sim = app.world().resource::<TickCount>().0 - base;
        assert_eq!(
            count_sim, 3,
            "should have ticked 3 times in Simulating, got {count_sim}"
        );

        // Transition to Paused -> no further ticks. This `app.update()` applies the
        // `StateTransition` schedule and may still tick once in `Simulating` before
        // the state flips; `prev` is captured *after* that transitional tick so
        // the following assertion measures ticks only while `Paused`.
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update(); // apply StateTransition (may tick once more in Simulating)
        let prev = app.world().resource::<TickCount>().0;
        for _ in 0..3 {
            app.update();
        }
        let after = app.world().resource::<TickCount>().0;
        assert_eq!(
            prev, after,
            "should not tick in Paused (prev {prev}, after {after})"
        );
    }

    #[test]
    fn order_queue_component_spawn_and_query_headless() {
        // Verify OrderQueue is a valid Component that can be queried headless
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));

        let e = app.world_mut().spawn(OrderQueue::new()).id();
        app.world_mut()
            .entity_mut(e)
            .get_mut::<OrderQueue>()
            .unwrap()
            .push_back(Order::FlyTo(Vec3::new(5.0, 0.0, 0.0)));

        app.update();

        let q = app.world().get::<OrderQueue>(e).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(5.0, 0.0, 0.0))));
    }

    #[test]
    fn order_queue_fifo_survives_fixed_update_ticks() {
        // Proves FIFO survives *scheduled* `FixedUpdate` ticks, not manual `pop_front()`.
        // A pop system runs in `AiSet` (gated by `Simulating`) and deterministically
        // pops one order per `FixedUpdate` frame via `ManualDuration`.
        use bevy::time::{Fixed, Time, TimeUpdateStrategy};

        fn pop_front_system(mut query: Query<&mut OrderQueue>) {
            for mut q in &mut query {
                q.pop_front();
            }
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        app.add_systems(FixedUpdate, pop_front_system.in_set(AiSet));

        let mut queue = OrderQueue::new();
        for i in 0..10 {
            queue.push_back(Order::FlyTo(Vec3::new(i as f32, 0.0, 0.0)));
        }
        let entity = app.world_mut().spawn(queue).id();

        // Warm up startup; then tick 5 FixedUpdate frames (one pop per frame).
        app.update();
        for _ in 0..5 {
            app.update();
        }

        let q = app.world().get::<OrderQueue>(entity).unwrap();
        assert_eq!(q.len(), 5);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(5.0, 0.0, 0.0))));
        // Full FIFO order is intact via `get`/`iter`.
        for (idx, order) in q.iter().enumerate() {
            assert_eq!(
                *order,
                Order::FlyTo(Vec3::new((idx as f32) + 5.0, 0.0, 0.0))
            );
        }
    }

    #[test]
    fn system_sets_chain_in_fixed_update_order() {
        // Verifies the `FixedUpdate: (Economy->Ai->Movement->Mining->Combat).chain()`
        // ordering is deterministic: systems in earlier sets run before later sets
        // within the same `FixedUpdate` tick.
        use bevy::time::{Fixed, Time, TimeUpdateStrategy};

        #[derive(Resource, Default)]
        struct OrderLog(Vec<&'static str>);

        fn economy_sys(mut log: ResMut<OrderLog>) {
            log.0.push("economy");
        }
        fn ai_sys(mut log: ResMut<OrderLog>) {
            log.0.push("ai");
        }
        fn movement_sys(mut log: ResMut<OrderLog>) {
            log.0.push("movement");
        }
        fn mining_sys(mut log: ResMut<OrderLog>) {
            log.0.push("mining");
        }
        fn combat_sys(mut log: ResMut<OrderLog>) {
            log.0.push("combat");
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        app.init_resource::<OrderLog>();
        app.add_systems(FixedUpdate, economy_sys.in_set(EconomySet));
        app.add_systems(FixedUpdate, ai_sys.in_set(AiSet));
        app.add_systems(FixedUpdate, movement_sys.in_set(MovementSet));
        app.add_systems(FixedUpdate, mining_sys.in_set(MiningSet));
        app.add_systems(FixedUpdate, combat_sys.in_set(CombatSet));

        app.update(); // warm up startup
        app.world_mut().resource_mut::<OrderLog>().0.clear();
        app.update(); // one deterministic FixedUpdate tick

        let log = app.world().resource::<OrderLog>().0.clone();
        assert_eq!(log, vec!["economy", "ai", "movement", "mining", "combat"]);
    }
}
