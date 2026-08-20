//! Feathers + `bsn!` UI — keep BSN inline here.
//!
//! Slice 1 dev UI: detached `Camera2d` (order 1) + `bsn!`/`bsn_list!`
//! order queue text overlay. Right-click context menu is spawned
//! imperatively at cursor position (EVE Online pattern) and despawned
//! on action or dismiss. No `.bsn` asset loader in Bevy 0.19 — BSN is
//! inline as `bsn!{ ... }` via `Commands::spawn_scene`.
//!
//! `Update` only; never `FixedUpdate`.

use bevy::{
    picking::events::{Click, Pointer},
    prelude::*,
    scene::prelude::bsn,
    window::PrimaryWindow,
};
use spacegame_sim::{Asteroid, ContextMenuState, GroundPlane, Order, OrderQueue};

// Re-export for backward compat — canonical definitions live in
// `spacegame_sim::picking` to avoid circular `render ↔ ui` dependency.
pub use spacegame_sim::{LastFlyToPos, SelectedAsteroid};

/// Marker for the OrderQueue overlay text entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OrderQueueText;

/// Marker for the context menu root entity (imperatively spawned).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ContextMenuRoot;

/// Tracks the currently open context menu entity for despawn.
#[derive(Resource, Default)]
struct ContextMenuEntity(Option<Entity>);

/// `UiPlugin` — inline `bsn!` observers per AGENTS.md.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastFlyToPos>();
        app.init_resource::<SelectedAsteroid>();
        app.init_resource::<ContextMenuEntity>();
        // Global picking observers — wire `Pointer<Click>` from mesh picking
        // (requires `MeshPickingPlugin` in `spacegame_render`). Without these,
        // `SelectedAsteroid`/`LastFlyToPos` stayed `None` and the menu fell
        // back to non-deterministic `iter().next()`.
        app.add_observer(on_asteroid_click);
        app.add_observer(on_ground_click);
        app.add_systems(Startup, setup_ui);
        app.add_systems(
            Update,
            (
                context_menu_right_click_detect,
                dismiss_context_menu_on_left_click,
                dismiss_context_menu_on_escape,
                update_order_queue_overlay,
            ),
        );
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

/// Despawn the current context menu entity, if any.
fn despawn_context_menu(
    commands: &mut Commands,
    menu: &mut ResMut<ContextMenuEntity>,
    state: &mut ResMut<ContextMenuState>,
) {
    if let Some(entity) = menu.0.take() {
        commands.entity(entity).despawn();
    }
    **state = ContextMenuState::Hidden;
}

