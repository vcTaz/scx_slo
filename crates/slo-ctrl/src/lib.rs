//! scx_slo controller (`CTRL` in docs/TAXONOMY.md).
//!
//! Two pieces, both free of I/O so they can be tested without a testbed:
//!
//! - [`window`]: per-tick latency histograms combined into a short and a long
//!   window, each reporting a p99.
//! - [`controller`]: a pure step function from (config, state, p99s) to the
//!   next state, which carries the batch core count `k` and batch slice.

pub mod controller;
pub mod window;

pub use controller::{step, Config, ConfigError, Mode, Observation, State};
pub use window::{bucket_index, bucket_upper_bound, Histogram, Windows, NR_BUCKETS};
