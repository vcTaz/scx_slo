"""Plot the simulator's output (results/sim/*.csv) as PNG and PDF.

Usage:
    cargo run --release -p slo-sim            # writes results/sim/
    .venv/bin/python analysis/plot_sim.py     # writes results/sim/plots/

All figures are simulated, not measured, and say so in their titles.
"""

import argparse
import csv
import os
from collections import defaultdict
from pathlib import Path
from statistics import mean

os.environ.setdefault("MPLCONFIGDIR", str(Path(".mplconfig").resolve()))
import matplotlib  # noqa: E402

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

# Okabe-Ito, colour-blind safe; one fixed colour per variant across figures.
STYLE = {
    "B1-eevdf": ("No control (batch on all CPUs)", "#D55E00", "o"),
    "B2-static": ("Static limit sized for 90% load", "#0072B2", "s"),
    "B5-fixedk": ("Fixed k = 8", "#999999", "^"),
    "T-slo": ("scx_slo (adaptive)", "#009E73", "D"),
}


def read_csv(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f))


def plot_sweep(rows, slo_us, out):
    by = defaultdict(lambda: defaultdict(list))
    for r in rows:
        by[r["variant"]][float(r["load_pct"])].append(r)

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(11, 4.2), constrained_layout=True)
    for vid, loads in by.items():
        label, color, marker = STYLE[vid]
        xs = sorted(loads)
        for ax, key in ((ax1, "p99_us"), (ax2, "batch_pct")):
            vals = [[float(r[key]) for r in loads[x]] for x in xs]
            ys = [mean(v) for v in vals]
            err = [[y - min(v) for y, v in zip(ys, vals)], [max(v) - y for y, v in zip(ys, vals)]]
            ax.errorbar(xs, ys, yerr=err, label=label, color=color, marker=marker,
                        markersize=5, linewidth=1.8, capsize=2)

    ax1.set_yscale("log")
    ax1.axhline(slo_us, color="black", linestyle="--", linewidth=1)
    ax1.text(11, slo_us * 1.12, f"SLO {slo_us:.0f} µs", fontsize=9)
    ax1.set_title("memcached p99 latency")
    ax1.set_ylabel("p99 latency (µs, log scale)")
    ax2.set_title("Batch throughput")
    ax2.set_ylabel("% of unrestricted batch throughput")
    ax2.set_ylim(0, 105)
    for ax in (ax1, ax2):
        ax.set_xlabel("Offered load (% of memcached-alone peak)")
        ax.set_xticks(range(10, 100, 10))
        ax.grid(True, alpha=0.3)
    ax2.legend(loc="lower left", fontsize=9)
    fig.suptitle("Load sweep, simulated (mean of 5 runs, bars = min–max)", fontsize=12)
    save(fig, out / "sweep")


def plot_burst(rows, slo_us, out):
    by = defaultdict(list)
    for r in rows:
        by[r["variant"]].append(r)

    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(10, 6), sharex=True, constrained_layout=True)
    any_rows = next(iter(by.values()))
    base = float(any_rows[0]["load_pct"])
    burst = [float(r["t_s"]) for r in any_rows if float(r["load_pct"]) != base]
    for vid, rs in by.items():
        label, color, _ = STYLE[vid]
        pts = [(float(r["t_s"]), float(r["p99_short_us"])) for r in rs if r["p99_short_us"]]
        ax1.plot(*zip(*pts), label=label, color=color, linewidth=1.5)
        ax2.step([float(r["t_s"]) for r in rs], [int(r["k"]) for r in rs], where="post",
                 label=label, color=color, linewidth=1.8)
    for ax in (ax1, ax2):
        if burst:
            ax.axvspan(min(burst), max(burst), color="black", alpha=0.06)
        ax.grid(True, alpha=0.3)
    ax1.set_yscale("log")
    ax1.axhline(slo_us, color="black", linestyle="--", linewidth=1)
    ax1.set_ylabel("p99, short window (µs, log)")
    ax1.set_title(f"Load burst, simulated: {base:.0f}% → {burst_load(any_rows, base):.0f}% (shaded) → {base:.0f}%")
    ax1.legend(loc="upper right", fontsize=9)
    ax2.set_ylabel("CPUs given to batch (k)")
    ax2.set_xlabel("Time (s)")
    ax2.set_ylim(0, 17)
    save(fig, out / "burst")


def burst_load(rows, base):
    return next(float(r["load_pct"]) for r in rows if float(r["load_pct"]) != base)


def save(fig, stem):
    for ext in ("png", "pdf"):
        fig.savefig(f"{stem}.{ext}", dpi=160)
    plt.close(fig)
    print(f"wrote {stem}.png, {stem}.pdf")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dir", type=Path, default=Path("results/sim"))
    ap.add_argument("--slo-us", type=float, default=500.0)
    args = ap.parse_args()
    out = args.dir / "plots"
    out.mkdir(parents=True, exist_ok=True)
    plot_sweep(read_csv(args.dir / "sweep.csv"), args.slo_us, out)
    plot_burst(read_csv(args.dir / "burst.csv"), args.slo_us, out)


if __name__ == "__main__":
    main()
