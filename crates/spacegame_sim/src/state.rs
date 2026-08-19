//! Simulation state machine.
//!
//! `GameState` gates all deterministic ticking on [`FixedUpdate`] via
//! `in_state(GameState::Simulating)`. `Paused` exists solely to prove
//! gating in headless tests; the binary will add menu/loading variants
//! later without touching sim systems.

use bevy::prelude::*;

/// Global simulation state. Only [`GameState::Simulating`] ticks sim
/// systems on `FixedUpdate`.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Deterministic simulation is running on `FixedUpdate`.
    #[default]
    Simulating,
    /// Paused — headless test harness to prove `in_state` gating.
    ///
    /// Not speculative: required to verify `FixedUpdate` gating without
    /// a window, and reserved for the future pause/menu state.
    Paused,
}
