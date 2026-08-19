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
    /// Spanned RON error (line/column annotated) from `ron::from_str`.
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

    /// Domain validation failure (e.g. negative `speed` or `mining_range < orbit_range`).
    #[error("validation failed for {field}: {message}")]
    Validation {
        /// Field or invariant that failed validation.
        field: String,
        /// Human-readable reason (lowercase, no trailing punctuation per `err-lowercase-msg`).
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Domain newtypes — api-newtype-safety, type-newtype-validated, own-copy-small
// ---------------------------------------------------------------------------

/// Cargo volume per unit (or total capacity). Must be finite and > 0.
///
/// Newtype prevents mixing `Volume` with `Distance`/`Speed`/`Secs` at call sites.
/// Construct via [`Volume::new`] for validated values; `Deserialize` is transparent
/// so RON `volume: 1.0` stays ergonomic and is validated centrally in
/// `validate_ware` / `validate_ship` per `api-parse-dont-validate`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Reflect)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Volume(pub f32);

impl Volume {
    /// Create a validated volume.
    ///
    /// # Errors
    /// Returns [`DataError::Validation`] if `value` is not finite or not positive.
    pub fn new(value: f32) -> Result<Self, DataError> {
        ensure_finite_positive(value, "volume")?;
        Ok(Self(value))
    }

    /// Raw `f32` value.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

impl std::fmt::Display for Volume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Volume> for f32 {
    fn from(v: Volume) -> Self {
        v.0
    }
}

impl TryFrom<f32> for Volume {
    type Error = DataError;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Kinematic speed (units / second). Must be finite and > 0.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Reflect)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Speed(pub f32);

impl Speed {
    /// Create a validated speed.
    ///
    /// # Errors
    /// Returns [`DataError::Validation`] if `value` is not finite or not positive.
    pub fn new(value: f32) -> Result<Self, DataError> {
        ensure_finite_positive(value, "speed")?;
        Ok(Self(value))
    }

    /// Raw `f32` value.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

impl std::fmt::Display for Speed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Speed> for f32 {
    fn from(v: Speed) -> Self {
        v.0
    }
}

impl TryFrom<f32> for Speed {
    type Error = DataError;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Distance (world units). Must be finite and > 0. Used for `mining_range` and `orbit_range`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Reflect)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Distance(pub f32);

impl Distance {
    /// Create a validated distance.
    ///
    /// # Errors
    /// Returns [`DataError::Validation`] if `value` is not finite or not positive.
    pub fn new(value: f32) -> Result<Self, DataError> {
        ensure_finite_positive(value, "distance")?;
        Ok(Self(value))
    }

    /// Raw `f32` value.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

impl std::fmt::Display for Distance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Distance> for f32 {
    fn from(v: Distance) -> Self {
        v.0
    }
}

impl TryFrom<f32> for Distance {
    type Error = DataError;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Duration in seconds. Must be finite and > 0. Used for `cycle_secs`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Reflect)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Secs(pub f32);

impl Secs {
    /// Create a validated duration.
    ///
    /// # Errors
    /// Returns [`DataError::Validation`] if `value` is not finite or not positive.
    pub fn new(value: f32) -> Result<Self, DataError> {
        ensure_finite_positive(value, "secs")?;
        Ok(Self(value))
    }

    /// Raw `f32` value.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

impl std::fmt::Display for Secs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Secs> for f32 {
    fn from(v: Secs) -> Self {
        v.0
    }
}

impl TryFrom<f32> for Secs {
    type Error = DataError;
    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
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
    pub volume: Volume,
}

/// Registry wrapper matching the on-disk shape `(wares: [...])` in
/// `assets/data/wares.ron`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Reflect)]
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
    pub speed: Speed,
    /// Total cargo volume capacity.
    pub cargo_capacity: Volume,
    /// Maximum range at which the mining laser can cycle.
    pub mining_range: Distance,
    /// Seconds per mining cycle (before fatigue scaling).
    pub cycle_secs: Secs,
    /// Base ore units yielded per cycle (before skill/fatigue scaling).
    pub yield_per_cycle: u32,
    /// Desired distance to hold while orbiting a target.
    pub orbit_range: Distance,
}

// ---------------------------------------------------------------------------
// Validation (err-custom-type, err-lowercase-msg, num-float-compare)
// ---------------------------------------------------------------------------

fn validation_err(field: impl Into<String>, message: impl Into<String>) -> DataError {
    DataError::Validation {
        field: field.into(),
        message: message.into(),
    }
}

fn ensure_finite_positive(value: f32, field: &str) -> Result<(), DataError> {
    if !value.is_finite() {
        return Err(validation_err(field, "must be finite"));
    }
    if value <= 0.0 {
        return Err(validation_err(field, "must be positive"));
    }
    Ok(())
}

fn validate_ware(ware: &WareTemplate) -> Result<(), DataError> {
    if ware.id.trim().is_empty() {
        return Err(validation_err("id", "must be non-empty"));
    }
    ensure_finite_positive(ware.volume.get(), "volume")?;
    Ok(())
}

fn validate_wares_registry(reg: &WaresRegistry) -> Result<(), DataError> {
    for ware in &reg.wares {
        validate_ware(ware)?;
    }
    Ok(())
}

fn validate_ship(ship: &ShipTemplate) -> Result<(), DataError> {
    if ship.id.trim().is_empty() {
        return Err(validation_err("id", "must be non-empty"));
    }
    ensure_finite_positive(ship.speed.get(), "speed")?;
    ensure_finite_positive(ship.cargo_capacity.get(), "cargo_capacity")?;
    ensure_finite_positive(ship.mining_range.get(), "mining_range")?;
    ensure_finite_positive(ship.cycle_secs.get(), "cycle_secs")?;
    ensure_finite_positive(ship.orbit_range.get(), "orbit_range")?;
    if ship.yield_per_cycle == 0 {
        return Err(validation_err("yield_per_cycle", "must be positive"));
    }
    if ship.mining_range < ship.orbit_range {
        return Err(validation_err("mining_range", "must be >= orbit_range"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parse helpers
// ---------------------------------------------------------------------------

/// Parse a `WaresRegistry` from a RON string.
///
/// # Errors
/// Returns [`DataError::Spanned`] if the RON is invalid, or
/// [`DataError::Validation`] if domain invariants are violated.
pub fn parse_wares_ron(ron_str: &str) -> Result<WaresRegistry, DataError> {
    let reg: WaresRegistry = ron::from_str(ron_str)?;
    validate_wares_registry(&reg)?;
    Ok(reg)
}

/// Parse a [`ShipTemplate`] from a RON string.
///
/// # Errors
/// Returns [`DataError::Spanned`] if the RON is invalid, or
/// [`DataError::Validation`] if domain invariants are violated (e.g.
/// negative `speed` or `mining_range < orbit_range`).
pub fn parse_ship_ron(ron_str: &str) -> Result<ShipTemplate, DataError> {
    let ship: ShipTemplate = ron::from_str(ron_str)?;
    validate_ship(&ship)?;
    Ok(ship)
}

fn read_file(path: &std::path::Path) -> Result<String, DataError> {
    std::fs::read_to_string(path).map_err(|source| DataError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn load_via_parse<T, F>(path: &std::path::Path, parse: F) -> Result<T, DataError>
where
    F: Fn(&str) -> Result<T, DataError>,
{
    let content = read_file(path)?;
    parse(&content)
}

/// Load a [`WaresRegistry`] from a file on disk.
///
/// # Errors
/// Returns [`DataError::Io`] if the file cannot be read, or a RON/validation
/// error if parsing fails.
pub fn load_wares_file(path: impl AsRef<std::path::Path>) -> Result<WaresRegistry, DataError> {
    load_via_parse(path.as_ref(), parse_wares_ron)
}

/// Load a [`ShipTemplate`] from a file on disk.
///
/// # Errors
/// Returns [`DataError::Io`] if the file cannot be read, or a RON/validation
/// error if parsing fails.
pub fn load_ship_file(path: impl AsRef<std::path::Path>) -> Result<ShipTemplate, DataError> {
    load_via_parse(path.as_ref(), parse_ship_ron)
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
        assert!((reparsed.wares[0].volume.get() - 1.0).abs() < f32::EPSILON);
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
        assert!((reparsed.speed.get() - 75.0).abs() < f32::EPSILON);
        assert!((reparsed.cargo_capacity.get() - 100.0).abs() < f32::EPSILON);
        assert!((reparsed.mining_range.get() - 1500.0).abs() < f32::EPSILON);
        assert!((reparsed.cycle_secs.get() - 5.0).abs() < f32::EPSILON);
        assert_eq!(reparsed.yield_per_cycle, 10);
        assert!((reparsed.orbit_range.get() - 1000.0).abs() < f32::EPSILON);
    }

    #[test]
    fn invalid_ron_returns_error() {
        let bad = "(wares: [ (id: \"ore\", volume: )] )";
        let err = parse_wares_ron(bad).unwrap_err();
        // Should be a RON parse error, not panic
        assert!(matches!(err, DataError::Spanned(_)));
    }

    #[test]
    fn wares_validation_rejects_negative_volume() {
        let bad = r#"(wares: [(id: "ore", volume: -1.0)])"#;
        let err = parse_wares_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
        let msg = err.to_string();
        assert!(msg.contains("volume"));
    }

    #[test]
    fn wares_validation_rejects_non_finite_volume() {
        let bad = r#"(wares: [(id: "ore", volume: NaN)])"#;
        // ron parses NaN as float; validation must reject
        let err = parse_wares_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
    }

    #[test]
    fn wares_validation_rejects_empty_id() {
        let bad = r#"(wares: [(id: "", volume: 1.0)])"#;
        let err = parse_wares_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
    }

    #[test]
    fn ship_validation_rejects_negative_speed() {
        let bad = r#"(
            id: "miner", speed: -10.0, cargo_capacity: 100.0,
            mining_range: 1500.0, cycle_secs: 5.0, yield_per_cycle: 10, orbit_range: 1000.0
        )"#;
        let err = parse_ship_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
        assert!(err.to_string().contains("speed"));
    }

    #[test]
    fn ship_validation_rejects_mining_range_lt_orbit_range() {
        let bad = r#"(
            id: "miner", speed: 75.0, cargo_capacity: 100.0,
            mining_range: 500.0, cycle_secs: 5.0, yield_per_cycle: 10, orbit_range: 1000.0
        )"#;
        let err = parse_ship_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { ref field, .. } if field == "mining_range"));
    }

    #[test]
    fn ship_validation_rejects_zero_yield() {
        let bad = r#"(
            id: "miner", speed: 75.0, cargo_capacity: 100.0,
            mining_range: 1500.0, cycle_secs: 5.0, yield_per_cycle: 0, orbit_range: 1000.0
        )"#;
        let err = parse_ship_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
    }

    #[test]
    fn ship_validation_rejects_non_finite_cycle_secs() {
        let bad = r#"(
            id: "miner", speed: 75.0, cargo_capacity: 100.0,
            mining_range: 1500.0, cycle_secs: inf, yield_per_cycle: 10, orbit_range: 1000.0
        )"#;
        let err = parse_ship_ron(bad).unwrap_err();
        assert!(matches!(err, DataError::Validation { .. }));
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
    fn newtype_constructors_validate() {
        // Volume/Speed/Distance/Secs::new enforce finite positive per type-newtype-validated
        assert!(Volume::new(1.0).is_ok());
        assert!(Volume::new(-1.0).is_err());
        assert!(Volume::new(f32::NAN).is_err());
        assert!(Volume::new(f32::INFINITY).is_err());

        assert!(Speed::new(75.0).is_ok());
        assert!(Speed::new(0.0).is_err());

        assert!(Distance::new(1500.0).is_ok());
        assert!(Distance::new(-5.0).is_err());

        assert!(Secs::new(5.0).is_ok());
        assert!(Secs::new(0.0).is_err());
    }

    #[test]
    fn newtype_try_from_and_display() {
        let v: Volume = Volume::try_from(1.0).unwrap();
        assert!((f32::from(v) - 1.0).abs() < f32::EPSILON);
        assert_eq!(v.to_string(), "1");

        let s: Speed = 75.0.try_into().unwrap();
        assert!((s.get() - 75.0).abs() < f32::EPSILON);
    }

    #[test]
    fn newtype_prevents_mixing_at_type_level() {
        // The following would not compile if uncommented, proving primitive obsession is fixed:
        // let v: Volume = Volume(1.0);
        // let d: Distance = v; // error: mismatched types
        // This test ensures the distinct types exist and are not interchangeable.
        let vol = Volume::new(1.0).unwrap();
        let dist = Distance::new(1000.0).unwrap();
        // Access via typed getters keeps domain semantics explicit
        assert!(vol.get() != dist.get());
        // Different newtypes have different sizes but same layout (transparent)
        assert_eq!(std::mem::size_of::<Volume>(), std::mem::size_of::<f32>());
        assert_eq!(std::mem::size_of::<Speed>(), std::mem::size_of::<f32>());
    }

    #[test]
    fn on_disk_miner_file_loads() {
        // Deterministic path via CARGO_MANIFEST_DIR — no cwd guessing
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = manifest.join("../../assets/data/ships/miner.ron");
        let ship = load_ship_file(&path).expect("miner.ron must exist and parse");
        assert_eq!(ship.id, "miner");
        assert!(ship.speed.get() > 0.0);
        assert!(ship.cargo_capacity.get() > 0.0);
        assert!(ship.mining_range.get() > 0.0);
        assert!(ship.cycle_secs.get() > 0.0);
        assert!(ship.orbit_range.get() > 0.0);
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
        assert!((ore.volume.get() - 1.0).abs() < f32::EPSILON);
        // Roundtrip on-disk content
        let serialized =
            ron::ser::to_string_pretty(&reg, ron::ser::PrettyConfig::default()).unwrap();
        let reparsed = parse_wares_ron(&serialized).unwrap();
        assert_eq!(reg, reparsed);
    }
}
