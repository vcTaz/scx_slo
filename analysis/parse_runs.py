"""Summarise CloudLab runs from bench/ (standard library only, runs anywhere).

    python3 analysis/parse_runs.py results/w1          # table + results/w1/summary.csv
    python3 analysis/parse_runs.py --one RUN_DIR       # one run, one line

A run folder (results/EXP/VARIANT/qpsN/repR/) holds meta.json, memtier.json,
and, depending on the variant, batch.json, sched.csv and mpstat.txt.
"""

import argparse
import csv
import json
import sys
from collections import defaultdict
from pathlib import Path
from statistics import mean

FIELDS = [
    "variant", "qps", "rep", "ops_s", "p50_us", "p90_us", "p99_us", "p999_us",
    "kbuild_objects", "ffmpeg_frames", "cpu_busy_pct", "sched_congested",
]


def totals(memtier):
    """memtier's 'ALL STATS' -> 'Totals' object, wherever it is nested."""
    stack = [memtier]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            if "Totals" in node and isinstance(node["Totals"], dict):
                return node["Totals"]
            stack.extend(node.values())
    raise ValueError("no Totals in memtier.json")


def percentiles_us(tot):
    """{50.0: µs, 99.0: µs, ...}; memtier reports milliseconds."""
    out = {}
    for key, val in tot.get("Percentile Latencies", {}).items():
        if key.startswith("p"):
            try:
                out[float(key[1:])] = float(val) * 1000.0
            except ValueError:
                pass
    return out


def cpu_busy(path):
    if not path.exists():
        return None
    for line in path.read_text().splitlines():
        cols = line.split()
        if len(cols) > 2 and cols[0].startswith("Average") and cols[1] == "all":
            return round(100.0 - float(cols[-1]), 1)
    return None


def congested(path):
    if not path.exists():
        return None
    rows = [ln.split(",") for ln in path.read_text().splitlines() if ln and ln[0].isdigit()]
    return int(rows[-1][6]) if rows and len(rows[-1]) > 6 else None


def parse_run(run):
    meta = json.loads((run / "meta.json").read_text())
    tot = totals(json.loads((run / "memtier.json").read_text()))
    pct = percentiles_us(tot)
    batch = json.loads((run / "batch.json").read_text()) if (run / "batch.json").exists() else {}
    return {
        "variant": meta["variant"],
        "qps": meta["qps"],
        "rep": meta["rep"],
        "ops_s": round(float(tot.get("Ops/sec", 0.0))),
        "p50_us": round(pct.get(50.0, float("nan")), 1),
        "p90_us": round(pct.get(90.0, float("nan")), 1),
        "p99_us": round(pct.get(99.0, float("nan")), 1),
        "p999_us": round(pct.get(99.9, float("nan")), 1),
        "kbuild_objects": batch.get("kbuild_objects"),
        "ffmpeg_frames": batch.get("ffmpeg_frames"),
        "cpu_busy_pct": cpu_busy(run / "mpstat.txt"),
        "sched_congested": congested(run / "sched.csv"),
    }


def fmt(v):
    if v is None:
        return "-"
    if isinstance(v, float):
        return f"{v:.0f}" if abs(v) >= 10 else f"{v:.1f}"
    return str(v)


def print_table(rows):
    groups = defaultdict(list)
    for r in rows:
        groups[(r["qps"], r["variant"])].append(r)
    cols = ["qps", "variant", "reps", "ops_s", "p50_us", "p99_us", "p999_us",
            "kbuild_objects", "ffmpeg_frames", "cpu_busy_pct", "sched_congested"]
    table = []
    for (qps, variant), rs in sorted(groups.items()):
        row = {"qps": qps, "variant": variant, "reps": len(rs)}
        for c in cols[3:]:
            vals = [r[c] for r in rs if r[c] is not None]
            row[c] = mean(vals) if vals else None
        table.append([fmt(row[c]) for c in cols])
    widths = [max(len(c), *(len(t[i]) for t in table)) for i, c in enumerate(cols)]
    print("  ".join(c.rjust(w) for c, w in zip(cols, widths)))
    for t in table:
        print("  ".join(v.rjust(w) for v, w in zip(t, widths)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("path", type=Path, nargs="?")
    ap.add_argument("--one", type=Path, help="print a single run folder")
    args = ap.parse_args()

    if args.one:
        r = parse_run(args.one)
        print("  ".join(f"{k}={fmt(r[k])}" for k in FIELDS))
        return
    if not args.path:
        ap.error("give an experiment folder or --one RUN_DIR")

    rows, bad = [], []
    for mt in sorted(args.path.rglob("memtier.json")):
        try:
            rows.append(parse_run(mt.parent))
        except (OSError, ValueError, KeyError) as e:
            bad.append(f"{mt.parent}: {e}")
    if not rows:
        sys.exit(f"no runs under {args.path}")
    with open(args.path / "summary.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=FIELDS)
        w.writeheader()
        w.writerows(rows)
    print_table(rows)
    print(f"\n{len(rows)} runs -> {args.path / 'summary.csv'}")
    for b in bad:
        print(f"skipped {b}", file=sys.stderr)


if __name__ == "__main__":
    main()
