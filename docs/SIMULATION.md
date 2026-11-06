# Simulation

`crates/slo-sim` runs the real controller (`crates/slo-ctrl`) in a closed
loop against a simple model of memcached sharing a machine with batch jobs.
It exists to test the controller's design before the CloudLab testbed works.
**Its numbers are not measurements**, and every figure and table says so.

## Run it

```sh
cargo run --release -p slo-sim                 # results/sim/{summary.md,sweep.csv,burst.csv}

python3 -m venv --without-pip .venv            # this Debian Python lacks ensurepip
curl -sSL https://bootstrap.pypa.io/get-pip.py | .venv/bin/python
.venv/bin/pip install -r analysis/requirements.txt
.venv/bin/python analysis/plot_sim.py          # results/sim/plots/{sweep,burst}.{png,pdf}
```

The whole run takes about 2 seconds and uses fixed seeds, so results are reproducible.

## Model

| Piece | Model | Constant |
| --- | --- | --- |
| memcached | One M/M/1 queue; response time ~ Exp(μ_eff − λ) | μ0 = 50k req/s (20 µs service) |
| Interference | I = (k / 16) · (0.5 + 0.5 · slice / 20 ms) | |
| Slowdown | μ_eff = μ0 · (1 − γ · I) | γ = 0.45 |
| Load | λ = load × peak; peak is where memcached alone has p99 = SLO | SLO = 500 µs (placeholder) |
| Batch work | k · slice / (slice + overhead), as % of k = 16 at a 20 ms slice | overhead = 0.1 ms |
| Overload | If λ ≥ μ_eff, latency ~ Exp(50/s), i.e. about 90 ms p99 | |
| Loop | 100 ms tick; levers chosen at tick t apply at t + 1; windows of 3 and 20 ticks | |

Every request is sampled and bucketed with the same histogram and windows
the real scheduler will use, so the controller sees the same kind of noisy
p99 it will see on hardware.

## Variants

| ID | Policy |
| --- | --- |
| `B1-eevdf` | No control: batch tasks on all 16 CPUs with a 20 ms slice |
| `B2-static` | Fixed `k`, the largest that meets the SLO at 90% load (it comes out as 2): a static cgroup limit sized for peak |
| `B5-fixedk` | Fixed k = 8 |
| `T-slo` | scx_slo controller with default tunables |

## What it shows (current defaults)

- With no control, p99 crosses the SLO between 40% and 50% load, and the queue overloads from 70%.
- The static limit always meets the SLO but gives batch jobs 12% throughput at every load.
- scx_slo meets the SLO at every load from 10% to 90%, with 91% mean batch throughput up to 70% load. The proposal's success criterion is met **in simulation**.
- In the 30% → 85% burst, scx_slo exceeds the SLO for 3 ticks (0.3 s) and is back under within 0.5 s. With no control, it stays over for the whole 40 s burst.

### Findings to take into W6 tuning

1. **The first tick of a burst is always lost.** The controller acts one tick late, and at 85% load all 16 batch CPUs overload memcached immediately. A shorter tick, or a probe that triggers a cut directly from BPF, would shrink this.
2. **The hysteresis band stops recovery early.** At 40% load the controller settles at about 80% batch throughput instead of 100%, because p99 sits between the low (70%) and high (90%) thresholds, so it neither cuts nor recovers. Lowering the high threshold or raising the low one trades this against flapping.
3. **It over-cuts at high load.** At 90% load, halving every 2 ticks drives batch jobs down to 1 CPU and a 0.5 ms slice (5% throughput), below the static limit's 12%. A gentler cut factor, or cutting cores before slice, should fix this.
4. **The 50% load point oscillates.** It repeatedly cuts and recovers; the min–max bars show the spread. Measure the number of `k` changes per minute on hardware.

## Limits

- One M/M/1 queue, no multi-threaded memcached, no SMT topology, and interference that grows linearly in `k`. Only the shape of the trade-off is meaningful, not the absolute values.
- It does not model the sched_ext mechanism: the overflow escape (`nr_sched_congested`), user-space dispatch latency, and the priority of the fair class over sched_ext.
- The SLO is a placeholder. Re-run with the real one once `B0-alone` is measured on CloudLab.
