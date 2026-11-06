//! Latency histograms and the short/long monitoring windows.
//!
//! Buckets are log-linear: values below 8 ns get one bucket each, and every
//! power of two above that is split into 8 equal sub-buckets. Percentiles
//! report a bucket's upper bound, so they overestimate by at most 12.5%,
//! which errs on the side of protecting the SLO.
//!
//! The BPF probe must bucket with the same scheme as [`bucket_index`].

use std::collections::VecDeque;

const SUB_BITS: u32 = 3;
const SUB: u64 = 1 << SUB_BITS; // 8 sub-buckets per power of two

/// Number of buckets needed to cover every `u64` value.
pub const NR_BUCKETS: usize = (SUB + (64 - SUB_BITS as u64) * SUB) as usize; // 496

/// Bucket for a latency value in nanoseconds.
pub fn bucket_index(v: u64) -> usize {
    if v < SUB {
        return v as usize;
    }
    let exp = 63 - v.leading_zeros(); // position of the leading 1, >= 3
    let sub = (v >> (exp - SUB_BITS)) & (SUB - 1);
    (SUB + (exp - SUB_BITS) as u64 * SUB + sub) as usize
}

/// Largest value that falls into bucket `idx`.
pub fn bucket_upper_bound(idx: usize) -> u64 {
    let idx = idx as u64;
    if idx < SUB {
        return idx;
    }
    let exp = (idx - SUB) / SUB + SUB_BITS as u64;
    let sub = (idx - SUB) % SUB;
    let width = 1u64 << (exp - SUB_BITS as u64);
    let lower = (SUB + sub) << (exp - SUB_BITS as u64);
    lower + (width - 1)
}

/// Request-latency counts for one tick (or a sum of ticks).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    counts: Vec<u64>,
    total: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    pub fn new() -> Self {
        Self {
            counts: vec![0; NR_BUCKETS],
            total: 0,
        }
    }

    /// Build from raw bucket counts, e.g. as read from the probe's BPF map.
    /// Panics if `counts` does not have [`NR_BUCKETS`] entries.
    pub fn from_counts(counts: Vec<u64>) -> Self {
        assert_eq!(
            counts.len(),
            NR_BUCKETS,
            "histogram must have NR_BUCKETS buckets"
        );
        let total = counts.iter().sum();
        Self { counts, total }
    }

    pub fn record(&mut self, latency_ns: u64) {
        self.counts[bucket_index(latency_ns)] += 1;
        self.total += 1;
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// Add another histogram's counts into this one.
    pub fn add(&mut self, other: &Histogram) {
        for (a, b) in self.counts.iter_mut().zip(&other.counts) {
            *a += b;
        }
        self.total += other.total;
    }

    fn sub(&mut self, other: &Histogram) {
        for (a, b) in self.counts.iter_mut().zip(&other.counts) {
            *a -= b;
        }
        self.total -= other.total;
    }

    /// Value at quantile `q` (0 < q <= 1), as a bucket upper bound.
    /// `None` if the histogram is empty.
    pub fn quantile(&self, q: f64) -> Option<u64> {
        if self.total == 0 {
            return None;
        }
        let rank = ((q * self.total as f64).ceil() as u64).clamp(1, self.total);
        let mut seen = 0;
        for (idx, &c) in self.counts.iter().enumerate() {
            seen += c;
            if seen >= rank {
                return Some(bucket_upper_bound(idx));
            }
        }
        unreachable!("rank <= total")
    }
}

/// Short and long sliding windows over per-tick histograms.
///
/// Push one histogram per controller tick. Each window keeps a running sum,
/// so a push costs two histogram additions and up to two subtractions.
pub struct Windows {
    short_ticks: usize,
    long_ticks: usize,
    min_samples: u64,
    ticks: VecDeque<Histogram>,
    short_sum: Histogram,
    long_sum: Histogram,
}

impl Windows {
    /// `short_ticks <= long_ticks`, both at least 1. A window with fewer
    /// than `min_samples` requests reports no p99.
    pub fn new(short_ticks: usize, long_ticks: usize, min_samples: u64) -> Self {
        assert!(
            short_ticks >= 1 && short_ticks <= long_ticks,
            "need 1 <= short <= long"
        );
        Self {
            short_ticks,
            long_ticks,
            min_samples,
            ticks: VecDeque::with_capacity(long_ticks + 1),
            short_sum: Histogram::new(),
            long_sum: Histogram::new(),
        }
    }

