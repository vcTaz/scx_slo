//! The feedback controller: fast restriction, slow recovery, with hysteresis.
//!
//! Each tick, [`step`] maps the current [`State`] and an [`Observation`]
//! (p99 over the short and long windows) to the next state. The state carries
//! the two levers the actuator applies to batch tasks: `k`, the number of
//! CPUs they may run on, and `slice_ns`, their time slice.
//!
//! Rules, in priority order:
//!
//! 1. **Restrict.** Short-window p99 above `high_frac * SLO`: cut `k` and the
//!    slice by `cut_factor` at once, then cut again every
//!    `min_ticks_between_cuts` ticks while it stays high. Arms a hold-down.
//! 2. **Hold-down.** For `hold_ticks` ticks after a cut, change nothing. The
//!    windows still contain pre-cut latencies, so recovering now would undo
//!    the cut before its effect is visible.
//! 3. **Recover.** Both p99s known, long-window p99 below `low_frac * SLO`:
//!    every `recover_every_ticks` calm ticks, give back one step, cores
//!    first, then slice.
//! 4. **Hold.** Anything else, including p99 between the two thresholds
//!    (the hysteresis band) and missing data, changes nothing.
//!
//! Missing data holds rather than recovers. A dead probe looks the same as an
//! idle service, and giving CPU back blind is the unsafe choice.

/// SLO used until the `B0-alone` baseline gives a real number.
pub const PLACEHOLDER_SLO_NS: u64 = 500_000; // 500 µs

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Target p99 for the latency-critical service.
    pub slo_ns: u64,
    /// Restrict when short-window p99 exceeds `high_frac * slo_ns`.
    pub high_frac: f64,
    /// Recover only when long-window p99 is below `low_frac * slo_ns`.
    pub low_frac: f64,
    /// Fewest CPUs batch tasks can be confined to (at least 1).
    pub k_min: u32,
    /// All CPUs available to batch tasks.
    pub k_max: u32,
    pub slice_min_ns: u64,
    pub slice_max_ns: u64,
    /// Each cut multiplies `k` and the slice by this (0 < f < 1).
    pub cut_factor: f64,
    /// Ticks between successive cuts while p99 stays high.
    pub min_ticks_between_cuts: u32,
    /// Ticks after a cut during which nothing is given back.
    pub hold_ticks: u32,
    /// Calm ticks needed per recovery step.
    pub recover_every_ticks: u32,
    pub recover_k_step: u32,
    pub recover_slice_step_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    ZeroSlo,
    Thresholds,
    CoreRange,
    SliceRange,
    CutFactor,
    ZeroTicks,
    ZeroSteps,
}

impl Config {
    /// Defaults for a machine where batch tasks may use up to `k_max` CPUs.
    pub fn with_k_max(k_max: u32) -> Self {
        Self {
            slo_ns: PLACEHOLDER_SLO_NS,
            high_frac: 0.9,
            low_frac: 0.7,
            k_min: 1,
            k_max,
            slice_min_ns: 500_000,    // 0.5 ms
            slice_max_ns: 20_000_000, // 20 ms, sched_ext's default slice
            cut_factor: 0.5,
            min_ticks_between_cuts: 2,
            hold_ticks: 5,
            recover_every_ticks: 3,
            recover_k_step: 1,
            recover_slice_step_ns: 2_000_000,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.slo_ns == 0 {
            return Err(ConfigError::ZeroSlo);
        }
        if !(0.0 < self.low_frac && self.low_frac < self.high_frac) {
            return Err(ConfigError::Thresholds);
        }
        if !(1 <= self.k_min && self.k_min <= self.k_max) {
            return Err(ConfigError::CoreRange);
        }
        if !(1 <= self.slice_min_ns && self.slice_min_ns <= self.slice_max_ns) {
            return Err(ConfigError::SliceRange);
        }
        if !(0.0 < self.cut_factor && self.cut_factor < 1.0) {
            return Err(ConfigError::CutFactor);
        }
        if self.min_ticks_between_cuts == 0 || self.recover_every_ticks == 0 {
            return Err(ConfigError::ZeroTicks);
        }
        if self.recover_k_step == 0 || self.recover_slice_step_ns == 0 {
            return Err(ConfigError::ZeroSteps);
        }
        Ok(())
    }

    fn high_ns(&self) -> f64 {
        self.slo_ns as f64 * self.high_frac
    }

    fn low_ns(&self) -> f64 {
        self.slo_ns as f64 * self.low_frac
    }
}

/// p99 latencies for one tick; `None` means too few samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    pub p99_short_ns: Option<u64>,
    pub p99_long_ns: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Restrict,
    Hold,
    Recover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub mode: Mode,
    /// CPUs batch tasks may run on.
    pub k: u32,
    /// Batch time slice.
    pub slice_ns: u64,
    hold_left: u32,
    ticks_since_cut: u32,
    calm_ticks: u32,
}

