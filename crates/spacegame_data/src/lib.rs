//! Typed RON loaders and registries for data-driven templates.
//!
//! `spacegame_data` owns `thiserror` parse errors for RON and is the source of
//! truth for authored stats; simulation code must never hardcode `speed`,
//! `cargo_capacity` or `mining_range` values — they come from `assets/data/**`.
//!
//! # Examples
//!
//! ```rust no_run
//! use spacegame_data::{parse_ship_ron, parse_wares_ron};
//!
//! let wares = parse_wares_ron(r#"(wares: [(id: "ore", volume: 1.0)])"#).unwrap();
//! assert_eq!(wares.wares[0].id, "ore");
//!
//! let ship = parse_ship_ron(r#"(
//!     id: "miner", speed: 75.0, cargo_capacity: 100.0,
//!     mining_range: 1500.0, cycle_secs: 5.0, yield_per_cycle: 10, orbit_range: 1000.0
//! )"#).unwrap();
//! assert_eq!(ship.id, "miner");
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed parse/load errors for RON templates.
///
/// Library crates use `thiserror` per `err-thiserror-lib`; application edges
/// map these via `anyhow::Context` / `?`.
#[derive(Debug, Error)]
pub enum DataError {
    /// Low-level `ron::Error` (e.g. from `ron::from_str` on a string).
    #[error("ron parse error: {0}")]
    Ron(#[from] ron::Error),

    /// Spanned RON error (line/column annotated).
    #[error("ron spanned error: {0}")]
    Spanned(#[from] ron::error::SpannedError),

    /// IO error when loading a RON file from disk.
    #[error("io error reading {path}: {source}")]
    Io {
        /// File path that failed to read.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

// ---------------------------------------------------------------------------
// Ware templates
// ---------------------------------------------------------------------------

/// Single ware definition authored in `assets/data/wares.ron`.
///
/// `volume` is cargo volume per unit; inventory is constrained by
/// `ship.cargo_capacity` * `volume` on the simulation side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Reflect)]
pub struct WareTemplate {
    /// Stable string id, e.g. `"ore"`.
    pub id: String,
    /// Cargo volume per unit (e.g. `1.0` for Ore).
    pub volume: f32,
}

/// Registry wrapper matching the on-disk shape `(wares: [...])` in
/// `assets/data/wares.ron`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Reflect, Default)]
pub struct WaresRegistry {
    /// All ware definitions.
    pub wares: Vec<WareTemplate>,
}

impl WaresRegistry {
    /// Find a ware by `id`.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&WareTemplate> {
        self.wares.iter().find(|w| w.id == id)
    }
}

// ---------------------------------------------------------------------------
// Ship templates
// ---------------------------------------------------------------------------

/// Minimal mining ship template authored in `assets/data/ships/miner.ron`.
///
/// All stats are data-driven — no simulation code may hardcode them.
/// Fields correspond to slice-1 acceptance criteria:
/// `speed`, `cargo_capacity`, `mining_range`, `cycle_secs`,
/// `yield_per_cycle`, `orbit_range`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Reflect)]
pub struct ShipTemplate {
    /// Stable template id, e.g. `"miner"`.
    pub id: String,
    /// Kinematic speed (units / second) used by seek/arrive steering.
    pub speed: f32,
    /// Total cargo volume capacity.
    pub cargo_capacity: f32,
    /// Maximum range at which the mining laser can cycle.
    pub mining_range: f32,
    /// Seconds per mining cycle (before fatigue scaling).
    pub cycle_secs: f32,
    /// Base ore units yielded per cycle (before skill/fatigue scaling).
    pub yield_per_cycle: u32,
    /// Desired distance to hold while orbiting a target.
    pub orbit_range: f32,
}

// ---------------------------------------------------------------------------
// Parse helpers
// ---------------------------------------------------------------------------

/// Parse a `WaresRegistry` from a RON string.
///
/// # Errors
/// Returns [`DataError::Spanned`] or [`DataError::Ron`] if the RON is invalid.
pub fn parse_wares_ron(ron_str: &str) -> Result<WaresRegistry, DataError> {
    Ok(ron::from_str(ron_str)?)
}

/// Parse a [`ShipTemplate`] from a RON string.
///
/// # Errors
/// Returns [`DataError::Spanned`] or [`DataError::Ron`] if the RON is invalid.
pub fn parse_ship_ron(ron_str: &str) -> Result<ShipTemplate, DataError> {
    Ok(ron::from_str(ron_str)?)
}

fn read_file(path: &std::path::Path) -> Result<String, DataError> {
    std::fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Load a [`WaresRegistry`] from a file on disk.
///
/// # Errors
/// Returns [`DataError::Io`] if the file cannot be read, or a RON error if
/// parsing fails.
pub fn load_wares_file(path: impl AsRef<std::path::Path>) -> Result<WaresRegistry, DataError> {
    let content = read_file(path.as_ref())?;
    parse_wares_ron(&content)
}

/// Load a [`ShipTemplate`] from a file on disk.
///
/// # Errors
/// Returns [`DataError::Io`] if the file cannot be read, or a RON error if
/// parsing fails.
pub fn load_ship_file(path: impl AsRef<std::path::Path>) -> Result<ShipTemplate, DataError> {
    let content = read_file(path.as_ref())?;
    parse_ship_ron(&content)
}

// ---------------------------------------------------------------------------
// Tests — slice-1 data pipeline roundtrip + typed loader
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_wares_ron() -> &'static str {
        r#"(
    wares: [
        (id: "ore", volume: 1.0),
    ],
)"#
    }

    fn sample_miner_ron() -> &'static str {
        r#"(
    id: "miner",
    speed: 75.0,
    cargo_capacity: 100.0,
    mining_range: 1500.0,
    cycle_secs: 5.0,
    yield_per_cycle: 10,
    orbit_range: 1000.0,
)"#
    }

    #[test]
    fn wares_ron_roundtrip() {
        // Arrange
        let parsed = parse_wares_ron(sample_wares_ron()).expect("parse wares");
        // Act
        let serialized = ron::ser::to_string_pretty(&parsed, ron::ser::PrettyConfig::default())
            .expect("serialize");
        let reparsed = parse_wares_ron(&serialized).expect("re-parse");

        // Assert
        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.wares.len(), 1);
        assert_eq!(reparsed.wares[0].id, "ore");
        assert!((reparsed.wares[0].volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ship_miner_ron_roundtrip() {
        // Arrange
        let parsed = parse_ship_ron(sample_miner_ron()).expect("parse miner");
        // Act
        let serialized = ron::ser::to_string_pretty(&parsed, ron::ser::PrettyConfig::default())
            .expect("serialize");
        let reparsed = parse_ship_ron(&serialized).expect("re-parse");

        // Assert — external behavior: all mining-kit fields survive roundtrip
        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.id, "miner");
        assert!((reparsed.speed - 75.0).abs() < f32::EPSILON);
        assert!((reparsed.cargo_capacity - 100.0).abs() < f32::EPSILON);
        assert!((reparsed.mining_range - 1500.0).abs() < f32::EPSILON);
        assert!((reparsed.cycle_secs - 5.0).abs() < f32::EPSILON);
        assert_eq!(reparsed.yield_per_cycle, 10);
        assert!((reparsed.orbit_range - 1000.0).abs() < f32::EPSILON);
    }

    #[test]
    fn invalid_ron_returns_error() {
        let bad = "(wares: [ (id: \"ore\", volume: )] )";
        let err = parse_wares_ron(bad).unwrap_err();
        // Should be a RON parse error, not panic
        assert!(matches!(err, DataError::Spanned(_) | DataError::Ron(_)));
    }

    #[test]
    fn thiserror_display_is_lowercase_no_panic() {
        let err = parse_ship_ron("not ron at all").unwrap_err();
        let msg = err.to_string();
        assert!(!msg.is_empty());
        // err-lowercase-msg: messages start lowercase
        assert!(
            msg.chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || !c.is_alphabetic())
        );
    }

    #[test]
    fn wares_registry_find() {
        let reg = parse_wares_ron(sample_wares_ron()).unwrap();
        assert!(reg.find("ore").is_some());
        assert!(reg.find("missing").is_none());
    }

    #[test]
    fn on_disk_miner_file_loads() {
        // Deterministic path via CARGO_MANIFEST_DIR — no cwd guessing
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = manifest.join("../../assets/data/ships/miner.ron");
        let ship = load_ship_file(&path).expect("miner.ron must exist and parse");
        assert_eq!(ship.id, "miner");
        assert!(ship.speed > 0.0);
        assert!(ship.cargo_capacity > 0.0);
        assert!(ship.mining_range > 0.0);
        assert!(ship.cycle_secs > 0.0);
        assert!(ship.orbit_range > 0.0);
        assert!(ship.yield_per_cycle > 0);
        // mining_range must cover orbit_range per spec (orbit inside mining range)
        assert!(
            ship.mining_range >= ship.orbit_range,
            "mining_range {} must be >= orbit_range {}",
            ship.mining_range,
            ship.orbit_range
        );
    }

    #[test]
    fn on_disk_wares_file_loads() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = manifest.join("../../assets/data/wares.ron");
        let reg = load_wares_file(&path).expect("wares.ron must exist and parse");
        let ore = reg.find("ore").expect("Ore ware must exist");
        assert!((ore.volume - 1.0).abs() < f32::EPSILON);
        // Roundtrip on-disk content
        let serialized =
            ron::ser::to_string_pretty(&reg, ron::ser::PrettyConfig::default()).unwrap();
        let reparsed = parse_wares_ron(&serialized).unwrap();
        assert_eq!(reg, reparsed);
    }
}
