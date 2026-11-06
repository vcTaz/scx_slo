//! A deliberately simple co-location model, used to exercise the real
//! controller in a closed loop before the CloudLab testbed exists.
//!
//! memcached is one M/M/1 queue. Batch tasks on `k` of `cpus` CPUs with time
//! slice `slice` cause interference (shared caches, SMT siblings, run-queue
//! wait) that slows memcached's service rate:
//!
//! ```text
//! I      = (k / cpus) * (0.5 + 0.5 * slice / slice_max)     interference, 0..1
//! mu_eff = mu0 * (1 - gamma * I)                             memcached service rate
//! T      ~ Exp(mu_eff - lambda)                              M/M/1 response time
//! ```
//!
//! Load is a fraction of peak, where peak is the highest arrival rate at
//! which memcached alone meets the SLO (the `B0-alone` definition).
//!
//! Batch throughput is `k * slice / (slice + switch_overhead)`, in
//! core-equivalents. None of the constants are measured; they only make the
//! trade-off visible. See docs/SIMULATION.md.

use slo_ctrl::{step, Config, Histogram, Mode, State, Windows};

#[derive(Clone, Debug)]
pub struct Model {
    pub cpus: u32,
    /// memcached service rate with no interference, requests/s.
    pub mu0: f64,
    /// Fraction of `mu0` lost at full interference.
    pub gamma: f64,
    /// Per-slice cost for batch tasks (context switch, cache refill).
    pub switch_overhead_ns: f64,
    pub tick_s: f64,
    pub slo_ns: u64,
    pub slice_max_ns: u64,
}

impl Model {
    pub fn default_for(cfg: &Config) -> Self {
        Self {
            cpus: cfg.k_max,
            mu0: 50_000.0, // 20 µs mean service time
            gamma: 0.45,
            switch_overhead_ns: 100_000.0,
            tick_s: 0.1,
            slo_ns: cfg.slo_ns,
            slice_max_ns: cfg.slice_max_ns,
        }
    }

    /// Arrival rate at which memcached alone has p99 exactly at the SLO.
    pub fn peak_lambda(&self) -> f64 {
        // p99 of Exp(r) is ln(100) / r; solve ln(100) / (mu0 - lambda) = SLO.
        self.mu0 - 100f64.ln() / (self.slo_ns as f64 * 1e-9)
    }

    pub fn interference(&self, k: u32, slice_ns: u64) -> f64 {
        let slice = slice_ns as f64 / self.slice_max_ns as f64;
        (k as f64 / self.cpus as f64) * (0.5 + 0.5 * slice)
    }

    pub fn batch_work(&self, k: u32, slice_ns: u64) -> f64 {
        let s = slice_ns as f64;
        k as f64 * s / (s + self.switch_overhead_ns)
    }

    /// Batch work with every CPU and the longest slice, for normalising.
    pub fn batch_work_max(&self) -> f64 {
        self.batch_work(self.cpus, self.slice_max_ns)
    }
}

#[derive(Clone, Debug)]
pub enum Policy {
    /// Batch confined to `k` CPUs with a fixed slice (no feedback).
    Fixed { k: u32, slice_ns: u64 },
    /// scx_slo's controller.
    Slo(Config),
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub id: &'static str,
    pub label: String,
    pub policy: Policy,
}

#[derive(Clone, Debug)]
pub struct Tick {
    pub t_s: f64,
    pub load: f64,
    /// p99 of this tick's requests.
    pub p99_tick_ns: Option<u64>,
    /// p99 the controller saw (short window).
    pub p99_short_ns: Option<u64>,
    pub k: u32,
    pub slice_ns: u64,
    pub mode: Option<Mode>,
    pub batch_work: f64,
}

pub struct RunOutput {
    pub ticks: Vec<Tick>,
    /// All requests after `warmup_ticks`.
    pub measured: Histogram,
}

/// Window sizes the controller runs with (in ticks).
pub const SHORT_TICKS: usize = 3;
pub const LONG_TICKS: usize = 20;
pub const MIN_SAMPLES: u64 = 100;