impl State {
    /// Start fully restricted and earn capacity back, so a cold start
    /// cannot hurt the latency-critical service.
    pub fn initial(cfg: &Config) -> Self {
        Self {
            mode: Mode::Hold,
            k: cfg.k_min,
            slice_ns: cfg.slice_min_ns,
            hold_left: 0,
            ticks_since_cut: u32::MAX,
            calm_ticks: 0,
        }
    }
}

/// Advance the controller by one tick. Pure: no I/O, no clock.
pub fn step(cfg: &Config, s: &State, obs: Observation) -> State {
    let mut next = State {
        ticks_since_cut: s.ticks_since_cut.saturating_add(1),
        ..*s
    };

    let short_high = obs.p99_short_ns.is_some_and(|p| p as f64 > cfg.high_ns());
    if short_high {
        next.mode = Mode::Restrict;
        next.calm_ticks = 0;
        next.hold_left = cfg.hold_ticks;
        if next.ticks_since_cut >= cfg.min_ticks_between_cuts {
            next.k = cut(s.k as u64, cfg.cut_factor, cfg.k_min as u64) as u32;
            next.slice_ns = cut(s.slice_ns, cfg.cut_factor, cfg.slice_min_ns);
            next.ticks_since_cut = 0;
        }
        return next;
    }

    if s.hold_left > 0 {
        next.mode = Mode::Hold;
        next.hold_left = s.hold_left - 1;
        next.calm_ticks = 0;
        return next;
    }

    let calm =
        obs.p99_short_ns.is_some() && obs.p99_long_ns.is_some_and(|p| (p as f64) < cfg.low_ns());
    if !calm {
        next.mode = Mode::Hold;
        next.calm_ticks = 0;
        return next;
    }

    next.mode = Mode::Recover;
    next.calm_ticks = s.calm_ticks + 1;
    if next.calm_ticks >= cfg.recover_every_ticks {
        next.calm_ticks = 0;
        if s.k < cfg.k_max {
            next.k = (s.k + cfg.recover_k_step).min(cfg.k_max);
        } else {
            next.slice_ns = (s.slice_ns + cfg.recover_slice_step_ns).min(cfg.slice_max_ns);
        }
    }
    next
}

/// Multiply by `factor`, but always drop by at least 1 and never below `min`.
fn cut(v: u64, factor: f64, min: u64) -> u64 {
    let scaled = (v as f64 * factor) as u64;
    scaled.min(v.saturating_sub(1)).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(Config::with_k_max(16).validate(), Ok(()));
    }

    #[test]
    fn rejects_bad_configs() {
        let base = Config::with_k_max(8);
        type Case = (fn(&mut Config), ConfigError);
        let cases: [Case; 7] = [
            (|c| c.slo_ns = 0, ConfigError::ZeroSlo),
            (|c| c.low_frac = c.high_frac, ConfigError::Thresholds),
            (|c| c.k_min = 0, ConfigError::CoreRange),
            (
                |c| c.slice_min_ns = c.slice_max_ns + 1,
                ConfigError::SliceRange,
            ),
            (|c| c.cut_factor = 1.0, ConfigError::CutFactor),
            (|c| c.recover_every_ticks = 0, ConfigError::ZeroTicks),
            (|c| c.recover_k_step = 0, ConfigError::ZeroSteps),
        ];
        for (mutate, want) in cases {
            let mut c = base.clone();
            mutate(&mut c);
            assert_eq!(c.validate(), Err(want));
        }
    }

    #[test]
    fn cut_always_makes_progress_and_respects_min() {
        assert_eq!(cut(16, 0.5, 1), 8);
        assert_eq!(cut(3, 0.9, 1), 2); // 2.7 rounds to 2, still a drop
        assert_eq!(cut(2, 0.9, 1), 1);
        assert_eq!(cut(1, 0.5, 1), 1);
        assert_eq!(cut(4, 0.1, 3), 3);
    }
}
