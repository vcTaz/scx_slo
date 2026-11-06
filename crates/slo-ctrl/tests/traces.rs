//! Controller behaviour on synthetic p99 traces (`E-unit` in docs/TAXONOMY.md).

use slo_ctrl::{step, Config, Mode, Observation, State};

const SLO: u64 = 500_000;

fn cfg() -> Config {
    let mut c = Config::with_k_max(16);
    c.slo_ns = SLO;
    c
}

/// p99 as a fraction of the SLO, same value for both windows.
fn at(frac: f64) -> Observation {
    let p = (SLO as f64 * frac) as u64;
    Observation {
        p99_short_ns: Some(p),
        p99_long_ns: Some(p),
    }
}

fn run(cfg: &Config, start: State, trace: impl IntoIterator<Item = Observation>) -> Vec<State> {
    let mut s = start;
    trace
        .into_iter()
        .map(|o| {
            s = step(cfg, &s, o);
            s
        })
        .collect()
}

/// Run calm traffic until the controller has given everything back.
fn fully_recovered(cfg: &Config) -> State {
    let states = run(cfg, State::initial(cfg), std::iter::repeat_n(at(0.3), 500));
    let s = *states.last().unwrap();
    assert_eq!(
        (s.k, s.slice_ns),
        (cfg.k_max, cfg.slice_max_ns),
        "did not recover in 500 ticks"
    );
    s
}

/// Minimal deterministic PRNG so the crate stays dependency-free.
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[test]
fn starts_restricted() {
    let c = cfg();
    let s = State::initial(&c);
    assert_eq!((s.k, s.slice_ns), (c.k_min, c.slice_min_ns));
}

#[test]
fn spike_is_cut_on_the_same_tick() {
    let c = cfg();
    let s0 = fully_recovered(&c);
    let s1 = step(&c, &s0, at(1.2));
    assert_eq!(s1.mode, Mode::Restrict);
    assert_eq!(s1.k, 8);
    assert_eq!(s1.slice_ns, 10_000_000);
}

#[test]
fn sustained_violation_cuts_down_to_the_floor() {
    let c = cfg();
    let states = run(&c, fully_recovered(&c), std::iter::repeat_n(at(1.5), 20));
    let ks: Vec<u32> = states.iter().map(|s| s.k).collect();
    // One cut every `min_ticks_between_cuts` (2) ticks: 16 -> 8 -> 4 -> 2 -> 1.
    assert_eq!(&ks[..8], &[8, 8, 4, 4, 2, 2, 1, 1]);
    assert_eq!(states.last().unwrap().k, c.k_min);
    assert_eq!(states.last().unwrap().slice_ns, c.slice_min_ns);
}

#[test]
fn ramp_triggers_on_first_tick_above_high_threshold() {
    let c = cfg();
    // p99 climbs from 50% to 150% of the SLO in 1% steps.
    let trace: Vec<f64> = (50..=150).map(|p| p as f64 / 100.0).collect();
    let states = run(&c, fully_recovered(&c), trace.iter().map(|&f| at(f)));
    let first_cut = states
        .iter()
        .position(|s| s.mode == Mode::Restrict)
        .unwrap();
    assert!(trace[first_cut] > c.high_frac);
    assert!(trace[first_cut - 1] <= c.high_frac);
}

#[test]
fn nothing_is_given_back_during_hold_down() {
    let c = cfg();
    let mut s = step(&c, &fully_recovered(&c), at(1.2));
    let after_cut = s;
    for _ in 0..c.hold_ticks {
        s = step(&c, &s, at(0.1));
        assert_eq!(s.mode, Mode::Hold);
        assert_eq!((s.k, s.slice_ns), (after_cut.k, after_cut.slice_ns));
    }
    // After the hold-down, calm ticks start recovery.
    let states = run(
        &c,
        s,
        std::iter::repeat_n(at(0.1), c.recover_every_ticks as usize),
    );
    assert_eq!(states.last().unwrap().k, after_cut.k + c.recover_k_step);
}

