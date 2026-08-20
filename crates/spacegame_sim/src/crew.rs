//! Crew skeleton — [`Crew`] staffs a [`Ship`] and scales mining.
//!
//! `Crew` is a `Component` on a child entity of a ship (`ChildOf(ship)`).
//! Fatigue ticks up while mining in range, recovers idle, and linearly scales
//! yield/cycle. Skill linearly boosts yield and reduces cycle time. Skeleton
//! sim only — no needs/morale/food.

use bevy::prelude::*;

/// Role of a crew member. Skeleton sim only has `Miner`.
#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub enum CrewRole {
    /// Mining specialist — the only role in slice 1.
    Miner,
}

/// Crew member assigned to a ship via `ChildOf`.
///
/// `skill_mining` in `[0,1]` linearly scales output; `fatigue` in `[0,100]`
/// linearly penalises. Both are validated on construction and clamped on
/// update to prevent negative wares or negative cycles.
#[derive(Debug, Clone, PartialEq, Component, Reflect)]
pub struct Crew {
    /// Role — currently always [`CrewRole::Miner`].
    pub role: CrewRole,
    /// Mining skill `[0,1]`. `1` is best.
    pub skill_mining: f32,
    /// Fatigue `[0,100]`. `0` rested, `100` exhausted.
    pub fatigue: f32,
}

/// Fatigue gained per second while actively mining in range.
pub const FATIGUE_GAIN_PER_SEC: f32 = 5.0;
/// Fatigue recovered per second while idle / out of range / no Mine order.
pub const FATIGUE_RECOVERY_PER_SEC: f32 = 2.5;

/// How much skill boosts yield at `skill=1`. `0.5` means `1.5x` yield.
const SKILL_YIELD_BONUS: f32 = 0.5;
/// How much fatigue penalises yield at `fatigue=100`. `0.5` means `0.5x` yield.
const FATIGUE_YIELD_PENALTY: f32 = 0.5;
/// How much skill reduces cycle at `skill=1`. `0.2` means `0.8x` cycle.
const SKILL_CYCLE_REDUCTION: f32 = 0.2;
/// How much fatigue increases cycle at `fatigue=100`. `0.5` means `1.5x` cycle.
const FATIGUE_CYCLE_PENALTY: f32 = 0.5;

