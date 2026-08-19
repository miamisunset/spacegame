//! Deterministic headless simulation — no bevy_render / bevy_pbr dependency.
//! Runs on FixedUpdate, seeded WyRand, data-driven via spacegame_data RON templates.
//!
//! Slice 1 tracer: FIFO [`OrderQueue`] ticking on [`FixedUpdate`] with
//! [`GameState::Simulating`] gating and ordered [`SystemSet`]s.
//!
//! Modules are split by feature to avoid Divergent Change: `state` owns
//! the state machine, `sets` owns schedule ordering, `order` owns the
//! queue, `plugin` wires them. Public API is re-exported here.
//! Speculative Generality is intentional: `MiningSet`/`CombatSet` and
//! `GameState::Paused` lock the 5-set chain and gating proof now so
//! slice-2 systems insert without reordering.

mod order;
mod plugin;
mod sets;
mod state;

pub use order::{OrbitTarget, Order, OrderQueue};
pub use plugin::SimPlugin;
pub use sets::{AiSet, CombatSet, EconomySet, MiningSet, MovementSet};
pub use state::GameState;
