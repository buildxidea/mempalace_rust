/// dynamics.rs — Living-connection math for halls + tunnels.
///
/// Hebbian potentiation (strength grows on co-access) and Ebbinghaus exponential
/// decay (strength fades with time since last activation), with the Cepeda
/// spacing effect: stability grows when reinforcement is spaced rather than
/// massed.
///
/// This module is pure. No I/O, no DB, no chromadb. It operates on
/// [`ConnectionDynamics`] structs and mutates them in place. Callers in
/// `palace_graph` and related modules invoke these functions; the math lives
/// here in one place so both connection kinds share identical semantics.
///
/// Research grounding:
///     - Hebb (1949): "neurons that fire together, wire together" -> potentiation
///     - Ebbinghaus (1885): exponential forgetting curve -> `apply_decay`
///     - Cepeda et al. (2006): spacing effect -> stability growth on spaced reinforcement
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tunable constants. Hardcoded for v1; future PRs may expose via
// MempalaceConfig if real-palace empirical tuning calls for it.
// ---------------------------------------------------------------------------

/// Lower bound on strength. Connections never decay below this — they
/// become dim but remain queryable explicitly. The palace doesn't forget;
/// salience just drops.
pub const STRENGTH_FLOOR: f64 = 0.05;

/// Upper bound on strength. Caps so super-frequently-used connections
/// don't dominate ranking entirely. Above this, the connection is "fully
/// present" — further potentiation is a no-op.
pub const MAX_STRENGTH: f64 = 5.0;

/// Initial stability for a newly-created connection. Higher = slower decay.
/// Grows with spaced reinforcement (Cepeda spacing effect).
pub const DEFAULT_STABILITY: f64 = 1.0;

/// Initial strength for a newly-created connection. Treats new halls/tunnels
/// as "normally present" — neither hot nor cold.
pub const DEFAULT_STRENGTH: f64 = 1.0;

/// How much strength increases on each co-access event. Tuned so that
/// ~20 co-accesses bring a fresh connection to MAX_STRENGTH.
pub const POTENTIATION_INCREMENT: f64 = 0.05;

/// Minimum gap (in hours) between potentiations to count as 'spaced'
/// reinforcement. Bursts of rapid co-access don't build stability;
/// distributed practice does.
pub const SPACED_INTERVAL_HOURS: f64 = 1.0;

/// How much stability grows on each spaced reinforcement. Tuned so a
/// connection reinforced once a day for ~30 days roughly doubles its
/// stability — making it durable against weeks of neglect.
pub const STABILITY_INCREMENT: f64 = 0.1;

// ---------------------------------------------------------------------------
// ConnectionDynamics struct
// ---------------------------------------------------------------------------

/// Dynamics fields for a hall or tunnel connection.
///
/// All fields are default-safe — construct via [`ConnectionDynamics::new`]
/// with the connection's `created_at` timestamp to get sensible defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionDynamics {
    /// Hebbian connection weight, floored at [`STRENGTH_FLOOR`], capped at
    /// [`MAX_STRENGTH`].
    pub strength: f64,

    /// Decay resistance; grows with spaced reinforcement (Cepeda spacing
    /// effect).
    pub stability: f64,

    /// ISO datetime of last co-access event; updates on potentiation.
    pub last_activated: DateTime<Utc>,

    /// Cumulative co-access events.
    pub access_count: u64,
}

impl ConnectionDynamics {
    /// Create a new `ConnectionDynamics` with default values.
    ///
    /// `created_at` is the connection's creation timestamp — used as the
    /// initial value for `last_activated` so decay starts from creation,
    /// not from initialization-call-time.
    pub fn new(created_at: DateTime<Utc>) -> Self {
        Self {
            strength: DEFAULT_STRENGTH,
            stability: DEFAULT_STABILITY,
            last_activated: created_at,
            access_count: 0,
        }
    }

    /// Strengthen the connection on a co-access event.
    ///
    /// Updates `strength` (capped at [`MAX_STRENGTH`]), `last_activated`,
    /// and `access_count`. Grows `stability` by [`STABILITY_INCREMENT`]
    /// only if the gap since the prior activation is at least
    /// [`SPACED_INTERVAL_HOURS`] (the Cepeda spacing effect — rapid bursts
    /// don't build durability; distributed practice does).
    ///
    /// `now` is dependency injection for tests; defaults to current UTC time.
    pub fn potentiate(&mut self, now: Option<DateTime<Utc>>) {
        let now = now.unwrap_or_else(Utc::now);

        // Compute the gap since the last activation to decide if this counts
        // as spaced reinforcement.
        let hours_since = (now - self.last_activated).num_seconds() as f64 / 3600.0;

        // Strength grows by increment, capped at MAX_STRENGTH.
        self.strength = (self.strength + POTENTIATION_INCREMENT).min(MAX_STRENGTH);

        // Spacing effect: only grow stability when reinforcement is spaced.
        if hours_since >= SPACED_INTERVAL_HOURS {
            self.stability += STABILITY_INCREMENT;
        }

        // Always update last_activated and the cumulative counter.
        self.last_activated = now;
        self.access_count = self.access_count.saturating_add(1);
    }