impl Crew {
    /// Create a validated crew member.
    ///
    /// Clamps `skill_mining` to `[0,1]` and `fatigue` to `[0,100]` — never
    /// panics on bad input, but asserts in debug for programmer errors where
    /// values are far out of range (err-result-over-panic: prefer clamp over
    /// panic in prod).
    #[must_use]
    pub fn new(role: CrewRole, skill_mining: f32, fatigue: f32) -> Self {
        // num-float-compare: never == on f32; clamp handles NaN as 0 after is_finite check.
        let skill = if skill_mining.is_finite() {
            skill_mining.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fat = if fatigue.is_finite() {
            fatigue.clamp(0.0, 100.0)
        } else {
            0.0
        };
        // debug_assert for ordering invariants in debug builds; prod clamps.
        debug_assert!(
            (0.0..=1.0).contains(&skill),
            "skill_mining {skill} must be in [0,1]"
        );
        debug_assert!(
            (0.0..=100.0).contains(&fat),
            "fatigue {fat} must be in [0,100]"
        );
        Self {
            role,
            skill_mining: skill,
            fatigue: fat,
        }
    }

    /// Convenience for a fresh miner.
    #[must_use]
    pub fn miner(skill_mining: f32) -> Self {
        Self::new(CrewRole::Miner, skill_mining, 0.0)
    }
}

/// Linearly scaled yield: `base * (1 + skill*0.5) * (1 - fatigue/100*0.5)`.
///
/// Clamped to at least `1` when `base > 0` so mining always makes progress,
/// and `0` when `base == 0`. Never negative (no negative wares).
// own-borrow-over-clone: f32 args are Copy
#[must_use]
pub fn effective_yield(base_yield: u32, skill: f32, fatigue: f32) -> u32 {
    if base_yield == 0 {
        return 0;
    }
    let s = skill.clamp(0.0, 1.0);
    let f = fatigue.clamp(0.0, 100.0);
    let skill_factor = 1.0 + s * SKILL_YIELD_BONUS;
    let fatigue_factor = 1.0 - (f / 100.0) * FATIGUE_YIELD_PENALTY;
    // fatigue_factor in [0.5,1.0], skill_factor in [1.0,1.5] => product in [0.5,1.5]
    let scaled = base_yield as f32 * skill_factor * fatigue_factor;
    // Round to nearest, at least 1.
    (scaled.round() as u32).max(1)
}

/// Linearly scaled cycle time: `base * (1 - skill*0.2) * (1 + fatigue/100*0.5)`.
///
/// Lower is faster. Clamped to small epsilon `0.1` to avoid zero/negative cycles.
/// err-result-over-panic: never unwraps, returns finite positive.
#[must_use]
pub fn effective_cycle_secs(base_secs: f32, skill: f32, fatigue: f32) -> f32 {
    let s = skill.clamp(0.0, 1.0);
    let f = fatigue.clamp(0.0, 100.0);
    // base_secs should be >0 per RON validation; if not, clamp to 0.1.
    let base = if base_secs.is_finite() && base_secs > 0.0 {
        base_secs
    } else {
        0.1
    };
    let skill_factor = 1.0 - s * SKILL_CYCLE_REDUCTION;
    let fatigue_factor = 1.0 + (f / 100.0) * FATIGUE_CYCLE_PENALTY;
    (base * skill_factor * fatigue_factor).max(0.1)
}

/// Update fatigue for a tick: `delta = (gain? +GAIN : -RECOVERY) * dt` clamped `[0,100]`.
///
/// `is_mining` true when ship is in `Mine` order, in range, and not cargo-full
/// (i.e. actually cycling). Deterministic helper — no RNG.
#[must_use]
pub fn update_fatigue(current: f32, is_mining: bool, dt: f32) -> f32 {
    let cur = current.clamp(0.0, 100.0);
    let delta = if is_mining {
        FATIGUE_GAIN_PER_SEC * dt
    } else {
        -FATIGUE_RECOVERY_PER_SEC * dt
    };
    (cur + delta).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crew_new_clamps() {
        let c = Crew::new(CrewRole::Miner, 2.0, -10.0);
        assert!((c.skill_mining - 1.0).abs() < f32::EPSILON);
        assert!((c.fatigue - 0.0).abs() < f32::EPSILON);
        let c2 = Crew::new(CrewRole::Miner, f32::NAN, f32::INFINITY);
        assert!((c2.skill_mining - 0.0).abs() < f32::EPSILON);
        assert!((c2.fatigue - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_yield_linear_scaling() {
        // base 10, no skill no fatigue => 10
        assert_eq!(effective_yield(10, 0.0, 0.0), 10);
        // skill 1 => 15
        assert_eq!(effective_yield(10, 1.0, 0.0), 15);
        // fatigue 100 => 5
        assert_eq!(effective_yield(10, 0.0, 100.0), 5);
        // skill 1 fatigue 100 => 10*1.5*0.5=7.5 => 8 rounded
        assert_eq!(effective_yield(10, 1.0, 100.0), 8);
        // never zero when base>0
        assert!(effective_yield(1, 0.0, 100.0) >= 1);
        // base 0 => 0
        assert_eq!(effective_yield(0, 1.0, 0.0), 0);
    }

    #[test]
    fn effective_cycle_linear_scaling() {
        // base 5, no skill no fatigue => 5
        assert!((effective_cycle_secs(5.0, 0.0, 0.0) - 5.0).abs() < 1e-5);
        // skill 1 => 4.0
        assert!((effective_cycle_secs(5.0, 1.0, 0.0) - 4.0).abs() < 1e-5);
        // fatigue 100 => 7.5
        assert!((effective_cycle_secs(5.0, 0.0, 100.0) - 7.5).abs() < 1e-5);
        // skill 1 fatigue 100 => 5*0.8*1.5=6.0
        assert!((effective_cycle_secs(5.0, 1.0, 100.0) - 6.0).abs() < 1e-5);
        // clamped small
        assert!(effective_cycle_secs(0.0, 0.0, 0.0) >= 0.1);
    }

    #[test]
    fn fatigue_ticks_up_and_recovers() {
        let mut f = 0.0;
        // mine for 1 sec => +5
        f = update_fatigue(f, true, 1.0);
        assert!((f - 5.0).abs() < 1e-5);
        // idle 2 sec => -5 => 0 clamped
        f = update_fatigue(f, false, 2.0);
        assert!((f - 0.0).abs() < 1e-5);
        // gain to cap
        f = 99.0;
        f = update_fatigue(f, true, 1.0);
        assert!((f - 100.0).abs() < 1e-5);
        // recovery from 100
        f = update_fatigue(f, false, 1.0);
        assert!((f - 97.5).abs() < 1e-5);
    }

    #[test]
    fn crew_childof_ship_integration() {
        use bevy::prelude::*;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let ship = app.world_mut().spawn_empty().id();
        let crew_entity = app
            .world_mut()
            .spawn((Crew::miner(0.8), ChildOf(ship)))
            .id();
        let child_of = app.world().get::<ChildOf>(crew_entity).unwrap();
        assert_eq!(child_of.parent(), ship);
        let crew = app.world().get::<Crew>(crew_entity).unwrap();
        assert_eq!(crew.role, CrewRole::Miner);
        // skill reflected in yield
        assert!(effective_yield(10, crew.skill_mining, crew.fatigue) > 10);
    }
}
