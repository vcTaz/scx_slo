# Running the experiments on CloudLab

From an empty CloudLab account to the W1 check (TAXONOMY `R-class`) and the
baseline sweep. Expect about 1.5 hours to the first results, mostly builds and
one reboot.

> **Not yet run on CloudLab.** The scripts were written against the
> sched-ext/scx v1.1.4 and memtier_benchmark 2.5.2 sources. They pass
> shellcheck, and the parser was tested on sample output, but the first real
> run will probably need small fixes. Note what breaks and fix it in the repo.

## What runs where

| Node | LAN IP | Runs |
| --- | --- | --- |
| `server` | 10.10.1.1 | memcached, batch jobs (kernel build + ffmpeg), sched_ext schedulers, all `bench/` scripts |
| `client` | 10.10.1.2 | memtier_benchmark, driven from the server over ssh |

| Variant | Batch jobs run as | Scheduler loaded |
| --- | --- | --- |
| `B0-alone` | (none) | none |
| `B1-eevdf` | normal tasks, all CPUs | none |
| `B2-cgroup` | normal tasks in a systemd scope with `AllowedCPUs` = top `BATCH_K` CPUs | none |
| `B3-rustland` | normal tasks | stock `scx_rustland` (schedules everything, memcached included) |
| `B4-partial` | SCHED_EXT via `tools/ext_exec` | `scx_slo` partial mode, all CPUs |
| `B5-fixedk` | SCHED_EXT via `tools/ext_exec` | `scx_slo --cpus` = top `BATCH_K` CPUs |

`BATCH_K` defaults to half the CPUs (`bench/config.sh`).

## 1. Start an experiment

1. CloudLab → **Experiments → Create Experiment Profile → Git Repo**, and enter
   `https://github.com/vcTaz/scx_slo`. CloudLab reads `profile.py` from the repo
   root; the repo must be public, or push the branch first. Alternatively, paste
   `profile.py` into a new profile's editor.
2. **Instantiate** it. Defaults are 2 × `xl170` (Utah), Ubuntu 24.04, 100 GB
   at `/mydata`. If no xl170s are free, try `c220g5` (Wisconsin), which has more CPUs.
3. Extend the experiment to cover your session (the default is a few hours).

## 2. Get the code onto both nodes

Log in **with agent forwarding**, so the server can ssh to the client:

```sh
ssh -A <you>@<server-hostname>    # hostnames are on the experiment's List View
```

On **both** nodes:

```sh
git clone -b controller https://github.com/vcTaz/scx_slo ~/scx_slo
# private repo or unpushed changes: rsync -a ./ <you>@<node>:~/scx_slo/ from your laptop
```

## 3. Set up the client (about 5 min)

```sh
ssh <you>@<client-hostname>
~/scx_slo/infra/cloudlab/setup-client.sh
```

## 4. Set up the server (about 30–45 min, one reboot)

```sh
~/scx_slo/infra/cloudlab/setup-server.sh   # installs a sched_ext kernel, then says to reboot
sudo reboot
# log back in with ssh -A
~/scx_slo/infra/cloudlab/setup-server.sh   # builds scx_rustland, scx_slo, ext_exec; fetches batch inputs
~/scx_slo/infra/cloudlab/check.sh          # every line must say ok
```

If the first run stops with "lacks CONFIG_SCHED_CLASS_EXT", run
`infra/cloudlab/build-kernel.sh` (20–40 min), reboot, and continue.

**Once `check.sh` passes, snapshot the server:** in the experiment page, choose
the server node → **Create Disk Image**. Later experiments can then use that
image and skip step 4 (set the profile's `image` parameter to it).

Commit `scheduler/Cargo.lock` if the build changed it.

## 5. Find the peak and pick the SLO (about 4 min)

```sh
cd ~/scx_slo && bench/peak.sh
```

This measures memcached's maximum throughput alone, then p99 at 20–90% of it.
It saves the peak to `results/peak_qps.txt`. Pick the SLO from the table,
for example the p99 at the knee of the curve, and put it in
`crates/slo-ctrl/src/controller.rs` (`PLACEHOLDER_SLO_NS`) and
`analysis/plot_sim.py --slo-us`.

## 6. The W1 check (about 6 min): for the midterm report

```sh
bench/w1_check.sh          # B0, B1, B4, B5 at 50% of peak
```

How to read it (TAXONOMY `R-class`):

- **B4 p99 ≈ B0 p99:** EEVDF beats sched_ext on contended CPUs, so partial
  mode alone protects memcached. scx_slo's value then has to come from batch
  throughput and SMT/cache interference.
- **B4 p99 ≈ B1 p99, or worse:** the controller has real work to do.
- **B5 vs B4:** what a static core split buys without feedback.
- **`sched_congested` above 0 for B5:** tasks escaped `--cpus` through the
  shared DSQ (`R-overflow`). Report it.

## 7. Sweeps

```sh
# Midterm-sized (about 40 min): 4 variants × 3 loads × 2 reps
VARIANTS="B1-eevdf B2-cgroup B4-partial B5-fixedk" LOADS="30 50 70" REPS=2 bench/sweep.sh

# Full E-sweep (about 4.5 h; run it under tmux so it survives a dropped ssh)
tmux new -s sweep 'bench/sweep.sh'
```

`sweep.sh` skips runs that already have results, so re-running it resumes.

## 8. Bring results home

From your laptop:

```sh
rsync -a <you>@<server-hostname>:scx_slo/results/ ./results/
python3 analysis/parse_runs.py results/w1
```

## One run by hand

```sh
bench/run.sh B5-fixedk 60000 1 adhoc                 # VARIANT QPS REP [EXPERIMENT]
DURATION=20 BATCH=kbuild BATCH_K=4 bench/run.sh B5-fixedk 60000 1 adhoc
```

Each run folder (`results/EXP/VARIANT/qpsN/repR/`) holds `meta.json`,
`memtier.json`/`memtier.txt` (latency, throughput), `batch.json` (object files
compiled, frames encoded), `mpstat.txt` (CPU use), and for scheduler variants
`sched.csv` (dispatch statistics per second) and `sched.err`.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `check.sh`: cannot ssh to client | Log in again with `ssh -A`; check that `ssh 10.10.1.2 true` works |
| sched_ext state stays `enabled` after a crash | `sudo pkill -INT scx_slo scx_rustland`; the kernel also unloads a stalled scheduler by itself |
| `ext_exec: Invalid argument` | Kernel without sched_ext; check `uname -r` and `/sys/kernel/sched_ext` |
| Batch numbers are 0 | Look at `kbuild.log` / `ffmpeg.log` in the run folder |
| `B5-fixedk` p99 looks like `B1` | Check `sched_congested` in `sched.csv`: overflowing tasks run on any CPU |
| `memtier.json` missing | Look at `memtier.txt`; the client may not reach `10.10.1.1:11211` |
