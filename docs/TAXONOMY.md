# scx_slo taxonomy

Shared vocabulary for code, scripts, plots and the reports. Source: the Notion
proposal page (14 Oct, updated 16–17 Oct) and the W1–W7 task rows.

Each item has a short ID. Use the IDs in run names, file names, commit
messages and plot legends so results trace back to one definition.

## 1. Workloads

| ID | Class | What | Scheduling class | Role |
| --- | --- | --- | --- | --- |
| `LC` | Latency-critical | memcached | Fair (EEVDF), never SCHED_EXT | The service whose p99 must meet the SLO |
| `BATCH-kb` | Batch | Linux kernel build (`make -jN`) | SCHED_EXT | CPU-heavy, many short tasks |
| `BATCH-ff` | Batch | ffmpeg transcode | SCHED_EXT | CPU-heavy, few long-running threads |
| `LOADGEN` | Client | mutilate (fallback memtier_benchmark) on node 2 | n/a | Offered load, client-side latency |

## 2. System components

Layered from signal to actuation. Each maps to one directory (see section 8).

| ID | Component | Responsibility | Notes |
| --- | --- | --- | --- |
| `PROBE` | Latency probe | Separate BPF program tracing memcached request latency into a histogram map | No change to the scheduler's BPF. Open it before `init()`. Fallback: loadgen pushes client p99 over a socket |
| `CTRL` | Controller | Pure Rust function: p99 samples + SLO + state → (k batch cores, batch `slice_ns`) | No I/O, unit-tested on synthetic traces |
| `ACT` | Actuator | Applies `CTRL` output inside `dispatch_task()`: batch tasks only to CPUs in the allowed set, `slice_ns` from the controller | Never `RL_CPU_ANY` for batch tasks |
| `SCHED` | Scheduler shell | User-space scheduler built from `scx_rlfifo` on `scx_rustland_core` | `partial: true`, `builtin_idle: false`, one pinned scx tag |
| `LOG` | Telemetry | Per-tick log: timestamp, p99 short/long, controller state, k, slice, `nr_sched_congested` | Source for reaction time and oscillation analysis |
| `HARNESS` | Run harness | Scripts that start LC, batch, loadgen and the scheduler variant, then collect results | One command per run point |
| `ANALYSIS` | Analysis | Parse results, compute metrics, plot | |
| `TESTBED` | Testbed | Two bare-metal CloudLab nodes, Linux 6.12+ with `CONFIG_SCHED_CLASS_EXT=y` | This dev machine has BPF and BTF but not sched_ext, so `SCHED` runs only on CloudLab |

## 3. Controller model

**Inputs**

| Signal | Meaning |
| --- | --- |
| `p99_short` | p99 over a short window (burst detection) |
| `p99_long` | p99 over a long window (trend) |
| `SLO` | Target p99 for memcached |
| `slack` | `(SLO - p99) / SLO` |

**Outputs**

| Lever | Range | Applied by |
| --- | --- | --- |
| `k` | batch cores allowed, `k_min..k_max` | `ACT` CPU choice |
| `slice` | batch time slice, `slice_min..slice_max` | `ACT` `slice_ns` |

**States** (fast restriction, slow recovery, with hysteresis)

| State | Enter when | Action |
| --- | --- | --- |
| `RESTRICT` | `p99_short` above the high threshold | Cut `k` and `slice` by a large step at once |
| `HOLD` | p99 between the thresholds, or still in hold-down after a cut | No change (the hysteresis band) |
| `RECOVER` | `p99_long` below the low threshold for N ticks | Give back `k`, then `slice`, one small step per tick |

**Tunables:** window lengths, high and low thresholds, cut size, recovery step,
hold-down ticks, tick period. W6 tunes these.

## 4. Scheduler variants (baselines and treatments)

| ID | Variant | Question it answers |
| --- | --- | --- |
| `B0-alone` | memcached alone, no batch | Best-case latency and peak QPS (sets the load axis) |
| `B1-eevdf` | memcached + batch, all on EEVDF | Cost of naive co-location |
| `B2-cgroup` | Static cgroup CPU limit on batch | Industry default: protects latency, wastes capacity |
| `B3-rustland` | Stock `scx_rustland`, all tasks | Off-the-shelf sched_ext |
| `B4-partial` | Our scheduler, partial mode, no controller (all CPUs, default slice) | How much does fair-over-ext class priority alone buy? |
| `B5-fixedk` | Our scheduler with a fixed `k` and slice | Value of a static core split without feedback |
| `T-slo` | scx_slo, adaptive controller | The contribution |

