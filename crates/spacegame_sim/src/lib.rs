//! Deterministic headless simulation — no bevy_render / bevy_pbr dependency.
//! Runs on FixedUpdate, seeded WyRand, data-driven via spacegame_data RON templates.
use bevy::prelude::*;

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, _app: &mut App) {}
}
