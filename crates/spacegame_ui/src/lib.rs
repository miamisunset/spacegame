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
///
/// # Bevy system dependencies
/// Requires `spacegame_render::RenderPlugin` to be added **before** this plugin:
/// `RenderPlugin` adds `MeshPickingPlugin` (mesh ray-cast backend). Without it,
/// `Pointer<Click>` observers (`on_asteroid_click`, `on_ground_click`) never fire
/// for 3-D meshes. `RenderPlugin` must run on `Update` while the context-menu
/// detection runs on `PreUpdate` so `ContextMenuState::Shown` is visible to
/// `camera_controller_system` (`orbit_enabled` gate) in the same frame.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastFlyToPos>();
        app.init_resource::<SelectedAsteroid>();
        app.init_resource::<ContextMenuState>();
        app.init_resource::<ContextMenuEntity>();
        // Global picking observers — wire `Pointer<Click>` from mesh picking
        // (requires `MeshPickingPlugin` in `spacegame_render`). Without these,
        // `SelectedAsteroid`/`LastFlyToPos` stayed `None` and the menu fell
        // back to non-deterministic `iter().next()`.
        app.add_observer(on_asteroid_click);
        app.add_observer(on_ground_click);
        app.add_systems(Startup, setup_ui);
        // `context_menu_right_click_detect` in `PreUpdate` so the
        // `ContextMenuState` is already `Shown` before `camera_controller_system`
        // (`Update`) checks `orbit_enabled`. Avoids one-frame orbit-before-menu race.
        app.add_systems(PreUpdate, context_menu_right_click_detect);
        app.add_systems(
            Update,
            (
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
        commands
            .entity(entity)
            .despawn_related::<Children>()
            .despawn();
    }
    **state = ContextMenuState::Hidden;
}

