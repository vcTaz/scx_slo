//! Run the simulated experiments and write CSVs and a Markdown summary.
//! Plots come from `analysis/plot_sim.py`.
//!
//! Usage: `cargo run --release -p slo-sim [-- OUT_DIR]` (default `results/sim`).
//!
//! Every number this produces comes from the model in `model.rs`, not from
//! a machine. See docs/SIMULATION.md.

mod model;

use std::fmt::Write as _;
use std::{env, fs, path::PathBuf};

use model::{run, Model, Policy, RunOutput, Variant};
use slo_ctrl::Config;

const SWEEP_LOADS: [f64; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
const SWEEP_TICKS: usize = 600; // 60 s
const WARMUP_TICKS: usize = 100; // controller starts restricted; skip its ramp-up
const REPS: u64 = 5;

const BURST_LOW: f64 = 0.3;
const BURST_HIGH: f64 = 0.85;
const BURST_START: usize = 300; // tick 300 = 30 s
const BURST_END: usize = 700; // 70 s
const BURST_TICKS: usize = 1200; // 120 s

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(env::args().nth(1).unwrap_or_else(|| "results/sim".into()));
    fs::create_dir_all(&out)?;

    let cfg = Config::with_k_max(16);
    cfg.validate().expect("valid controller config");
    let model = Model::default_for(&cfg);
    let slo_us = cfg.slo_ns as f64 / 1000.0;

    let k_static = static_k_for_peak(&model, &cfg, 0.9);
    let variants = vec![
        Variant {
            id: "B1-eevdf",
            label: "No control (all 16 CPUs)".into(),
            policy: Policy::Fixed {
                k: cfg.k_max,
                slice_ns: cfg.slice_max_ns,
            },
        },
        Variant {
            id: "B2-static",
            label: format!("Static limit k={k_static} (sized for 90% load)"),
            policy: Policy::Fixed {
                k: k_static,
                slice_ns: cfg.slice_max_ns,
            },
        },
        Variant {
            id: "B5-fixedk",
            label: "Fixed k=8".into(),
            policy: Policy::Fixed {
                k: 8,
                slice_ns: cfg.slice_max_ns,
            },
        },
        Variant {
            id: "T-slo",
            label: "scx_slo (adaptive)".into(),
            policy: Policy::Slo(cfg.clone()),
        },
    ];

    // ---- E-sweep: steady load, 10-90% of peak ----
    let mut sweep_csv = String::from("variant,load_pct,rep,p99_us,slo_ok_pct,batch_pct\n");
    // rows[variant][load] = mean (p99_us, slo_ok_pct, batch_pct)
    let mut rows: Vec<Vec<(f64, f64, f64)>> = Vec::new();
    for v in &variants {
        let mut per_load = Vec::new();
        for &load in &SWEEP_LOADS {
            let mut acc = (0.0, 0.0, 0.0);
            for rep in 0..REPS {
                let seed = 1000 * rep + (load * 100.0) as u64;
                let r = run(&model, v, &vec![load; SWEEP_TICKS], WARMUP_TICKS, seed);
                let m = metrics(&r, &model, cfg.slo_ns, WARMUP_TICKS);
                let _ = writeln!(
                    sweep_csv,
                    "{},{:.0},{rep},{:.1},{:.2},{:.2}",
                    v.id,
                    load * 100.0,
                    m.0,
                    m.1,
                    m.2
                );
                acc = (acc.0 + m.0, acc.1 + m.1, acc.2 + m.2);
            }
            let n = REPS as f64;
            per_load.push((acc.0 / n, acc.1 / n, acc.2 / n));
        }
        rows.push(per_load);
    }
    fs::write(out.join("sweep.csv"), sweep_csv)?;

    // ---- E-burst: 30% -> 85% -> 30% ----
    let loads: Vec<f64> = (0..BURST_TICKS)
        .map(|i| {
            if (BURST_START..BURST_END).contains(&i) {
                BURST_HIGH
            } else {
                BURST_LOW
            }
        })
        .collect();
    let burst_variants = [&variants[0], &variants[1], &variants[3]];
    let mut burst_csv =
        String::from("variant,t_s,load_pct,p99_tick_us,p99_short_us,k,slice_us,mode,batch_work\n");
    let mut burst_runs = Vec::new();
    for v in burst_variants {
        let r = run(&model, v, &loads, 0, 77);
        for t in &r.ticks {
            let _ = writeln!(
                burst_csv,
                "{},{:.1},{:.0},{},{},{},{},{},{:.3}",
                v.id,
                t.t_s,
                t.load * 100.0,
                us_opt(t.p99_tick_ns),
                us_opt(t.p99_short_ns),
                t.k,
                t.slice_ns / 1000,
                t.mode.map(|m| format!("{m:?}")).unwrap_or_default(),
                t.batch_work
            );
        }
        burst_runs.push(r);
    }
    fs::write(out.join("burst.csv"), burst_csv)?;

    // ---- Summary ----
    let tick_s = model.tick_s;
    let mut md = String::new();
    let _ = writeln!(md, "# scx_slo simulation results\n");
    let _ = writeln!(md, "**Simulated, not measured.** Model and caveats: docs/SIMULATION.md. SLO = {slo_us:.0} µs (placeholder). Controller: `slo-ctrl` defaults, {} ms tick, windows {}/{} ticks.\n", tick_s * 1000.0, model::SHORT_TICKS, model::LONG_TICKS);
    let _ = writeln!(
        md,
        "## Load sweep (mean of {REPS} runs, {} s each after {} s warm-up)\n",
        (SWEEP_TICKS - WARMUP_TICKS) as f64 * tick_s,
        WARMUP_TICKS as f64 * tick_s
    );
    let _ = write!(md, "| Variant | Metric |");
    for l in SWEEP_LOADS {
        let _ = write!(md, " {:.0}% |", l * 100.0);
    }
    let _ = writeln!(md);
    let _ = writeln!(md, "|---|---|{}", "---:|".repeat(SWEEP_LOADS.len()));
    for (vi, v) in variants.iter().enumerate() {
        for (name, f) in [
            (
                "p99 (µs)",
                (|r: &(f64, f64, f64)| r.0) as fn(&(f64, f64, f64)) -> f64,
            ),
            ("ticks within SLO (%)", |r| r.1),
            ("batch throughput (%)", |r| r.2),
        ] {
            let _ = write!(md, "| {} | {name} |", v.id);
            for r in &rows[vi] {
                let _ = write!(md, " {:.0} |", f(r));
            }
            let _ = writeln!(md);
        }
    }
    let _ = writeln!(
        md,
        "\nVariants: {}.\n",
        variants
            .iter()
            .map(|v| format!("`{}` = {}", v.id, v.label))
            .collect::<Vec<_>>()
            .join("; ")
    );

    // Success criterion: p99 within 10% of SLO up to 70% load, more batch work than static.
    let t = variants.iter().position(|v| v.id == "T-slo").unwrap();
    let s = variants.iter().position(|v| v.id == "B2-static").unwrap();
    let upto70: Vec<usize> = (0..SWEEP_LOADS.len())
        .filter(|&i| SWEEP_LOADS[i] <= 0.7 + 1e-9)
        .collect();
    let p99_ok = upto70.iter().all(|&i| rows[t][i].0 <= slo_us * 1.1);
    let batch_t: f64 = upto70.iter().map(|&i| rows[t][i].2).sum::<f64>() / upto70.len() as f64;
    let batch_s: f64 = upto70.iter().map(|&i| rows[s][i].2).sum::<f64>() / upto70.len() as f64;
    let _ = writeln!(md, "**Success criterion (10–70% load):** p99 within 10% of SLO: **{}**. Mean batch throughput: scx_slo {batch_t:.0}% vs static {batch_s:.0}% → **{}**.\n", yes_no(p99_ok), yes_no(batch_t > batch_s));

    let _ = writeln!(
        md,
        "## Load burst ({:.0}% → {:.0}% at {:.0} s → {:.0}% at {:.0} s)\n",
        BURST_LOW * 100.0,
        BURST_HIGH * 100.0,
        BURST_START as f64 * tick_s,
        BURST_LOW * 100.0,
        BURST_END as f64 * tick_s
    );
    let _ = writeln!(md, "| Variant | Ticks over SLO | Worst tick p99 (µs) | Back under SLO after burst start | Batch throughput (%) |");
    let _ = writeln!(md, "|---|---:|---:|---:|---:|");
    for (v, r) in burst_variants.iter().zip(&burst_runs) {
        let over = r
            .ticks
            .iter()
            .filter(|t| t.p99_tick_ns.is_some_and(|p| p > cfg.slo_ns))
            .count();
        let worst = r
            .ticks
            .iter()
            .filter_map(|t| t.p99_tick_ns)
            .max()
            .unwrap_or(0) as f64
            / 1000.0;
        let settle = settle_time(r, cfg.slo_ns, BURST_START, BURST_END)
            .map(|ticks| format!("{:.1} s", ticks as f64 * tick_s))
            .unwrap_or_else(|| "never".into());
        let batch = r.ticks.iter().map(|t| t.batch_work).sum::<f64>()
            / (r.ticks.len() as f64 * model.batch_work_max())
            * 100.0;
        let _ = writeln!(
            md,
            "| {} | {over} / {BURST_TICKS} | {worst:.0} | {settle} | {batch:.0} |",
            v.id
        );
    }
    let _ = writeln!(
        md,
        "\nRaw data: `sweep.csv`, `burst.csv`. Plots: `.venv/bin/python analysis/plot_sim.py`."
    );
    fs::write(out.join("summary.md"), &md)?;

    print!("{md}");
    eprintln!("\nwrote {}", out.display());
    Ok(())
}

