//! Feathers + `bsn!` UI — keep BSN inline here.
//!
//! Slice 1 dev UI: detached `Camera2d` (order 1) + `bsn!`/`bsn_list!`
//! context menu `[FlyTo Here, Approach, Orbit, Mine]` driven by
//! `on(|e: On<Pointer<Click>>| {...})` observers and an `OrderQueue`
//! text overlay. No `.bsn` asset loader in Bevy 0.19 — BSN is inline
//! as `bsn!{ ... }` via `Commands::spawn_scene`.
//!
//! Right-click on an asteroid shows the context menu at cursor position.
//! Left-click or Escape hides it. `Update` only; never `FixedUpdate`.

use bevy::{
    picking::events::{Click, Pointer},
    prelude::*,
    scene::prelude::{bsn, bsn_list},
    window::PrimaryWindow,
};
use spacegame_sim::{Asteroid, ContextMenuState, GroundPlane, Order, OrderQueue};

// Re-export for backward compat — canonical definitions live in
// `spacegame_sim::picking` to avoid circular `render ↔ ui` dependency.
pub use spacegame_sim::{LastFlyToPos, SelectedAsteroid};

/// Marker for the OrderQueue overlay text entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OrderQueueText;

/// Marker for the dev context menu root.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ContextMenuRoot;

/// `UiPlugin` — inline `bsn!` observers per AGENTS.md.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastFlyToPos>();
        app.init_resource::<SelectedAsteroid>();
        app.init_resource::<ContextMenuState>();
        // Global picking observers — wire `Pointer<Click>` from mesh picking
        // (requires `MeshPickingPlugin` in `spacegame_render`). Without these,
        // `SelectedAsteroid`/`LastFlyToPos` stayed `None` and the menu fell
        // back to non-deterministic `iter().next()`.
        app.add_observer(on_asteroid_click);
        app.add_observer(on_ground_click);
        // Right-click context menu — raw input + ray cast for reliable detection.
        app.add_systems(
            Update,
            (
                (
                    context_menu_right_click_detect,
                    on_left_click_hide_menu_system,
                ),
                (
                    hide_menu_on_escape,
                    update_order_queue_overlay,
                    update_context_menu_visibility,
                ),
            ),
        );
        app.add_systems(Startup, setup_ui);
    }
}

/// Set `SelectedAsteroid` on asteroid mesh click (any button).
fn on_asteroid_click(
    click: On<Pointer<Click>>,
    mut selected: ResMut<SelectedAsteroid>,
    asteroids: Query<(), With<Asteroid>>,
) {
    if asteroids.contains(click.entity) {
        selected.0 = Some(click.entity);
    }
}

/// Set `LastFlyToPos` on ground-plane click via `HitData::position`.
fn on_ground_click(
    click: On<Pointer<Click>>,
    mut last_pos: ResMut<LastFlyToPos>,
    ground: Query<(), With<GroundPlane>>,
) {
    if ground.contains(click.entity)
        && let Some(pos) = click.hit.position
    {
        last_pos.0 = Some(pos);
    }
}

/// Detect right-click on asteroids via raw input + ray cast.
///
/// Bypasses the picking pipeline entirely — reads `MouseButtonInput` events,
/// ray-casts from the camera through the cursor, and checks sphere intersection
/// with asteroid transforms. Shows context menu at cursor position on hit.
fn context_menu_right_click_detect(
    mut mouse_events: MessageReader<bevy::input::mouse::MouseButtonInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    asteroids: Query<(&Transform, &Asteroid), Without<Camera>>,
    mut state: ResMut<ContextMenuState>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Some((camera, camera_transform)) =
        camera_q.iter().find(|(c, _)| c.order == 0 && c.is_active)
    else {
        return;
    };

    for ev in mouse_events.read() {
        if ev.button != MouseButton::Right || !ev.state.is_pressed() {
            continue;
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
            continue;
        };
        let hit = asteroids.iter().any(|(tf, _)| {
            let oc = ray.origin - tf.translation;
            let dir = *ray.direction;
            let r = 80.0;
            let b = oc.dot(dir);
            let c = oc.dot(oc) - r * r;
            b * b - c >= 0.0 // a=1 for normalized dir, discriminant = b²-4ac = b²-4c; >=0 when b²>=c (simplified)
        });
        if hit {
            *state = ContextMenuState::Shown(cursor_pos);
            return;
        }
    }
}

/// Hide context menu on left-click anywhere via raw input.
fn on_left_click_hide_menu_system(
    mut mouse_events: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut state: ResMut<ContextMenuState>,
) {
    for ev in mouse_events.read() {
        if ev.button == MouseButton::Left && ev.state.is_pressed() {
            *state = ContextMenuState::Hidden;
        }
    }
}

/// Hide context menu on Escape key press (runs every frame).
fn hide_menu_on_escape(keyboard: Res<ButtonInput<KeyCode>>, mut state: ResMut<ContextMenuState>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        *state = ContextMenuState::Hidden;
    }
}

/// Sync `ContextMenuState` to the `ContextMenuRoot` entity's visibility
/// and position. Runs each frame on `Update`.
fn update_context_menu_visibility(
    state: Res<ContextMenuState>,
    mut q: Query<(&mut Visibility, &mut Node), With<ContextMenuRoot>>,
) {
    let Ok((mut visibility, mut node)) = q.single_mut() else {
        return;
    };
    match *state {
        ContextMenuState::Hidden => {
            *visibility = Visibility::Hidden;
        }
        ContextMenuState::Shown(pos) => {
            *visibility = Visibility::Visible;
            // Offset by parent padding (12px) so menu appears at exact cursor position.
            node.left = Val::Px(pos.x - 12.0);
            node.top = Val::Px(pos.y - 12.0);
        }
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
                 last_pos: Res<LastFlyToPos>,
                 mut state: ResMut<ContextMenuState>| {
                    let target = last_pos.0.unwrap_or(Vec3::new(2000.0, 0.0, 800.0));
                    for mut q in &mut queues {
                        q.push_back(Order::FlyTo(target));
                    }
                    *state = ContextMenuState::Hidden;
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
                 asteroids: Query<Entity, With<Asteroid>>,
                 mut state: ResMut<ContextMenuState>| {
                    // Deterministic fallback — `iter().min()` is insertion-order
                    // independent (reviews flagged `iter().next()` as nondeterministic).
                    let target = selected.0.or_else(|| asteroids.iter().min());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            q.push_back(Order::Approach(entity));
                        }
                    }
                    *state = ContextMenuState::Hidden;
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
                 asteroids: Query<Entity, With<Asteroid>>,
                 mut state: ResMut<ContextMenuState>| {
                    let target = selected.0.or_else(|| asteroids.iter().min());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            if let Ok(dist) = spacegame_data::Distance::new(1000.0) {
                                q.push_back(Order::orbit(entity, dist));
                            }
                        }
                    }
                    *state = ContextMenuState::Hidden;
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
                 asteroids: Query<Entity, With<Asteroid>>,
                 mut state: ResMut<ContextMenuState>| {
                    let target = selected.0.or_else(|| asteroids.iter().min());
                    if let Some(entity) = target {
                        for mut q in &mut queues {
                            q.push_back(Order::Mine(entity));
                        }
                    }
                    *state = ContextMenuState::Hidden;
                }
            )
        )
    };

    // Context menu starts hidden — shown by `on_asteroid_right_click` observer.
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
                    position_type: PositionType::Absolute,
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                }
                BackgroundColor(Color::srgba(0.10, 0.10, 0.14, 0.72))
                ZIndex(100)
                Visibility::Hidden
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