/// Simulate one variant over a per-tick load trace.
///
/// The levers chosen at the end of tick `t` apply during tick `t + 1`, so
/// the controller always acts on one tick of stale information, as it will
/// on a real machine.
pub fn run(
    model: &Model,
    variant: &Variant,
    loads: &[f64],
    warmup_ticks: usize,
    seed: u64,
) -> RunOutput {
    let mut rng = SplitMix64(seed);
    let mut windows = Windows::new(SHORT_TICKS, LONG_TICKS, MIN_SAMPLES);
    let mut measured = Histogram::new();
    let peak = model.peak_lambda();

    let (mut k, mut slice_ns, mut ctrl) = match &variant.policy {
        Policy::Fixed { k, slice_ns } => (*k, *slice_ns, None),
        Policy::Slo(cfg) => {
            let s = State::initial(cfg);
            (s.k, s.slice_ns, Some((cfg, s)))
        }
    };

    let mut ticks = Vec::with_capacity(loads.len());
    for (i, &load) in loads.iter().enumerate() {
        let lambda = load * peak;
        let mu_eff = model.mu0 * (1.0 - model.gamma * model.interference(k, slice_ns));
        // Overload: keep the rate positive so latencies are huge but finite.
        let rate = (mu_eff - lambda).max(50.0);

        let n = (lambda * model.tick_s).round() as u64;
        let mut hist = Histogram::new();
        for _ in 0..n {
            let t_ns = -(1.0 - rng.next_f64()).ln() / rate * 1e9;
            hist.record(t_ns as u64);
        }
        if i >= warmup_ticks {
            measured.add(&hist);
        }
        let p99_tick_ns = hist.quantile(0.99);
        let batch_work = model.batch_work(k, slice_ns);
        windows.push(hist);
        let obs = windows.observation();

        let mode = ctrl.as_mut().map(|(cfg, s)| {
            *s = step(cfg, s, obs);
            s.mode
        });
        ticks.push(Tick {
            t_s: i as f64 * model.tick_s,
            load,
            p99_tick_ns,
            p99_short_ns: obs.p99_short_ns,
            k,
            slice_ns,
            mode,
            batch_work,
        });
        if let Some((_, s)) = &ctrl {
            k = s.k;
            slice_ns = s.slice_ns;
        }
    }
    RunOutput { ticks, measured }
}

/// Small deterministic PRNG so the simulator needs no dependencies.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Config, Model) {
        let cfg = Config::with_k_max(16);
        let m = Model::default_for(&cfg);
        (cfg, m)
    }

    #[test]
    fn memcached_alone_at_peak_sits_at_the_slo() {
        let (cfg, m) = setup();
        let alone = Variant {
            id: "B0",
            label: "alone".into(),
            policy: Policy::Fixed {
                k: 0,
                slice_ns: cfg.slice_max_ns,
            },
        };
        let out = run(&m, &alone, &vec![1.0; 200], 20, 1);
        let p99 = out.measured.quantile(0.99).unwrap() as f64;
        let slo = cfg.slo_ns as f64;
        // Within histogram resolution (+12.5%) and sampling noise.
        assert!(p99 > slo * 0.9 && p99 < slo * 1.2, "p99 {p99} vs SLO {slo}");
    }

    #[test]
    fn more_batch_cores_means_more_latency_and_more_work() {
        let (cfg, m) = setup();
        let p99_at = |k| {
            let v = Variant {
                id: "x",
                label: "x".into(),
                policy: Policy::Fixed {
                    k,
                    slice_ns: cfg.slice_max_ns,
                },
            };
            run(&m, &v, &vec![0.5; 100], 10, 3)
                .measured
                .quantile(0.99)
                .unwrap()
        };
        assert!(p99_at(2) < p99_at(8));
        assert!(p99_at(8) < p99_at(16));
        assert!(m.batch_work(2, cfg.slice_max_ns) < m.batch_work(16, cfg.slice_max_ns));
    }

    #[test]
    fn same_seed_same_result() {
        let (cfg, m) = setup();
        let v = Variant {
            id: "T",
            label: "slo".into(),
            policy: Policy::Slo(cfg),
        };
        let loads: Vec<f64> = (0..300).map(|i| if i < 150 { 0.3 } else { 0.8 }).collect();
        let a = run(&m, &v, &loads, 0, 9);
        let b = run(&m, &v, &loads, 0, 9);
        assert_eq!(a.measured, b.measured);
    }
}