/// Detect right-click on asteroids via raw input + ray cast.
///
/// On hit, despawns any existing menu and spawns a new one at cursor position.
/// On miss, dismisses any existing menu so right-drag can orbit per spec.
///
/// # MessageReader draining
/// `MessageReader<MouseButtonInput>` must be drained every frame even when
/// guards (`windows.single()`, `camera_q.find()`) fail, otherwise the unread
/// buffer is re-read next tick with a stale `cursor_position()`. This function
/// calls `mouse_events.clear()` on early exit and drains all events in the
/// `for ev in mouse_events.read()` loop without `return` (uses `handled` flag).
#[allow(clippy::too_many_arguments, clippy::excessive_nesting)]
fn context_menu_right_click_detect(
    mut commands: Commands,
    mut mouse_events: MessageReader<bevy::input::mouse::MouseButtonInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    asteroids: Query<(Entity, &Transform, &Asteroid), Without<Camera>>,
    mut menu: ResMut<ContextMenuEntity>,
    mut state: ResMut<ContextMenuState>,
    queues: Query<Entity, With<OrderQueue>>,
    selected: Res<SelectedAsteroid>,
    asteroid_entities: Query<Entity, With<Asteroid>>,
) {
    let Ok(window) = windows.single() else {
        mouse_events.clear();
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        mouse_events.clear();
        return;
    };
    let Some((camera, camera_transform)) =
        camera_q.iter().find(|(c, _)| c.order == 0 && c.is_active)
    else {
        mouse_events.clear();
        return;
    };

    // Clamp menu to stay on-screen in logical pixels (cursor_position is logical).
    // `window.physical_size / scale_factor` is the logical viewport. Menu is
    // ~148px wide (140 + padding) and ~160px tall (4× buttons); clamp so it
    // never overflows off the right/bottom edge (HiDPI scale_factor >1 handled).
    let scale = window.scale_factor();
    let logical_size = window.physical_size().as_vec2() / scale;
    let menu_size = Vec2::new(148.0, 160.0);
    let clamped_pos = Vec2::new(
        cursor_pos
            .x
            .min((logical_size.x - menu_size.x).max(0.0))
            .max(0.0),
        cursor_pos
            .y
            .min((logical_size.y - menu_size.y).max(0.0))
            .max(0.0),
    );

    // Deferred despawn note: `despawn_context_menu` queues `Commands` despawn
    // which flushes at stage end. Old menu + new menu co-exist for one frame
    // if right-clicking rapidly — acceptable (single frame, same ZIndex) and
    // avoids exclusive `&mut World` access.
    let mut handled = false;
    for ev in mouse_events.read() {
        if ev.button != MouseButton::Right || !ev.state.is_pressed() {
            continue;
        }
        if handled {
            continue; // already spawned one menu this frame; drain remaining
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
            continue;
        };
        // Find closest hit asteroid — uses shared `Asteroid::radius()` so pick
        // radius == render radius (fixes 80.0 vs 86.7 mismatch). `t >= 0`
        // front-face check mirrors `ensure_asteroid_mesh_system`.
        let mut hit_entity: Option<Entity> = None;
        let mut closest_t = f32::MAX;
        for (entity, tf, asteroid) in &asteroids {
            let oc = ray.origin - tf.translation;
            let dir = *ray.direction;
            let r = asteroid.radius();
            let b = oc.dot(dir);
            let c = oc.dot(oc) - r * r;
            let disc = b * b - c;
            if disc < 0.0 {
                continue;
            }
            let sqrt_d = disc.sqrt();
            let t0 = -b - sqrt_d;
            let t1 = -b + sqrt_d;
            let t = if t0 >= 0.0 {
                t0
            } else if t1 >= 0.0 {
                t1
            } else {
                continue;
            };
            if t < closest_t {
                closest_t = t;
                hit_entity = Some(entity);
            }
        }
        if hit_entity.is_none() {
            // Right-click on empty space / ground: close menu so orbit is re-enabled.
            if *state != ContextMenuState::Hidden {
                despawn_context_menu(&mut commands, &mut menu, &mut state);
            }
            handled = true;
            continue;
        }

        // Despawn old menu, spawn new at clamped cursor position (EVE Online pattern).
        despawn_context_menu(&mut commands, &mut menu, &mut state);

        let queue_entity = queues.iter().next();
        // Contextual target is the hit asteroid; fallback to selected/min only if
        // hit_entity somehow missing (deterministic `min()` not `next()`).
        let hit = hit_entity;
        let fallback_entity = selected.0.or_else(|| asteroid_entities.iter().min());
        let asteroid_entity = hit.or(fallback_entity);
        // For FlyTo fallback when no LastFlyToPos, use hit asteroid position
        // deterministically (not `iter().next()`).
        let hit_pos = hit_entity.and_then(|e| {
            asteroids
                .iter()
                .find(|(entity, _, _)| *entity == e)
                .map(|(_, tf, _)| tf.translation)
        });

        let menu_entity = commands
            .spawn((
                ContextMenuRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(clamped_pos.x),
                    top: Val::Px(clamped_pos.y),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    width: Val::Auto,
                    height: Val::Auto,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.10, 0.14, 0.88)),
                ZIndex(100),
            ))
            .id();

        // Spawn each button as a child with an observer that despawns the menu.
        let btn_style = || Node {
            display: Display::Flex,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            width: Val::Px(140.0),
            ..default()
        };

        // FlyTo Here — resolves `LastFlyToPos` inside the observer so a ground
        // click between menu open and button pick is not stale (captured `Vec3`
        // would be). Falls back to hit asteroid position deterministically.
        let qe = queue_entity;
        let hit_fallback = hit_pos;
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
                move |_click: On<Pointer<Click>>,
                      mut commands: Commands,
                      mut menu: ResMut<ContextMenuEntity>,
                      mut state: ResMut<ContextMenuState>,
                      mut queues: Query<&mut OrderQueue>,
                      last_pos: Res<LastFlyToPos>,
                      asteroids: Query<(Entity, &Transform, &Asteroid), Without<Camera>>| {
                    let target = resolve_flyto_target(&last_pos, &asteroids, hit_fallback);
                    if let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::FlyTo(target));
                    }
                    if let Some(e) = menu.0.take() {
                        commands.entity(e).despawn_related::<Children>().despawn();
                    }
                    *state = ContextMenuState::Hidden;
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
                move |_click: On<Pointer<Click>>,
                      mut commands: Commands,
                      mut menu: ResMut<ContextMenuEntity>,
                      mut state: ResMut<ContextMenuState>,
                      mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::Approach(entity));
                    }
                    if let Some(e) = menu.0.take() {
                        commands.entity(e).despawn_related::<Children>().despawn();
                    }
                    *state = ContextMenuState::Hidden;
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
                move |_click: On<Pointer<Click>>,
                      mut commands: Commands,
                      mut menu: ResMut<ContextMenuEntity>,
                      mut state: ResMut<ContextMenuState>,
                      mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                        && let Ok(dist) = spacegame_data::Distance::new(1000.0)
                    {
                        q.push_back(Order::orbit(entity, dist));
                    }
                    if let Some(e) = menu.0.take() {
                        commands.entity(e).despawn_related::<Children>().despawn();
                    }
                    *state = ContextMenuState::Hidden;
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
                move |_click: On<Pointer<Click>>,
                      mut commands: Commands,
                      mut menu: ResMut<ContextMenuEntity>,
                      mut state: ResMut<ContextMenuState>,
                      mut queues: Query<&mut OrderQueue>| {
                    if let Some(entity) = target
                        && let Some(qe) = qe
                        && let Ok(mut q) = queues.get_mut(qe)
                    {
                        q.push_back(Order::Mine(entity));
                    }
                    if let Some(e) = menu.0.take() {
                        commands.entity(e).despawn_related::<Children>().despawn();
                    }
                    *state = ContextMenuState::Hidden;
                },
            );

        menu.0 = Some(menu_entity);
        *state = ContextMenuState::Shown(clamped_pos);
        handled = true;
    }
}

/// Resolve `FlyTo` target inside the click observer — prefers `LastFlyToPos`
/// (ground pick), then hit asteroid position, then deterministic `min()` asteroid,
/// then hardcoded fallback. Called at click time so stale `Vec3` capture is avoided.
fn resolve_flyto_target(
    last_pos: &LastFlyToPos,
    asteroids: &Query<(Entity, &Transform, &Asteroid), Without<Camera>>,
    hit_pos: Option<Vec3>,
) -> Vec3 {
    if let Some(pos) = last_pos.0 {
        return pos;
    }
    if let Some(pos) = hit_pos {
        return pos;
    }
    // Deterministic fallback: smallest Entity id, not `iter().next()` (query order nondet).
    if let Some((_, tf, _)) = asteroids.iter().min_by_key(|(e, _, _)| *e) {
        return tf.translation;
    }
    // No asteroids at all
    Vec3::new(2000.0, 0.0, 800.0)
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
        IsDefaultUiCamera,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
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
