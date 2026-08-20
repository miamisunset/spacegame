//! Deterministic sim plugin and schedule wiring.

use bevy::prelude::*;

use crate::movement::movement_system;
use crate::sets::{AiSet, CombatSet, EconomySet, MiningSet, MovementSet};
use crate::state::GameState;

/// Deterministic sim plugin. Headless-friendly — no `bevy_render` dependency.
///
/// Registers [`GameState`] and configures ordered [`crate::sets`] on
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
        app.add_systems(FixedUpdate, movement_system.in_set(MovementSet));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{Order, OrderQueue};
    use crate::sets::{AiSet, CombatSet, EconomySet, MiningSet, MovementSet};
    use crate::state::GameState;

    #[test]
    fn gamestate_default_is_simulating_headless() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));

        app.update();
        let state = app.world().resource::<State<GameState>>();
        assert_eq!(**state, GameState::Simulating);

        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update();
        let state = app.world().resource::<State<GameState>>();
        assert_eq!(**state, GameState::Paused);
    }

    #[test]
    fn fixed_update_sets_only_tick_in_simulating() {
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
        app.add_systems(FixedUpdate, counting_system.in_set(MovementSet));
        app.update();
        let base = app.world().resource::<TickCount>().0;

        for _ in 0..3 {
            app.update();
        }
        let count_sim = app.world().resource::<TickCount>().0 - base;
        assert_eq!(
            count_sim, 3,
            "should have ticked 3 times in Simulating, got {count_sim}"
        );

        // This `app.update()` applies `StateTransition` and may still tick once
        // in `Simulating` before the flip; `prev` is after that tick.
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update();
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
    fn order_queue_fifo_survives_fixed_update_ticks() {
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

        app.update();
        for _ in 0..5 {
            app.update();
        }

        let q = app.world().get::<OrderQueue>(entity).unwrap();
        assert_eq!(q.len(), 5);
        assert_eq!(q.front(), Some(&Order::FlyTo(Vec3::new(5.0, 0.0, 0.0))));
        for (idx, order) in q.iter().enumerate() {
            assert_eq!(
                *order,
                Order::FlyTo(Vec3::new((idx as f32) + 5.0, 0.0, 0.0))
            );
        }
    }

    #[test]
    fn system_sets_chain_in_fixed_update_order() {
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

        app.update();
        app.world_mut().resource_mut::<OrderLog>().0.clear();
        app.update();

        let log = app.world().resource::<OrderLog>().0.clone();
        assert_eq!(log, vec!["economy", "ai", "movement", "mining", "combat"]);
    }

    #[test]
    fn order_queue_component_spawn_and_query_headless() {
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
}
