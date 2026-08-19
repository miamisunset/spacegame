//! Ordered [`SystemSet`]s for the deterministic sim tick.
//!
//! All sets run on `FixedUpdate` and are gated by
//! `in_state(GameState::Simulating)` in [`crate::plugin::SimPlugin`].
//! Chain: `Economy -> Ai -> Movement -> Mining -> Combat` per
//! `AGENTS.md` Simulation Conventions. `MiningSet`/`CombatSet` are
//! deferred in slice 1 but declared now so the chain is stable and
//! slice-2 systems can insert without reordering.

use bevy::prelude::*;

/// Economy tick (market, inventory). Ordered first.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EconomySet;

/// AI tick (orders, autonomy). After economy.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AiSet;

/// Kinematic steering (seek/arrive + orbit). After AI.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovementSet;

/// Mining cycles (range-checked, fatigue-scaled). After movement so range is fresh.
///
/// Deferred in slice 1 — declared now to lock the chain order per `AGENTS.md`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MiningSet;

/// Combat (deferred in slice 1). Ordered last.
///
/// Declared now so the 5-set chain is stable; not speculative.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;
