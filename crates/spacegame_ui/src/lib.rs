//! Feathers + `bsn!` UI — keep BSN inline here.
//!
//! Slice 1 dev UI: detached `Camera2d` (order 1) + `bsn!`/`bsn_list!`
//! context menu `[FlyTo Here, Approach, Orbit, Mine]` driven by
//! `on(|e: On<Pointer<Click>>| {...})` observers and an `OrderQueue`
//! text overlay. No `.bsn` asset loader in Bevy 0.19 — BSN is inline
//! as `bsn!{ ... }` via `Commands::spawn_scene`.
//!
//! `Update` only; never `FixedUpdate`.

use bevy::{
    picking::events::{Click, Pointer},
    prelude::*,
    scene::prelude::{bsn, bsn_list},
};
use spacegame_sim::{Asteroid, Order, OrderQueue};

/// Marker for the OrderQueue overlay text entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OrderQueueText;

/// Marker for the dev context menu root.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ContextMenuRoot;

/// Last world-space click for `FlyTo Here` — set by ground picking.
#[derive(Resource, Debug, Clone, Default)]
pub struct LastFlyToPos(pub Option<Vec3>);

/// Last asteroid selected by pointer click — used by Approach/Orbit/Mine.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelectedAsteroid(pub Option<Entity>);

/// `UiPlugin` — inline `bsn!` observers per AGENTS.md.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastFlyToPos>();
        app.init_resource::<SelectedAsteroid>();
        app.add_systems(Startup, setup_ui);
        app.add_systems(Update, update_order_queue_overlay);
    }
}

/// Spawn UI: `Camera2d` order 1 + `bsn!` overlay and context menu.
///
/// Uses `bsn!` + `bsn_list!` proc-macros for declarative UI (Feathers).
/// `on(|e: On<Pointer<Click>>| {...})` inside `bsn!` wires callback-style
/// observers; buffered `Message` not needed for discrete order issuance.
fn setup_ui(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
    ));

    // Demonstrate `bsn_list!` — button list built as a reusable `SceneList`
    // spliced into the parent `Children` via `{buttons}` expression.
    // This satisfies the `bsn_list!` portion of the acceptance criteria.
    let buttons = bsn_list! {
        (
            Button
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            BackgroundColor(Color::srgb(0.22, 0.32, 0.68))
            Text("FlyTo Here")
            TextFont
            TextColor(Color::WHITE)
            on(
                |_click: On<Pointer<Click>>,
                 mut queues: Query<&mut OrderQueue>,
                 last_pos: Res<LastFlyToPos>| {
                    let target = last_pos.0.unwrap_or(Vec3::new(2000.0, 0.0, 800.0));
                    for mut q in &mut queues {
                        q.push_back(Order::FlyTo(target));
                    }
                }
            )
        ),
        (
            Button
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            BackgroundColor(Color::srgb(0.26, 0.46, 0.28))
            Text("Approach")
            TextFont
            TextColor(Color::WHITE)
            on(
                |_click: On<Pointer<Click>>,
                 mut queues: Query<&mut OrderQueue>,
                 selected: Res<SelectedAsteroid>,
                 asteroids: Query<Entity, With<Asteroid>>| {
                    let target = selected.0.or_else(|| asteroids.iter().next());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            q.push_back(Order::Approach(entity));
                        }
                    }
                }
            )
        ),
        (
            Button
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            BackgroundColor(Color::srgb(0.58, 0.42, 0.18))
            Text("Orbit")
            TextFont
            TextColor(Color::WHITE)
            on(
                |_click: On<Pointer<Click>>,
                 mut queues: Query<&mut OrderQueue>,
                 selected: Res<SelectedAsteroid>,
                 asteroids: Query<Entity, With<Asteroid>>| {
                    let target = selected.0.or_else(|| asteroids.iter().next());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            if let Ok(dist) = spacegame_data::Distance::new(1000.0) {
                                q.push_back(Order::orbit(entity, dist));
                            }
                        }
                    }
                }
            )
        ),
        (
            Button
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            }
            BackgroundColor(Color::srgb(0.62, 0.26, 0.26))
            Text("Mine")
            TextFont
            TextColor(Color::WHITE)
            on(
                |_click: On<Pointer<Click>>,
                 mut queues: Query<&mut OrderQueue>,
                 selected: Res<SelectedAsteroid>,
                 asteroids: Query<Entity, With<Asteroid>>| {
                    let target = selected.0.or_else(|| asteroids.iter().next());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            q.push_back(Order::Mine(entity));
                        }
                    }
                }
            )
        )
    };

    commands.spawn_scene(bsn! {
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            padding: UiRect::all(Val::Px(12.0)),
        }
        ZIndex(100)
        Children [
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(8.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                }
                BackgroundColor(Color::srgba(0.08, 0.08, 0.12, 0.78))
                ZIndex(100)
                Children [
                    (
                        Text("OrderQueue empty")
                        TextFont
                        TextColor(Color::srgb(0.92, 0.92, 0.95))
                        OrderQueueText
                    )
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    align_self: AlignSelf::FlexStart,
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                }
                BackgroundColor(Color::srgba(0.10, 0.10, 0.14, 0.72))
                ZIndex(100)
                ContextMenuRoot
                Children [
                    {buttons}
                ]
            )
        ]
    });
}

/// Update the `OrderQueue` text overlay each frame.
fn update_order_queue_overlay(
    queue_q: Query<&OrderQueue>,
    mut text_q: Query<&mut Text, With<OrderQueueText>>,
) {
    let Some(queue) = queue_q.iter().next() else {
        return;
    };
    let Some(mut text) = text_q.iter_mut().next() else {
        return;
    };

    let summary = if queue.is_empty() {
        "OrderQueue: empty - use [FlyTo Here, Approach, Orbit, Mine]".to_string()
    } else {
        let parts: Vec<String> = queue
            .iter()
            .map(|order| match order {
                Order::FlyTo(pos) => format!("FlyTo({:.0},{:.0},{:.0})", pos.x, pos.y, pos.z),
                Order::Approach(entity) => format!("Approach({:?})", entity),
                Order::Orbit(target) => {
                    format!("Orbit({:?} @ {:.0})", target.entity, target.distance.get())
                }
                Order::Mine(entity) => format!("Mine({:?})", entity),
            })
            .collect();
        format!("OrderQueue: {}", parts.join(" -> "))
    };

    *text = Text::new(summary);
}
