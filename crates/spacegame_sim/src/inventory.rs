//! Volume-limited inventory for ships/stations.
//!
//! Ware counts are stored as `HashMap<String, u32>` constrained by
//! `cargo_capacity` and per-ware `volume` from `wares.ron` (Ore volume 1.0).
//! All mutations check capacity before adding — never negative wares.

use bevy::prelude::*;
use std::collections::HashMap;

/// Stable ware identifier — alias for `String` to document intent and avoid
/// Stringly-typed APIs (`type-no-stringly`). Slice 2 may promote to
/// `struct WareId(String)` newtype if multi-ware lookups diversify; alias
/// keeps the diff minimal now.
pub type WareId = String;

/// Inventory component holding ware counts.
///
/// Stored on `Ship` entities; `cargo_capacity` lives on [`crate::movement::ShipStats`].
/// Capacity check requires callers to pass `cargo_capacity` and `volume_per_unit`
/// (from `spacegame_data::WaresRegistry`). No hardcoded volume.
#[derive(Debug, Clone, PartialEq, Component, Default)]
pub struct Inventory {
    wares: HashMap<WareId, u32>,
}

impl Inventory {
    /// Empty inventory.
    #[must_use]
    pub fn new() -> Self {
        Self {
            wares: HashMap::new(),
        }
    }

    /// Get count for `ware_id`, `0` if absent. No unwrap.
    #[must_use]
    pub fn get(&self, ware_id: &str) -> u32 {
        self.wares.get(ware_id).copied().unwrap_or(0)
    }

    /// Total cargo volume used: `sum(count * volume_per_unit)`.
    ///
    /// For slice 1 only Ore exists (volume `1.0`), but formula is generic.
    /// `volume_per_unit` is the per-unit volume for the single ware present;
    /// for mixed wares with differing volumes use [`Self::cargo_used_with`]
    /// with a per-ware lookup — `cargo_used`/`free_capacity`/`try_add` must
    /// NOT be used once heterogeneous volumes exist, because they compute
    /// `sum(count) * volume_per_unit` (uniform assumption). Delegates to
    /// `cargo_used_with` for the single-volume case.
    #[must_use]
    pub fn cargo_used(&self, volume_per_unit: f32) -> f32 {
        self.cargo_used_with(|_| volume_per_unit)
    }

    /// Generic cargo used with per-ware volume lookup.
    #[must_use]
    pub fn cargo_used_with(&self, mut volume_for: impl FnMut(&str) -> f32) -> f32 {
        let mut used = 0.0;
        for (id, count) in &self.wares {
            let vol = volume_for(id.as_str());
            used += *count as f32 * vol;
        }
        used
    }

    /// Free capacity: `cargo_capacity - used`. Clamped to `>=0`.
    #[must_use]
    pub fn free_capacity(&self, cargo_capacity: f32, volume_per_unit: f32) -> f32 {
        let used = self.cargo_used(volume_per_unit);
        (cargo_capacity - used).max(0.0)
    }

    /// Free capacity with per-ware volume lookup.
    #[must_use]
    pub fn free_capacity_with(
        &self,
        cargo_capacity: f32,
        volume_for: impl FnMut(&str) -> f32,
    ) -> f32 {
        let used = self.cargo_used_with(volume_for);
        (cargo_capacity - used).max(0.0)
    }

    /// Try to add `amount` of `ware_id`, respecting volume.
    ///
    /// Returns `added` (may be `0` if full, or less than `amount` if partially
    /// filled). Never exceeds capacity, never negative. `volume_per_unit` and
    /// `cargo_capacity` are `f32` — comparison uses epsilon (num-float-compare).
    // own-slice-over-vec: &str not String slice
    pub fn try_add(
        &mut self,
        ware_id: &str,
        amount: u32,
        volume_per_unit: f32,
        cargo_capacity: f32,
    ) -> u32 {
        if amount == 0 {
            return 0;
        }
        // Guard non-finite volumes/capacities — treat as full (err-result-over-panic: no unwrap).
        if !volume_per_unit.is_finite() || !cargo_capacity.is_finite() {
            return 0;
        }
        if volume_per_unit <= 0.0 || cargo_capacity <= 0.0 {
            return 0;
        }
        let free = self.free_capacity(cargo_capacity, volume_per_unit);
        // num-float-compare: use epsilon
        if free < volume_per_unit - f32::EPSILON {
            return 0;
        }
        let max_add = (free / volume_per_unit).floor() as u32;
        let to_add = amount.min(max_add);
        if to_add == 0 {
            return 0;
        }
        let entry = self.wares.entry(ware_id.to_string()).or_insert(0);
        *entry = entry.saturating_add(to_add);
        to_add
    }