    pub fn push(&mut self, tick: Histogram) {
        self.short_sum.add(&tick);
        self.long_sum.add(&tick);
        self.ticks.push_back(tick);

        // The tick that just left the short window is still in the long one.
        if self.ticks.len() > self.short_ticks {
            let leaving = &self.ticks[self.ticks.len() - 1 - self.short_ticks];
            self.short_sum.sub(leaving);
        }
        if self.ticks.len() > self.long_ticks {
            let oldest = self.ticks.pop_front().expect("non-empty");
            self.long_sum.sub(&oldest);
        }
    }

    pub fn p99_short(&self) -> Option<u64> {
        Self::p99(&self.short_sum, self.min_samples)
    }

    pub fn p99_long(&self) -> Option<u64> {
        Self::p99(&self.long_sum, self.min_samples)
    }

    pub fn observation(&self) -> crate::Observation {
        crate::Observation {
            p99_short_ns: self.p99_short(),
            p99_long_ns: self.p99_long(),
        }
    }

    fn p99(h: &Histogram, min_samples: u64) -> Option<u64> {
        if h.total() < min_samples.max(1) {
            return None;
        }
        h.quantile(0.99)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values_are_exact() {
        for v in 0..8 {
            assert_eq!(bucket_upper_bound(bucket_index(v)), v);
        }
    }

    #[test]
    fn bucket_bounds_contain_value_within_12_5_percent() {
        let mut v = 8u64;
        while v < u64::MAX / 3 {
            for x in [v, v + 1, v * 3 / 2, v * 2 - 1] {
                let idx = bucket_index(x);
                assert!(idx < NR_BUCKETS);
                let ub = bucket_upper_bound(idx);
                assert!(ub >= x, "upper bound {ub} below value {x}");
                assert!(
                    (ub - x) as f64 <= x as f64 * 0.125,
                    "bucket too wide at {x}: {ub}"
                );
            }
            v *= 2;
        }
        assert_eq!(bucket_index(u64::MAX), NR_BUCKETS - 1);
        assert_eq!(bucket_upper_bound(NR_BUCKETS - 1), u64::MAX);
    }

    #[test]
    fn buckets_are_contiguous() {
        for idx in 0..NR_BUCKETS - 1 {
            let ub = bucket_upper_bound(idx);
            assert_eq!(bucket_index(ub), idx);
            assert_eq!(bucket_index(ub + 1), idx + 1);
        }
    }

    #[test]
    fn p99_of_uniform_samples() {
        let mut h = Histogram::new();
        for us in 1..=1000u64 {
            h.record(us * 1000);
        }
        let p99 = h.quantile(0.99).unwrap();
        assert!((990_000..=990_000 * 9 / 8).contains(&p99), "p99 = {p99}");
    }

    #[test]
    fn empty_histogram_has_no_quantile() {
        assert_eq!(Histogram::new().quantile(0.99), None);
    }

    fn tick_with(latency_ns: u64, n: u64) -> Histogram {
        let mut h = Histogram::new();
        for _ in 0..n {
            h.record(latency_ns);
        }
        h
    }

    #[test]
    fn short_window_forgets_before_long_window() {
        let mut w = Windows::new(2, 5, 1);
        w.push(tick_with(1_000_000, 100)); // a slow tick
        for _ in 0..2 {
            w.push(tick_with(100_000, 100));
        }
        // Short window (last 2 ticks) only sees fast requests.
        assert!(w.p99_short().unwrap() < 200_000);
        // Long window (last 5) still includes the slow tick.
        assert!(w.p99_long().unwrap() >= 1_000_000);
        for _ in 0..3 {
            w.push(tick_with(100_000, 100));
        }
        assert!(w.p99_long().unwrap() < 200_000);
    }

    #[test]
    fn sparse_windows_report_none() {
        let mut w = Windows::new(1, 3, 50);
        w.push(tick_with(100_000, 10));
        assert_eq!(
            w.observation(),
            crate::Observation {
                p99_short_ns: None,
                p99_long_ns: None
            }
        );
        w.push(tick_with(100_000, 45));
        assert_eq!(w.p99_short(), None); // 45 < 50
        assert!(w.p99_long().is_some()); // 55 >= 50
    }

    #[test]
    fn running_sums_match_recomputation() {
        let (short, long) = (3, 7);
        let mut w = Windows::new(short, long, 1);
        let mut all = Vec::new();
        for i in 0..20u64 {
            let h = tick_with((i % 5 + 1) * 50_000, i + 1);
            all.push(h.clone());
            w.push(h);
            let sum_last = |n: usize| {
                let mut s = Histogram::new();
                for h in all.iter().rev().take(n) {
                    s.add(h);
                }
                s
            };
            assert_eq!(w.short_sum, sum_last(short));
            assert_eq!(w.long_sum, sum_last(long));
        }
    }
}
