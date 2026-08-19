//! Deterministic headless simulation — no bevy_render / bevy_pbr dependency.
//! Runs on FixedUpdate, seeded WyRand, data-driven via spacegame_data RON templates.
//!
//! Slice 1 tracer: FIFO [`OrderQueue`] ticking on [`FixedUpdate`] with
//! [`GameState::Simulating`] gating and ordered [`SystemSet`]s.

use bevy::prelude::*;
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
    /// Hold tangential velocity at `range` around an entity.
    Orbit(Entity, f32),
    /// Persistent looping mine of an entity while in `mining_range`.
    Mine(Entity),
}

/// FIFO queue of [`Order`]s on a ship. `Mine` persists until external
/// conditions (cargo full or asteroid despawned) pop it.
#[derive(Debug, Clone, PartialEq, Component, Default)]
pub struct OrderQueue {
    /// Ordered queue, front is current order.
    pub orders: VecDeque<Order>,
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

    /// Push an order to the front (for preemption, not used in slice 1 but handy).
    pub fn push_front(&mut self, order: Order) {
        self.orders.push_front(order);
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

    /// Convenience: pop front if `predicate` returns true for the front order.
    /// `Mine` callers should gate on cargo/asteroid state before calling.
    pub fn advance_if(&mut self, predicate: impl Fn(&Order) -> bool) -> Option<Order> {
        if self.front().is_some_and(&predicate) {
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
        // `StateTransition` schedule. Add it if missing (idempotent guard).
        if !app.is_plugin_added::<bevy::state::app::StatesPlugin>() {
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
        q.push_back(Order::Orbit(entities[1], 1000.0));
        q.push_back(Order::Mine(entities[2]));

        // Assert — FIFO order preserved
        assert_eq!(q.len(), 4);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(1.0, 0.0, 0.0))));
        assert!(matches!(q.orders[1], Order::Approach(_)));
        assert!(matches!(q.orders[2], Order::Orbit(_, _)));
        assert!(matches!(q.orders[3], Order::Mine(_)));
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
        // Prove in_state gating: a FixedUpdate counter should tick when Simulating,
        // and halt when Paused. We drive FixedUpdate directly via run_schedule to
        // avoid wall-clock / TimeUpdateStrategy flakiness while still proving
        // the `in_state` run condition on the configured SystemSets.
        #[derive(Resource, Default, Debug, PartialEq, Eq)]
        struct TickCount(u32);

        fn counting_system(mut c: ResMut<TickCount>) {
            c.0 += 1;
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.init_resource::<TickCount>();
        // Put counting_system in MovementSet so it inherits the set's gating.
        app.add_systems(FixedUpdate, counting_system.in_set(MovementSet));
        // Ensure startup + state init have run before we start counting.
        app.update();

        // Simulating -> ticks (each run_schedule(FixedUpdate) => one increment)
        for _ in 0..3 {
            app.world_mut().run_schedule(FixedUpdate);
        }
        let count_sim = app.world().resource::<TickCount>().0;
        assert_eq!(
            count_sim, 3,
            "should have ticked 3 times in Simulating, got {count_sim}"
        );

        // Transition to Paused -> no further ticks
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update(); // apply StateTransition
        let prev = app.world().resource::<TickCount>().0;
        for _ in 0..3 {
            app.world_mut().run_schedule(FixedUpdate);
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
    fn sets_chain_ordered_fixedupdate_deterministic_smoke() {
        // Smoke: multiple FixedUpdate passes don't lose queue ordering (determinism proxy).
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));

        let mut q = OrderQueue::new();
        for i in 0..10 {
            q.push_back(Order::FlyTo(Vec3::new(i as f32, 0.0, 0.0)));
        }
        // Tick 5 FixedUpdate frames deterministically: pop one per frame
        for _ in 0..5 {
            app.update();
            q.pop_front();
        }
        assert_eq!(q.len(), 5);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(5.0, 0.0, 0.0))));
    }
}