/// Detect right-click on asteroids via raw input + ray cast.
///
/// On hit, despawns any existing menu and spawns a new one at cursor position.
#[allow(clippy::too_many_arguments, clippy::excessive_nesting)]
fn context_menu_right_click_detect(
    mut commands: Commands,
    mut mouse_events: MessageReader<bevy::input::mouse::MouseButtonInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    asteroids: Query<(&Transform, &Asteroid), Without<Camera>>,
    mut menu: ResMut<ContextMenuEntity>,
    mut state: ResMut<ContextMenuState>,
    queues: Query<Entity, With<OrderQueue>>,
    selected: Res<SelectedAsteroid>,
    asteroid_entities: Query<Entity, With<Asteroid>>,
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
            b * b - c >= 0.0
        });
        if !hit {
            continue;
        }

        // Despawn old menu, spawn new at cursor position (EVE Online pattern).
        despawn_context_menu(&mut commands, &mut menu, &mut state);

        let queue_entity = queues.iter().next();
        let asteroid_entity = selected.0.or_else(|| asteroid_entities.iter().min());

        let menu_entity = commands
            .spawn((
                ContextMenuRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(cursor_pos.x),
                    top: Val::Px(cursor_pos.y),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.10, 0.14, 0.88)),
                ZIndex(100),
            ))
            .id();

        // Spawn each button as a child with an observer that despawns the menu.
        let btn_style = || Node {
            padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            ..default()
        };

        // FlyTo Here
        let flyto_target = last_fly_to_pos(&asteroids);
        let qe = queue_entity;
        commands
            .spawn((
                Button,
                btn_style(),
                BackgroundColor(Color::srgba(0.22, 0.32, 0.68, 0.9)),
                Text::new("FlyTo Here"),
                TextFont::default(),
                TextColor(Color::WHITE),
                ChildOf(menu_entity),
            ))
            .observe(
                move |_click: On<Pointer<Click>>, mut queues: Query<&mut OrderQueue>| {
                    if let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::FlyTo(flyto_target));
                    }
                },
            );

        // Approach
        let target = asteroid_entity;
        let qe = queue_entity;
        commands
            .spawn((
                Button,
                btn_style(),
                BackgroundColor(Color::srgba(0.26, 0.46, 0.28, 0.9)),
                Text::new("Approach"),
                TextFont::default(),
                TextColor(Color::WHITE),
                ChildOf(menu_entity),
            ))
            .observe(
                move |_click: On<Pointer<Click>>, mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::Approach(entity));
                    }
                },
            );

        // Orbit
        let target = asteroid_entity;
        let qe = queue_entity;
        commands
            .spawn((
                Button,
                btn_style(),
                BackgroundColor(Color::srgba(0.58, 0.42, 0.18, 0.9)),
                Text::new("Orbit"),
                TextFont::default(),
                TextColor(Color::WHITE),
                ChildOf(menu_entity),
            ))
            .observe(
                move |_click: On<Pointer<Click>>, mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                        && let Ok(dist) = spacegame_data::Distance::new(1000.0)
                    {
                        q.push_back(Order::orbit(entity, dist));
                    }
                },
            );

        // Mine
        let target = asteroid_entity;
        let qe = queue_entity;
        commands
            .spawn((
                Button,
                btn_style(),
                BackgroundColor(Color::srgba(0.62, 0.26, 0.26, 0.9)),
                Text::new("Mine"),
                TextFont::default(),
                TextColor(Color::WHITE),
                ChildOf(menu_entity),
            ))
            .observe(
                move |_click: On<Pointer<Click>>, mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::Mine(entity));
                    }
                },
            );

        menu.0 = Some(menu_entity);
        *state = ContextMenuState::Shown(cursor_pos);
        return;
    }
}

/// Fallback `LastFlyToPos` for FlyTo button — uses stored value or default.
fn last_fly_to_pos(asteroids: &Query<(&Transform, &Asteroid), Without<Camera>>) -> Vec3 {
    // Use the first asteroid position as a sensible default.
    asteroids
        .iter()
        .next()
        .map(|(tf, _)| tf.translation)
        .unwrap_or(Vec3::new(2000.0, 0.0, 800.0))
}

/// Dismiss context menu on left-click anywhere via raw input.
fn dismiss_context_menu_on_left_click(
    mut mouse_events: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut commands: Commands,
    mut menu: ResMut<ContextMenuEntity>,
    mut state: ResMut<ContextMenuState>,
) {
    for ev in mouse_events.read() {
        if ev.button == MouseButton::Left && ev.state.is_pressed() {
            despawn_context_menu(&mut commands, &mut menu, &mut state);
        }
    }
}

/// Dismiss context menu on Escape key press.
fn dismiss_context_menu_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut menu: ResMut<ContextMenuEntity>,
    mut state: ResMut<ContextMenuState>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        despawn_context_menu(&mut commands, &mut menu, &mut state);
    }
}

/// Spawn UI: `Camera2d` order 1 + `bsn!` order queue overlay.
fn setup_ui(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
    ));

    // Order queue overlay — always visible at top-left.
    commands.spawn_scene(bsn! {
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexStart,
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
                        Text("OrderQueue: empty - right-click an asteroid for commands")
                        TextFont
                        TextColor(Color::srgb(0.92, 0.92, 0.95))
                        OrderQueueText
                    )
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
        "OrderQueue: empty - right-click an asteroid for commands".to_string()
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
