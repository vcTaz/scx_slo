# scx_slo plan (re-baselined 6 Nov 2026)

IDs refer to [TAXONOMY.md](TAXONOMY.md).

## Where things stand

Per the Notion Tasks database on 6 Nov:

| Task | Due | Status |
| --- | --- | --- |
| W1 CloudLab testbed, scx_rlfifo running | 18 Oct | In progress, 19 days late |
| W2 Baselines | 25 Oct | Not started |
| W3 p99 latency probe | 1 Nov | Not started |
| W4 Controller windows, thresholds, hysteresis | 8 Nov | Not started |
| Midterm status report (2%) | **Sun 8 Nov** | Not started |
| W5 Fast restriction, gradual recovery | 15 Nov | Not started |
| W6 Burst experiments, tuning, reaction time | 22 Nov | Not started |
| W7 Full sweeps, analysis, plots | 29 Nov | Not started |
| Final presentation (15%) | 1–4 Dec | Not started |

The repo has only a README. About 4 weeks remain. The project is about 3 weeks
behind, and everything downstream depends on the testbed (W1).

**Key constraint:** this dev machine has BPF and BTF but no
`CONFIG_SCHED_CLASS_EXT`, so `SCHED` runs only on CloudLab. `CTRL` and most
of `PROBE` can be built and tested locally.

## Strategy

1. **Two parallel tracks** so testbed problems stop blocking code:
   - **Track A, testbed and measurement:** `TESTBED`, `HARNESS`, baselines, sweeps.
   - **Track B, code:** `CTRL`, `PROBE`, `SCHED`/`ACT`, `LOG`.

   Suggested split: one person per track, swapping for reviews. Both present.
2. **Testbed-free first.** `CTRL` is a pure function, so finish it locally
   with tests before spending CloudLab time.
3. **Snapshot the CloudLab disk image** as soon as the kernel and scx build
   work. Never redo W1.
4. **Cut scope before cutting quality** (see the cut list below).

## Schedule

### Now to Sun 8 Nov: unblock and report

| Track | Work | Done when |
| --- | --- | --- |
| A | Finish W1: 6.12+ kernel with sched_ext on CloudLab, scx tag pinned, `scx_rlfifo` runs with `partial: true`, `builtin_idle: false`; snapshot the image | `cat /sys/kernel/sched_ext/state` reads `enabled` with rlfifo loaded |
| A | Check `R-class`: does memcached (fair) beat a SCHED_EXT spinner on a shared CPU? | Yes/no with one measurement |
| A | Measure `B0-alone` peak QPS and `B1-eevdf` at 2–3 load points (short runs) | Numbers in a table |
| B | Scaffold the Cargo workspace (`crates/slo-ctrl`, pinned deps); write `CTRL` types and `RESTRICT`/`HOLD`/`RECOVER` logic | `cargo test` passes on step and spike traces |
| Both | Write the midterm report (1 page) | Submitted Sun 8 Nov |

**Midterm report outline.** Progress (1%): testbed state, `B0`/`B1` numbers,
the `R-class` result, the controller design and its tests. Challenges and plan (1%):
testbed delay, scx_rustland_core pitfalls (`R-idle`, `R-overflow`,
`R-seccomp`), this schedule, the cut list. Be explicit that the probe is not
working yet and when it will be.

### Week of 9–15 Nov: signal path (W2 + W3 + W4)

| Track | Work | Done when |
| --- | --- | --- |
| A | `HARNESS`: one script per variant, results in `results/<exp>/<variant>/<load>/<rep>/` | `bench/run.sh E-sweep B1-eevdf 50 1` works end to end |
| A | Baselines `B2-cgroup`, `B3-rustland`, `B4-partial` at 3 load points | Table with p50/p99/p99.9 |
| B | `PROBE`: BPF latency histogram for memcached, loaded and read from Rust; develop locally, verify on CloudLab | p99 from the probe within ~10% of loadgen p99 |
| B | If the probe is not working by Thu 12 Nov, switch to the fallback (loadgen pushes p99 over a socket) | A p99 signal reaches user space |
| B | `CTRL` finished: both windows, hysteresis, tunables in config, tests for ramp, noise and oscillation | Tests show no flapping on noisy traces |

### Week of 16–22 Nov: close the loop (W5)

| Track | Work | Done when |
| --- | --- | --- |
| B | `SCHED` from `scx_rlfifo`; `ACT` maps `k` → allowed CPU set, `slice` → `slice_ns`; probe opened before `init()` | `B5-fixedk` runs with batch confined to k CPUs |
| B | `LOG` per tick, including `nr_sched_congested` | CSV per run |
| A | `B5-fixedk` at 2–3 values of k; first `T-slo` end-to-end run | `T-slo` changes k in response to load |
| A | `analysis/`: parsers and first plots (p99 vs load, batch throughput vs load) | Plots generated from `results/` |

### Week of 23–29 Nov: tune and sweep (W6 + W7, merged)

| Work | Done when |
| --- | --- |
| `E-burst` + `E-react`: tune thresholds, steps, hold-down (2 days max) | Reaction time and oscillation numbers |
| `E-sweep` for all variants, 10/30/50/70/90% load, 3–5 reps (run overnight) | Full results directory |
| Plots: p99 vs load, SLO-time fraction, batch throughput vs `B2`; check the 70%-load success criterion | Final figures |
| `E-ovh` if time allows | |

### 30 Nov–4 Dec: presentation

Confirm the slot. 10–15 minutes: motivation (tail at scale), related work
(Heracles, PARTIES), design (taxonomy section 2 as one diagram), results,
limits. Rehearse twice. Both present.

## Cut list (cut in this order if late)

1. `E-ovh` overhead tests.
2. `BATCH-ff`: keep only the kernel build.
3. Sweep points 10/30/50/70/90 → 30/50/70; repetitions 5 → 3.
4. Slice as a lever: control `k` only.
5. In-kernel `PROBE`: use the loadgen-socket fallback.

Never cut: `B2-cgroup`, `B4-partial`, `B5-fixedk`, the 70%-load check, `LOG`
with `nr_sched_congested`.

## Decision points

| Date | Question | If no |
| --- | --- | --- |
| Sun 8 Nov | Is sched_ext running on CloudLab? | Escalate to the tutor; consider any 6.12+ machine with sched_ext |
| Sun 8 Nov | Does fair beat ext (`R-class`)? | Memcached must also be protected inside scx_slo; rethink `ACT` |
| Thu 12 Nov | Is the BPF probe working? | Switch to the socket fallback |
| Fri 20 Nov | Does `T-slo` close the loop end to end? | Present `B5-fixedk` vs baselines plus offline controller results |
