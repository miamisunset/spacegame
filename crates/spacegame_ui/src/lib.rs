//! Feathers + bsn! UI — keep BSN inline here.
//! Use `bsn!{ Entity { Children [ ... ] } }` with `on(|e: On<Pointer<Click>>| {...})`
//! No .bsn asset file loader in Bevy 0.19.
#![allow(clippy::excessive_nesting)]
use bevy::picking::Pickable;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::scene::prelude::{CommandsSceneExt, bsn, bsn_list};

use spacegame_data::Distance;
use spacegame_sim::{Asteroid, Inventory, MiningLaser, Order, OrderQueue, ShipStats};

/// Marker for the order-queue overlay text.
#[derive(Component, Clone, Default)]
struct OrderQueueOverlay;

/// Marker for context menu root.
#[derive(Component, Clone, Default)]
struct ContextMenu;

/// Resource holding the current context target and screen position.
///
/// `world_pos` is the `y=0` plane intersect (or picking `HitData::position`)
/// used for `FlyTo Here` — avoids the old hardcoded pixel offset.
#[derive(Resource, Default)]
pub struct ContextState {
    /// Entity under cursor at right-click (asteroid/ship) — `None` for empty space.
    pub target: Option<Entity>,
    /// Cursor position in window coords at right-click.
    pub screen_pos: Vec2,
    /// World-space `y=0` intersect at right-click, used for `FlyTo`.
    pub world_pos: Vec3,
    /// Whether the context menu is currently visible.
    pub visible: bool,
}

/// SystemSet for UI — runs before [`spacegame_render::CameraSet`] so
/// `ContextState::visible` is settled before camera orbit gating.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct UiSet;

/// Feathers + `bsn!` UI plugin.
///
/// - Spawns a detached right-click context menu `[FlyTo Here, Approach, Orbit, Mine]`
///   using inline `bsn!` with `on(|e: On<Pointer<Click>>| ...)` observers per AGENTS.md.
/// - Spawns an `OrderQueue` overlay that lists queued orders for the player ship.
/// - No `.bsn` asset loader is used.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContextState>();
        app.add_systems(Startup, setup_ui);
        app.add_systems(
            Update,
            (update_order_queue_overlay, handle_right_click_spawn_menu).in_set(UiSet),
        );
    }
}

fn overlay_scene() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::FlexEnd,
            padding: UiRect::all(px(12)),
        }
        Pickable::IGNORE
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(8)),
                    width: px(360),
                }
                BackgroundColor(Color::srgba(0.08, 0.08, 0.12, 0.85))
                Children [
                    (
                        Text("OrderQueue")
                        TextFont { font_size: px(16.0) }
                        TextColor(Color::WHITE)
                    ),
                    (
                        Text("No orders - right-click to issue")
                        TextFont { font_size: px(12.0) }
                        TextColor(Color::srgb(0.8, 0.8, 0.8))
                        OrderQueueOverlay
                    )
                ]
            ),
            (
                Node {
                    margin: UiRect::top(px(8)),
                    padding: UiRect::all(px(8)),
                    width: px(360),
                }
                BackgroundColor(Color::srgba(0.08, 0.08, 0.12, 0.65))
                Children [
                    (
                        Text("WASD: pan  Q/E: yaw  R/F: pitch  Wheel: zoom  RMB drag: orbit  RMB on asteroid: menu  1-4/Space: orders")
                        TextFont { font_size: px(10.0) }
                        TextColor(Color::srgba(0.9, 0.9, 0.9, 0.7))
                    )
                ]
            )
        ]
    }
}

#[derive(Clone, Copy)]
enum OrderKind {
    FlyTo,
    Approach,
    Orbit,
    Mine,
}

fn context_menu_scene(screen_pos: Vec2) -> impl Scene {
    let pos = screen_pos;
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(pos.x),
            top: px(pos.y),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            row_gap: px(4),
            width: px(180),
        }
        BackgroundColor(Color::srgba(0.12, 0.12, 0.16, 0.95))
        Visibility::Hidden
        ContextMenu
        Children [
            (context_button("FlyTo Here", OrderKind::FlyTo)),
            (context_button("Approach", OrderKind::Approach)),
            (context_button("Orbit (1000)", OrderKind::Orbit)),
            (context_button("Mine", OrderKind::Mine))
        ]
    }
}

