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

mod asteroid;
mod crew;
mod inventory;
mod mining;
mod movement;
mod order;
mod plugin;
mod rng;
mod sets;
mod state;

pub use asteroid::{Asteroid, RESPAWN_DELAY_TICKS, RespawnEntry, RespawnQueue, SimulationTick};
pub use crew::{Crew, CrewRole, FATIGUE_GAIN_PER_SEC, FATIGUE_RECOVERY_PER_SEC};
pub use inventory::{Inventory, WareId};
pub use mining::{MiningLaser, OreVolume};
pub use movement::{ARRIVAL_DISTANCE, ARRIVAL_RADIUS, ShipStats};
pub use order::{OrbitTarget, Order, OrderQueue};
pub use plugin::SimPlugin;
pub use rng::wyrand_next;
pub use sets::{AiSet, CombatSet, EconomySet, MiningSet, MovementSet};
pub use state::GameState;