#[test]
fn recovery_is_gradual_cores_first_then_slice() {
    let c = cfg();
    let s0 = step(&c, &fully_recovered(&c), at(2.0));
    let states = run(&c, s0, std::iter::repeat_n(at(0.3), 400));
    let mut prev = s0;
    for (i, s) in states.iter().enumerate() {
        assert!(
            s.k >= prev.k && s.k - prev.k <= c.recover_k_step,
            "tick {i}: k jumped"
        );
        assert!(s.slice_ns >= prev.slice_ns);
        assert!(s.slice_ns - prev.slice_ns <= c.recover_slice_step_ns);
        if s.slice_ns > prev.slice_ns {
            assert_eq!(s.k, c.k_max, "slice grew before cores were restored");
        }
        prev = *s;
    }
    assert_eq!((prev.k, prev.slice_ns), (c.k_max, c.slice_max_ns));
}

#[test]
fn hysteresis_band_changes_nothing() {
    let c = cfg();
    for start in [State::initial(&c), fully_recovered(&c)] {
        // Between low (0.7) and high (0.9): neither restrict nor recover.
        let states = run(&c, start, std::iter::repeat_n(at(0.8), 200));
        for s in &states {
            assert_eq!(
                (s.k, s.slice_ns, s.mode),
                (start.k, start.slice_ns, Mode::Hold)
            );
        }
    }
}

#[test]
fn short_spike_with_calm_long_window_still_restricts() {
    let c = cfg();
    let s0 = fully_recovered(&c);
    let obs = Observation {
        p99_short_ns: Some(SLO * 2),
        p99_long_ns: Some(SLO / 4),
    };
    assert_eq!(step(&c, &s0, obs).mode, Mode::Restrict);
}

#[test]
fn missing_data_holds() {
    let c = cfg();
    for start in [State::initial(&c), fully_recovered(&c)] {
        let partial = Observation {
            p99_short_ns: None,
            p99_long_ns: Some(SLO / 10),
        };
        let trace =
            std::iter::repeat_n(Observation::default(), 50).chain(std::iter::repeat_n(partial, 50));
        for s in run(&c, start, trace) {
            assert_eq!(
                (s.k, s.slice_ns, s.mode),
                (start.k, start.slice_ns, Mode::Hold)
            );
        }
    }
}

#[test]
fn oscillating_latency_does_not_flap() {
    // p99 alternates just above and just below the high threshold every tick.
    // Each high tick re-arms the hold-down, so k only ever goes down.
    let c = cfg();
    let trace = (0..200).map(|i| if i % 2 == 0 { at(0.95) } else { at(0.85) });
    let states = run(&c, fully_recovered(&c), trace);
    for w in states.windows(2) {
        assert!(
            w[1].k <= w[0].k,
            "k increased while latency kept crossing the threshold"
        );
    }
}

#[test]
fn few_mode_flips_on_noisy_calm_traffic() {
    // Noisy p99 between 20% and 65% of the SLO: always calm, so once at full
    // capacity the controller never restricts.
    let c = cfg();
    let mut rng = Lcg(42);
    let trace: Vec<_> = (0..2000).map(|_| at(0.2 + 0.45 * rng.next_f64())).collect();
    let states = run(&c, fully_recovered(&c), trace);
    assert!(states.iter().all(|s| s.mode == Mode::Recover));
    assert!(states
        .iter()
        .all(|s| s.k == c.k_max && s.slice_ns == c.slice_max_ns));
}

#[test]
fn levers_stay_in_range_on_random_traces() {
    let c = cfg();
    let mut rng = Lcg(7);
    for _ in 0..50 {
        let mut s = State::initial(&c);
        for _ in 0..1000 {
            let pick = |r: f64| {
                if r < 0.1 {
                    None
                } else {
                    Some((SLO as f64 * 2.0 * r) as u64)
                }
            };
            let obs = Observation {
                p99_short_ns: pick(rng.next_f64()),
                p99_long_ns: pick(rng.next_f64()),
            };
            s = step(&c, &s, obs);
            assert!((c.k_min..=c.k_max).contains(&s.k));
            assert!((c.slice_min_ns..=c.slice_max_ns).contains(&s.slice_ns));
        }
    }
}