#[allow(clippy::excessive_nesting)]
fn context_button(label: &'static str, kind: OrderKind) -> impl Scene {
    let label_owned = label;
    bsn! {
        Button
        Node {
            width: percent(100),
            height: px(28),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
        }
        BackgroundColor(Color::srgb(0.22, 0.22, 0.28))
        on(move |ev: On<Pointer<Click>>, mut ctx: ResMut<ContextState>, mut ships: Query<&mut OrderQueue, With<ShipStats>>| {
            if ev.button != PointerButton::Primary {
                return;
            }
            let Some(mut queue) = ships.iter_mut().next() else { return; };
            // Use stored world_pos for FlyTo (ray-plane intersect), and stored target for others.
            // Falls back to first asteroid only if ctx.target is None and kind requires a target —
            // avoids iter().next() as primary selection.
            match kind {
                OrderKind::FlyTo => {
                    queue.push_back(Order::FlyTo(ctx.world_pos));
                }
                OrderKind::Approach => {
                    if let Some(e) = ctx.target {
                        queue.push_back(Order::Approach(e));
                    }
                }
                OrderKind::Orbit => {
                    if let Some(e) = ctx.target
                        && let Ok(d) = Distance::new(1000.0) {
                            queue.push_back(Order::orbit(e, d));
                        }
                }
                OrderKind::Mine => {
                    if let Some(e) = ctx.target {
                        queue.push_back(Order::Mine(e));
                    }
                }
            }
            ctx.visible = false;
            let _ = label_owned;
        })
        Children [
            (
                Text(label_owned)
                TextFont { font_size: px(12.0) }
                TextColor(Color::WHITE)
            )
        ]
    }
}

fn setup_ui(mut commands: Commands) {
    commands.spawn(Camera2d);
    // Use bsn_list! functionally per AGENTS.md — spawn both scenes as a list.
    commands.spawn_scene_list(bsn_list![overlay_scene(), context_menu_scene(Vec2::ZERO)]);
}

/// Convert screen position to world `y=0` plane via camera ray.
///
/// Returns `None` if the ray is parallel to the plane or points away.
fn screen_to_world_y0(
    screen_pos: Vec2,
    camera_q: &Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) -> Option<Vec3> {
    let (camera, cam_transform) = camera_q.single().ok()?;
    let ray = camera.viewport_to_world(cam_transform, screen_pos).ok()?;
    // Plane y=0, normal (0,1,0): t = -origin.y / dir.y
    let dir = ray.direction.as_vec3();
    let origin = ray.origin;
    if dir.y.abs() < 1e-6 {
        return None;
    }
    let t = -origin.y / dir.y;
    if t < 0.0 {
        return None;
    }
    Some(origin + dir * t)
}