    /// Apply Ebbinghaus exponential decay to the connection's strength.
    ///
    /// The decay model is `new = old * exp(-days_since_last / stability)`,
    /// floored at [`STRENGTH_FLOOR`] so connections never reach zero. Higher
    /// stability = slower decay (the Cepeda principle: spaced reinforcement
    /// builds durability).
    ///
    /// Idempotent at the same instant — calling twice at the same `now`
    /// without a potentiation in between produces the same final strength.
    ///
    /// `now` is dependency injection for tests; defaults to current UTC time.
    pub fn apply_decay(&mut self, now: Option<DateTime<Utc>>) {
        let now = now.unwrap_or_else(Utc::now);

        let days_since = (now - self.last_activated).num_seconds() as f64 / 86400.0;
        if days_since <= 0.0 {
            // No time has passed (or clock skew); idempotent — return unchanged.
            return;
        }

        // Defensive: if stability is somehow zero or negative, use the default.
        let stability = if self.stability <= 0.0 {
            DEFAULT_STABILITY
        } else {
            self.stability
        };

        let decay_factor = (-days_since / stability).exp();
        let new_strength = self.strength * decay_factor;

        self.strength = new_strength.max(STRENGTH_FLOOR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// Create a fresh `ConnectionDynamics` at `now` for testing.
    fn make_dynamics(now: DateTime<Utc>) -> ConnectionDynamics {
        ConnectionDynamics::new(now)
    }

    // -----------------------------------------------------------------------
    // new() — default-safe construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_dynamics_has_default_values() {
        let now = Utc::now();
        let d = ConnectionDynamics::new(now);
        assert!((d.strength - DEFAULT_STRENGTH).abs() < 1e-9);
        assert!((d.stability - DEFAULT_STABILITY).abs() < 1e-9);
        assert_eq!(d.last_activated, now);
        assert_eq!(d.access_count, 0);
    }

    #[test]
    fn test_new_uses_created_at_for_last_activated() {
        let past = Utc::now() - Duration::hours(5);
        let d = ConnectionDynamics::new(past);
        assert_eq!(d.last_activated, past);
    }

    // -----------------------------------------------------------------------
    // potentiate() — Hebbian strengthening
    // -----------------------------------------------------------------------

    #[test]
    fn test_potentiate_increases_strength() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // Stagger last_activated so the spacing check passes.
        d.last_activated = now - Duration::hours(2);
        let initial = d.strength;
        d.potentiate(Some(now));
        assert!(
            d.strength > initial,
            "strength should increase: {initial} -> {}",
            d.strength
        );
    }

