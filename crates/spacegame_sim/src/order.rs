//! Orders and FIFO queue.
//!
//! `Order` is the CEO-issued strategic instruction; [`OrderQueue`] is the
//! per-ship FIFO that ticks front-only on `FixedUpdate`. The queue's inner
//! `VecDeque` is private so FIFO cannot be bypassed via `push_front`/`insert`.

use bevy::prelude::*;
use spacegame_data::Distance;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// OrbitTarget — Data Clumps fix bundle for Orbit
// ---------------------------------------------------------------------------

/// Target and range for an orbit order.
///
/// Bundles `Entity` + `Distance` that otherwise travel together as a Data
/// Clump (`Orbit(Entity, Distance)`). Newtype-style struct gives the pair
/// a name and a single construction site; future orbit steering can add
/// fields (e.g. `tangential_speed`) without changing the `Order` shape
/// via additional `Option` fields on this struct.
#[derive(Debug, Clone, PartialEq)]
pub struct OrbitTarget {
    /// Entity to orbit (e.g. asteroid or station).
    pub entity: Entity,
    /// Desired orbital radius around `entity`.
    pub distance: Distance,
}

impl OrbitTarget {
    /// Create a new orbit target.
    #[must_use]
    pub fn new(entity: Entity, distance: Distance) -> Self {
        Self { entity, distance }
    }
}

// ---------------------------------------------------------------------------
// Order
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
    Orbit(OrbitTarget),
    /// Persistent looping mine of an entity while in `mining_range`.
    Mine(Entity),
}

impl Order {
    /// Convenience constructor for `Orbit` that takes the raw pair.
    ///
    /// Exists to keep call-sites ergonomic while the stored form is the
    /// bundled [`OrbitTarget`] (Data Clumps fix).
    #[must_use]
    pub fn orbit(entity: Entity, distance: Distance) -> Self {
        Self::Orbit(OrbitTarget::new(entity, distance))
    }
}

// ---------------------------------------------------------------------------
// OrderQueue
// ---------------------------------------------------------------------------

/// FIFO queue of [`Order`]s on a ship. `Mine` persists until external
/// conditions (cargo full or asteroid despawned) pop it.
///
/// FIFO is compiler-enforced: the inner [`VecDeque`] is private; only
/// [`OrderQueue::push_back`] / [`OrderQueue::pop_front`] /
/// [`OrderQueue::advance_if`] can mutate ordering. Reads go through
/// [`OrderQueue::front`] / [`OrderQueue::get`] / [`OrderQueue::iter`].
///
/// `clear`/`get`/`iter`/`advance_if` are not speculative: `get`/`iter`
/// are the read-only API after encapsulation, `clear` is used by the
/// order-queue clear test and future AI re-plan, `advance_if` is the
/// ergonomic single-site for `Mine` persistence.
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::App;

    fn dummy_entities(n: usize, app: &mut App) -> Vec<Entity> {
        (0..n).map(|_| app.world_mut().spawn_empty().id()).collect()
    }

    #[test]
    fn queue_push_four_fifo_ordered() {
        let mut app = App::new();
        let entities = dummy_entities(3, &mut app);
        let mut q = OrderQueue::new();

        q.push_back(Order::FlyTo(Vec3::new(1.0, 0.0, 0.0)));
        q.push_back(Order::Approach(entities[0]));
        q.push_back(Order::orbit(
            entities[1],
            Distance::new(1000.0).expect("valid distance"),
        ));
        q.push_back(Order::Mine(entities[2]));

        assert_eq!(q.len(), 4);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(1.0, 0.0, 0.0))));
        assert!(matches!(q.get(1), Some(Order::Approach(_))));
        assert!(matches!(q.get(2), Some(Order::Orbit(_))));
        assert!(matches!(q.get(3), Some(Order::Mine(_))));
        assert_eq!(q.iter().count(), 4);
        assert!(!q.is_mining(), "front is FlyTo, not Mine");
    }

    #[test]
    fn queue_pop_on_completion_advances_front() {
        let mut app = App::new();
        let entities = dummy_entities(2, &mut app);
        let mut q = OrderQueue::new();
        q.push_back(Order::FlyTo(Vec3::ZERO));
        q.push_back(Order::Approach(entities[0]));
        q.push_back(Order::Mine(entities[1]));

        let popped = q.pop_front();
        assert_eq!(popped, Some(Order::FlyTo(Vec3::ZERO)));
        assert_eq!(q.front(), Some(&Order::Approach(entities[0])));
        assert_eq!(q.len(), 2);

        q.pop_front();
        assert_eq!(q.front(), Some(&Order::Mine(entities[1])));
        assert!(q.is_mining());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn queue_mine_persists_until_external_pop() {
        let mut app = App::new();
        let e = dummy_entities(1, &mut app)[0];
        let mut q = OrderQueue::with_order(Order::Mine(e));

        assert!(q.is_mining());
        let not_popped = q.advance_if(|o| matches!(o, Order::FlyTo(_)));
        assert!(not_popped.is_none());
        assert_eq!(q.len(), 1);
        assert!(q.is_mining());

        let popped = q.advance_if(|o| matches!(o, Order::Mine(_)));
        assert_eq!(popped, Some(Order::Mine(e)));
        assert!(q.is_empty());
        assert!(!q.is_mining());
    }

    #[test]
    fn queue_clear_and_is_empty() {
        let mut q = OrderQueue::new();
        q.push_back(Order::FlyTo(Vec3::ONE));
        q.push_back(Order::FlyTo(Vec3::ZERO));
        assert_eq!(q.len(), 2);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.front(), None);
    }

    #[test]
    fn orbit_target_bundles_entity_and_distance() {
        let mut app = App::new();
        let e = dummy_entities(1, &mut app)[0];
        let d = Distance::new(500.0).unwrap();
        let target = OrbitTarget::new(e, d);
        let order = Order::Orbit(target.clone());
        assert!(matches!(order, Order::Orbit(t) if t == target));
        // Convenience ctor equals manual bundle.
        assert_eq!(Order::orbit(e, d), Order::Orbit(target));
    }
}