    /// Whether inventory is full for `volume_per_unit` (no room for 1 unit).
    #[must_use]
    pub fn is_full(&self, cargo_capacity: f32, volume_per_unit: f32) -> bool {
        self.free_capacity(cargo_capacity, volume_per_unit) < volume_per_unit - f32::EPSILON
    }

    /// Whether empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wares.is_empty() || self.wares.values().all(|&c| c == 0)
    }

    /// Iterate wares.
    pub fn iter(&self) -> impl Iterator<Item = (&WareId, &u32)> {
        self.wares.iter()
    }

    /// Total units across all wares (for invariants).
    #[must_use]
    pub fn total_units(&self) -> u32 {
        self.wares.values().copied().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let inv = Inventory::new();
        assert!(inv.is_empty());
        assert_eq!(inv.get("ore"), 0);
        assert!((inv.cargo_used(1.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn try_add_within_capacity() {
        let mut inv = Inventory::new();
        let added = inv.try_add("ore", 10, 1.0, 100.0);
        assert_eq!(added, 10);
        assert_eq!(inv.get("ore"), 10);
        assert!((inv.cargo_used(1.0) - 10.0).abs() < 1e-5);
    }

    #[test]
    fn try_add_respects_volume_and_capacity() {
        let mut inv = Inventory::new();
        // capacity 100, volume 1 => max 100
        let added = inv.try_add("ore", 60, 1.0, 100.0);
        assert_eq!(added, 60);
        let added2 = inv.try_add("ore", 50, 1.0, 100.0);
        assert_eq!(added2, 40); // only 40 fits
        assert_eq!(inv.get("ore"), 100);
        assert!(inv.is_full(100.0, 1.0));
        // further add => 0
        let added3 = inv.try_add("ore", 10, 1.0, 100.0);
        assert_eq!(added3, 0);
    }

    #[test]
    fn try_add_partial_when_volume_fractional() {
        let mut inv = Inventory::new();
        // volume 0.5 per unit, capacity 10 => max 20 units
        let added = inv.try_add("ore", 25, 0.5, 10.0);
        assert_eq!(added, 20);
        assert_eq!(inv.get("ore"), 20);
    }

    #[test]
    fn no_negative_wares_and_saturating() {
        let mut inv = Inventory::new();
        inv.try_add("ore", u32::MAX, 1.0, 100.0);
        assert_eq!(inv.get("ore"), 100); // clamped by capacity not overflow
        // cargo_used never negative
        assert!(inv.cargo_used(1.0) >= 0.0);
        assert!(inv.free_capacity(100.0, 1.0) >= 0.0);
    }

    #[test]
    fn is_full_uses_epsilon() {
        let mut inv = Inventory::new();
        inv.try_add("ore", 100, 1.0, 100.0);
        assert!(inv.is_full(100.0, 1.0));
        // not full when space remains
        let mut inv2 = Inventory::new();
        inv2.try_add("ore", 99, 1.0, 100.0);
        assert!(!inv2.is_full(100.0, 1.0));
        // fractional volume: capacity 10 volume 0.5 => 20 units fills it
        let mut inv3 = Inventory::new();
        inv3.try_add("ore", 20, 0.5, 10.0);
        assert!(inv3.is_full(10.0, 0.5));
        assert!((inv3.free_capacity(10.0, 0.5) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn cargo_used_with_multi_ware_volumes() {
        let mut inv = Inventory::new();
        inv.try_add("ore", 10, 1.0, 100.0);
        // Simulate second ware with different volume via cargo_used_with
        inv.wares.insert("fuel".to_string(), 5);
        let used = inv.cargo_used_with(|id| if id == "ore" { 1.0 } else { 2.0 });
        // ore 10*1 + fuel 5*2 = 20
        assert!((used - 20.0).abs() < 1e-5);
    }
}