#[allow(clippy::too_many_arguments)]
fn handle_right_click_spawn_menu(
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut ctx: ResMut<ContextState>,
    mut menu_query: Query<&mut Visibility, With<ContextMenu>>,
    mut menu_node: Query<&mut Node, With<ContextMenu>>,
    asteroids: Query<Entity, With<Asteroid>>,
    ships: Query<Entity, With<ShipStats>>,
    hover: Res<HoverMap>,
    window_entity_q: Query<Entity, With<Window>>,
    mut order_queues: Query<&mut OrderQueue, With<ShipStats>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window.cursor_position();

    // Keyboard shortcuts — keep for dev, still use first asteroid as fallback.
    if keyboard.just_pressed(KeyCode::Digit1)
        && let Some(mut q) = order_queues.iter_mut().next()
    {
        // Use current world_pos if available, else fallback to fixed dest for dev shortcut.
        let dest = if ctx.world_pos != Vec3::ZERO {
            ctx.world_pos
        } else {
            Vec3::new(2000.0, 0.0, 0.0)
        };
        q.push_back(Order::FlyTo(dest));
    }
    if keyboard.just_pressed(KeyCode::Digit2)
        && let (Some(mut q), Some(target)) =
            (order_queues.iter_mut().next(), asteroids.iter().next())
    {
        q.push_back(Order::Approach(target));
    }
    if keyboard.just_pressed(KeyCode::Digit3)
        && let (Some(mut q), Some(target)) =
            (order_queues.iter_mut().next(), asteroids.iter().next())
        && let Ok(d) = Distance::new(1000.0)
    {
        q.push_back(Order::orbit(target, d));
    }
    if keyboard.just_pressed(KeyCode::Digit4)
        && let (Some(mut q), Some(target)) =
            (order_queues.iter_mut().next(), asteroids.iter().next())
    {
        q.push_back(Order::Mine(target));
    }
    if keyboard.just_pressed(KeyCode::Space)
        && let Some(mut q) = order_queues.iter_mut().next()
    {
        q.clear();
    }

    // EVE-style right-click: on pickable object -> menu, on empty -> orbit (no menu).
    if mouse_button.just_pressed(MouseButton::Right) {
        let Some(pos) = cursor else {
            return;
        };
        // Find hovered entity — first non-Window hit (UI or world). Root overlay
        // is Pickable::IGNORE so world picks shine through except where UI
        // panels (360px top-right) block. This gives EVE-style: UI blocks world.
        let hovered_first = hover.get(&PointerId::Mouse).and_then(|map| {
            let window_ent = window_entity_q.single().ok();
            map.iter()
                .find(|(e, _)| Some(**e) != window_ent)
                .map(|(e, hit)| (*e, hit.clone()))
        });

        let is_world_target = hovered_first
            .as_ref()
            .is_some_and(|(e, _)| asteroids.contains(*e) || ships.contains(*e));

        if is_world_target {
            let (entity, hit) = hovered_first.unwrap();
            ctx.target = Some(entity);
            ctx.screen_pos = pos;
            // Prefer picking hit position; fallback to ray-plane y=0.
            ctx.world_pos = hit
                .position
                .unwrap_or_else(|| screen_to_world_y0(pos, &camera_q).unwrap_or(Vec3::ZERO));
            ctx.visible = true;
            for mut node in &mut menu_node {
                node.left = Val::Px(pos.x);
                node.top = Val::Px(pos.y);
            }
        } else {
            // Empty space — close menu if open (let camera orbit handle the drag).
            if ctx.visible {
                ctx.visible = false;
            }
            // Still compute world_pos for potential FlyTo if menu were open via keyboard,
            // but don't set target.
            ctx.target = None;
            ctx.screen_pos = pos;
            ctx.world_pos = screen_to_world_y0(pos, &camera_q).unwrap_or(Vec3::ZERO);
            // If we were showing menu, hide will be synced below via is_changed.
            // If menu not visible, keep it hidden — no new menu on empty.
        }
    } else if mouse_button.just_pressed(MouseButton::Left) && ctx.visible {
        ctx.visible = false;
    }

    // Sync visibility only when ContextState changed — fixes per-frame churn.
    if ctx.is_changed() {
        let vis = if ctx.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        for mut v in &mut menu_query {
            if *v != vis {
                *v = vis;
            }
        }
    }
}

fn update_order_queue_overlay(
    ship_query: Query<(&OrderQueue, &Inventory, &ShipStats, Option<&MiningLaser>), With<ShipStats>>,
    mut text_query: Query<&mut Text, With<OrderQueueOverlay>>,
) {
    let Ok(mut text) = text_query.single_mut() else {
        return;
    };
    let Some((queue, inv, stats, laser_opt)) = ship_query.iter().next() else {
        text.0 = "No ship".to_string();
        return;
    };
    let cargo_used = inv.cargo_used(1.0);
    let cargo_cap = stats.cargo_capacity.get();
    let ore = inv.get("ore");
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "Cargo: {ore} ore  {cargo_used:.0}/{cargo_cap:.0} vol"
    ));
    if let Some(laser) = laser_opt {
        lines.push(format!("Laser: {:.0}% cycle", laser.progress * 100.0));
    }
    if queue.is_empty() {
        lines.push(
            "Queue: [empty] - Right-click asteroid for menu, empty drag to orbit, 1-4/Space"
                .to_string(),
        );
    } else {
        let orders: Vec<String> = queue
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let marker = if i == 0 { ">" } else { " " };
                match o {
                    Order::FlyTo(v) => format!("{marker} FlyTo({:.0},{:.0},{:.0})", v.x, v.y, v.z),
                    Order::Approach(e) => format!("{marker} Approach({e:?})"),
                    Order::Orbit(t) => {
                        format!("{marker} Orbit({:?} @ {:.0})", t.entity, t.distance.get())
                    }
                    Order::Mine(e) => format!("{marker} Mine({e:?})"),
                }
            })
            .collect();
        lines.push(format!("Queue ({}):", queue.len()));
        lines.extend(orders);
    }
    text.0 = lines.join("\n");
}
