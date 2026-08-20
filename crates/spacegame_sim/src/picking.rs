//! Selection and ground picking markers.
//!
//! `SelectedAsteroid` / `LastFlyToPos` are written by pointer picking
//! observers (mesh ray-cast) and consumed by the `OrderQueue` dev menu.
//! Defining them in `spacegame_sim` keeps both `spacegame_render`
//! (which spawns the pickable ground plane) and `spacegame_ui`
//! (which owns the observers) free of a circular dependency.

use bevy::prelude::*;

/// Last asteroid selected by pointer click — used by Approach/Orbit/Mine.
#[derive(Resource, Debug, Clone, Default)]
pub struct SelectedAsteroid(pub Option<Entity>);

/// Last world-space click for `FlyTo Here` — set by ground picking.
#[derive(Resource, Debug, Clone, Default)]
pub struct LastFlyToPos(pub Option<Vec3>);

/// Marker for the ground plane entity used for `FlyTo` world-position picking.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GroundPlane;