`B4` and `B5` matter most for the argument: if `B4` already meets the SLO,
scx_slo's value has to come from SMT and cache interference and from batch
throughput, not raw CPU priority (the Heracles argument).

## 5. Experiments

| ID | Experiment | Load pattern | Variants | Repetitions |
| --- | --- | --- | --- | --- |
| `E-sweep` | Steady-state load sweep | 10–90% of `B0` peak QPS | All | 5 per point (3 if time is short) |
| `E-burst` | Burst response | Step from low to high load and back | `B2`, `B5`, `T-slo` | 5 |
| `E-react` | Reaction time | Same as `E-burst` | `T-slo` | from `LOG` |
| `E-ovh` | Overhead | LC alone under `SCHED` vs `B0` | `B4`, `T-slo` | 3 |
| `E-unit` | Controller unit tests | Synthetic p99 traces (step, ramp, spike, noise) | `CTRL` only | CI |

## 6. Metrics

| Group | Metric | Source |
| --- | --- | --- |
| LC latency | p50, p99, p99.9 | Loadgen |
| SLO | Fraction of time with p99 ≤ SLO; number of violations | Loadgen time series |
| Batch | Jobs per hour (kernel builds/h, ffmpeg frames/s) | Batch wrapper |
| Efficiency | CPU utilisation | `mpstat` / `/proc/stat` |
| Controller | Reaction time (p99 crossing → `k` change), `k` changes per minute (oscillation) | `LOG` |
| Mechanism health | `nr_sched_congested` (overflow escapes), user-space dispatch latency | `SCHED` stats |

**Success criterion:** p99 within 10% of the SLO up to at least 70% load,
with more batch throughput than `B2-cgroup`.

## 7. Risks and known pitfalls

| ID | Risk | Mitigation |
| --- | --- | --- |
| `R-class` | The assumption that fair ranks above ext is wrong on our kernel | Check in W1 with `B4`; reframe if it fails |
| `R-idle` | `builtin_idle: true` dispatches to idle CPUs and escapes `k` | Always `false`; assert at startup |
| `R-overflow` | A full user-space queue sends tasks to the shared DSQ (any CPU) | Log `nr_sched_congested`; report it with every run |
| `R-seccomp` | `init()` locks memory and blocks new mmap | Spawn threads and open the probe map before `init()` |
| `R-bpfgen` | `RustLandBuilder` rewrites `main.bpf.c` on build | Don't edit it; vendor the crate if BPF changes are needed |
| `R-version` | `scx_rustland_core` 2.4.15 changed `init()` (`numa_local`), pins libbpf-rs 0.27.2 | Pin one scx tag and one crate version in `Cargo.lock` |
| `R-testbed` | CloudLab reservation or kernel image delays (W1 is already 19 days late) | Build everything testbed-free first; snapshot the disk image once it works |
| `R-probe` | Hard to trace memcached request latency in BPF | Fallback: loadgen reports client p99 over a socket |

## 8. Repository layout

```
scx_slo/
├── Cargo.toml            # workspace; pins scx_rustland_core and libbpf-rs
├── crates/
│   ├── slo-ctrl/         # CTRL: pure controller + synthetic-trace tests (no BPF)
│   └── slo-probe/        # PROBE: BPF latency histogram + libbpf-rs loader
├── scx_slo/              # SCHED + ACT + LOG: binary from scx_rlfifo
├── bench/                # HARNESS: run scripts, one per variant; configs/
├── analysis/             # ANALYSIS: parsers, metrics, plots
├── infra/cloudlab/       # TESTBED: profile, kernel config, setup script
├── results/              # raw runs: <experiment>/<variant>/<load>/<rep>/ (git-ignored)
└── docs/                 # this file, PLAN.md, reports
```

Run directory names use the IDs, e.g. `results/E-sweep/B5-fixedk/load70/rep3/`.