/// (p99 µs over measured requests, % of measured ticks within SLO, batch % of max)
fn metrics(r: &RunOutput, model: &Model, slo_ns: u64, warmup: usize) -> (f64, f64, f64) {
    let p99 = r.measured.quantile(0.99).unwrap_or(0) as f64 / 1000.0;
    let ticks = &r.ticks[warmup..];
    let ok = ticks
        .iter()
        .filter(|t| t.p99_tick_ns.is_none_or(|p| p <= slo_ns))
        .count();
    let work: f64 = ticks.iter().map(|t| t.batch_work).sum();
    (
        p99,
        ok as f64 / ticks.len() as f64 * 100.0,
        work / (ticks.len() as f64 * model.batch_work_max()) * 100.0,
    )
}

/// Largest fixed k whose p99 meets the SLO at `load` (how an operator would
/// size a static cgroup limit for peak).
fn static_k_for_peak(model: &Model, cfg: &Config, load: f64) -> u32 {
    (1..=cfg.k_max)
        .rev()
        .find(|&k| {
            let v = Variant {
                id: "probe",
                label: String::new(),
                policy: Policy::Fixed {
                    k,
                    slice_ns: cfg.slice_max_ns,
                },
            };
            let r = run(model, &v, &vec![load; 300], 50, 4242);
            r.measured.quantile(0.99).is_some_and(|p| p <= cfg.slo_ns)
        })
        .unwrap_or(0)
}

/// Ticks from burst start until the short-window p99 is under the SLO and
/// stays there for 1 s, within the burst. `Some(0)` if it never went over.
fn settle_time(r: &RunOutput, slo_ns: u64, start: usize, end: usize) -> Option<usize> {
    let ok = |i: usize| r.ticks[i].p99_short_ns.is_none_or(|p| p <= slo_ns);
    (start..end.saturating_sub(10))
        .find(|&i| (i..i + 10).all(ok))
        .map(|i| i - start)
}

fn us_opt(ns: Option<u64>) -> String {
    ns.map(|n| format!("{:.1}", n as f64 / 1000.0))
        .unwrap_or_default()
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "met"
    } else {
        "not met"
    }
}