    #[test]
    fn test_potentiate_respects_increment_amount() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::hours(2);
        let before = d.strength;
        d.potentiate(Some(now));
        let expected = before + POTENTIATION_INCREMENT;
        assert!(
            (d.strength - expected).abs() < 1e-12,
            "strength should increase by exactly POTENTIATION_INCREMENT: \
             expected {expected}, got {}",
            d.strength
        );
    }

    #[test]
    fn test_potentiate_caps_at_max_strength() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::hours(2);
        d.strength = MAX_STRENGTH;
        d.potentiate(Some(now));
        assert!(
            (d.strength - MAX_STRENGTH).abs() < 1e-9,
            "strength should not exceed MAX_STRENGTH, got {}",
            d.strength
        );
    }

    #[test]
    fn test_potentiate_approaches_max_strength() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // Just below max — one more increment should land at max, not overshoot.
        d.last_activated = now - Duration::hours(2);
        d.strength = MAX_STRENGTH - 0.001;
        d.potentiate(Some(now));
        assert!(
            d.strength <= MAX_STRENGTH,
            "strength should never exceed MAX_STRENGTH, got {}",
            d.strength
        );
    }

    #[test]
    fn test_potentiate_increases_access_count() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::hours(2);
        assert_eq!(d.access_count, 0);
        d.potentiate(Some(now));
        assert_eq!(d.access_count, 1);
        d.potentiate(Some(now));
        assert_eq!(d.access_count, 2);
    }

    #[test]
    fn test_potentiate_updates_last_activated() {
        let now = Utc::now();
        let past = now - Duration::hours(5);
        let mut d = make_dynamics(past);
        d.potentiate(Some(now));
        assert_eq!(d.last_activated, now);
    }

    #[test]
    fn test_potentiate_defaults_to_utc_now() {
        let before = Utc::now();
        let mut d = make_dynamics(before - Duration::hours(2));
        d.potentiate(None);
        let after = Utc::now();
        // Should fall between before and after (inclusive of before).
        assert!(
            d.last_activated >= before && d.last_activated <= after,
            "last_activated should be set to approx Utc::now(): got {}",
            d.last_activated
        );
    }

    // -----------------------------------------------------------------------
    // spacing effect — stability grows only on spaced reinforcement
    // -----------------------------------------------------------------------

    #[test]
    fn test_potentiate_spaced_grows_stability() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::hours(5);
        let initial = d.stability;
        d.potentiate(Some(now));
        assert!(
            d.stability > initial,
            "stability should grow on spaced reinforcement: {initial} -> {}",
            d.stability
        );
    }

    #[test]
    fn test_potentiate_massed_does_not_grow_stability() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // last_activated is the same instant — no spacing at all.
        let initial = d.stability;
        d.potentiate(Some(now));
        assert!(
            (d.stability - initial).abs() < 1e-12,
            "stability should NOT grow on massed reinforcement (gap < 1h)"
        );
    }

    #[test]
    fn test_potentiate_sub_threshold_no_stability_growth() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // 30 minutes — below the 1-hour threshold.
        d.last_activated = now - Duration::minutes(30);
        let initial = d.stability;
        d.potentiate(Some(now));
        assert!(
            (d.stability - initial).abs() < 1e-12,
            "stability should NOT grow for 30-minute gap (below 1h threshold)"
        );
    }

    #[test]
    fn test_potentiate_at_threshold_grows_stability() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // Exactly at the 1-hour threshold.
        d.last_activated = now - Duration::hours(1);
        let initial = d.stability;
        d.potentiate(Some(now));
        assert!(
            d.stability > initial,
            "stability should grow when gap >= 1h"
        );
    }

    // -----------------------------------------------------------------------
    // apply_decay() — Ebbinghaus exponential decay
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_decay_fresh_no_decay() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.strength = 1.0;
        d.apply_decay(Some(now));
        assert!(
            (d.strength - 1.0).abs() < 1e-9,
            "fresh connection should not decay, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_reduces_strength_over_time() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::days(30);
        d.strength = MAX_STRENGTH;
        d.stability = 10.0;
        d.apply_decay(Some(now));
        assert!(
            d.strength < MAX_STRENGTH,
            "strength should decay over time, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_decay_factor_is_exponential() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // With default stability (1.0), after 21 days: exp(-21) ≈ 7.6e-10,
        // which floors to STRENGTH_FLOOR.
        d.last_activated = now - Duration::days(21);
        d.strength = 5.0;
        d.stability = 1.0;
        d.apply_decay(Some(now));
        assert!(
            (d.strength - STRENGTH_FLOOR).abs() < 1e-9,
            "after ~21 days with stability=1, strength should floor, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_floor_never_below_strength_floor() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // Extreme time and low stability should still floor, not go to zero.
        d.last_activated = now - Duration::days(10_000);
        d.strength = 1.0;
        d.stability = 1.0;
        d.apply_decay(Some(now));
        assert!(
            d.strength >= STRENGTH_FLOOR,
            "strength should not drop below floor, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_idempotent_at_same_instant() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::days(10);
        d.strength = 2.0;
        d.apply_decay(Some(now));
        let after_first = d.strength;
        d.apply_decay(Some(now));
        let after_second = d.strength;
        assert!(
            (after_first - after_second).abs() < 1e-12,
            "decay should be idempotent at same instant: \
             first={after_first}, second={after_second}"
        );
    }

    #[test]
    fn test_apply_decay_higher_stability_slower_decay() {
        let now = Utc::now();
        let past = now - Duration::days(10);
        let mut low_stab = make_dynamics(now);
        low_stab.last_activated = past;
        low_stab.strength = DEFAULT_STRENGTH;
        low_stab.stability = 5.0;

        let mut high_stab = make_dynamics(now);
        high_stab.last_activated = past;
        high_stab.strength = DEFAULT_STRENGTH;
        high_stab.stability = 50.0;

        low_stab.apply_decay(Some(now));
        high_stab.apply_decay(Some(now));

        assert!(
            high_stab.strength > low_stab.strength,
            "higher stability should decay slower: high={}, low={}",
            high_stab.strength,
            low_stab.strength
        );
    }

    #[test]
    fn test_apply_decay_zero_stability_falls_back_to_default() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::days(5);
        d.strength = DEFAULT_STRENGTH;
        d.stability = 0.0;
        // Should not panic; should fall back to DEFAULT_STABILITY.
        d.apply_decay(Some(now));
        assert!(
            d.strength < DEFAULT_STRENGTH,
            "strength should decay when stability is 0, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_negative_stability_falls_back_to_default() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        d.last_activated = now - Duration::days(5);
        d.strength = DEFAULT_STRENGTH;
        d.stability = -1.0;
        d.apply_decay(Some(now));
        assert!(
            d.strength < DEFAULT_STRENGTH,
            "strength should decay when stability is negative, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_clock_skew_no_change() {
        let now = Utc::now();
        let mut d = make_dynamics(now);
        // Future last_activated => negative elapsed time => idempotent.
        d.last_activated = now + Duration::hours(1);
        d.strength = 1.0;
        d.apply_decay(Some(now));
        assert!(
            (d.strength - 1.0).abs() < 1e-9,
            "clock skew should not change strength, got {}",
            d.strength
        );
    }

    #[test]
    fn test_apply_decay_defaults_to_utc_now() {
        // apply_decay does not mutate last_activated, but when called with
        // `None` (no explicit now) it should use the current time internally
        // and produce a decayed strength value, not an idempotent no-op.
        let before = Utc::now();
        let mut d = make_dynamics(before - Duration::hours(2));
        d.last_activated = before - Duration::days(10);
        d.strength = DEFAULT_STRENGTH;
        d.apply_decay(None);
        let after = Utc::now();
        // The decay happened (strength decreased), confirming that some
        // `now` between `before` and `after` was used.
        assert!(
            d.strength < DEFAULT_STRENGTH,
            "strength should decay when using implicit Utc::now(), got {}",
            d.strength
        );
    }

    // -----------------------------------------------------------------------
    // serde round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_serde_roundtrip() {
        let now = Utc::now();
        let d = ConnectionDynamics {
            strength: 2.5,
            stability: 3.0,
            last_activated: now,
            access_count: 42,
        };
        let json = serde_json::to_string(&d).unwrap();
        let deserialized: ConnectionDynamics = serde_json::from_str(&json).unwrap();
        assert!((deserialized.strength - d.strength).abs() < 1e-9);
        assert!((deserialized.stability - d.stability).abs() < 1e-9);
        assert_eq!(deserialized.access_count, d.access_count);
        assert_eq!(
            deserialized.last_activated.timestamp(),
            d.last_activated.timestamp()
        );
    }

    #[test]
    fn test_serde_json_field_names() {
        let now = Utc::now();
        let d = ConnectionDynamics::new(now);
        let json = serde_json::to_string(&d).unwrap();
        // Verify expected field names appear in the serialized JSON.
        assert!(json.contains("\"strength\""));
        assert!(json.contains("\"stability\""));
        assert!(json.contains("\"last_activated\""));
        assert!(json.contains("\"access_count\""));
    }

    // -----------------------------------------------------------------------
    // constant value sanity
    // -----------------------------------------------------------------------

    #[test]
    fn test_constants_are_sane() {
        assert!(STRENGTH_FLOOR > 0.0, "floor must be positive");
        assert!(MAX_STRENGTH > STRENGTH_FLOOR, "max must exceed floor");
        assert!(
            DEFAULT_STRENGTH > STRENGTH_FLOOR,
            "default strength must be above floor"
        );
        assert!(
            DEFAULT_STRENGTH < MAX_STRENGTH,
            "default strength must be below max"
        );
        assert!(
            DEFAULT_STABILITY > 0.0,
            "default stability must be positive"
        );
        assert!(
            POTENTIATION_INCREMENT > 0.0,
            "potentiation inc must be positive"
        );
        assert!(
            SPACED_INTERVAL_HOURS > 0.0,
            "spaced interval must be positive"
        );
        assert!(STABILITY_INCREMENT > 0.0, "stability inc must be positive");

        // 80 co-accesses from default to max (5.0 - 1.0 = 4.0, 4.0 / 0.05 = 80).
        let steps_to_max = ((MAX_STRENGTH - DEFAULT_STRENGTH) / POTENTIATION_INCREMENT) as u32;
        assert_eq!(
            steps_to_max, 80,
            "POTENTIATION_INCREMENT = 0.05 should need 80 steps from default=1.0 to max=5.0"
        );
    }
}
