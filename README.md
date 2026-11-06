# scx_slo
SLO-aware CPU scheduling for co-located workloads with sched_ext. See
[docs/TAXONOMY.md](docs/TAXONOMY.md) and [docs/PLAN.md](docs/PLAN.md).

## Layout

- `crates/slo-ctrl`: the feedback controller and latency windows (pure Rust, no BPF).
- `crates/slo-sim`: closed-loop simulation of the controller against a co-location model ([docs/SIMULATION.md](docs/SIMULATION.md)).
- `analysis/`: plotting (`plot_sim.py`, Python venv) and run summaries (`parse_runs.py`, standard library).
- `scheduler/`: the scx_slo sched_ext scheduler (builds on the CloudLab server only).
- `tools/ext_exec.c`: run a command under SCHED_EXT.
- `bench/`, `infra/cloudlab/`, `profile.py`: CloudLab testbed and experiments ([docs/CLOUDLAB.md](docs/CLOUDLAB.md)).

## Test

```sh
cargo test
```

## Simulate and plot

```sh
cargo run --release -p slo-sim
python3 -m venv --without-pip .venv && curl -sSL https://bootstrap.pypa.io/get-pip.py | .venv/bin/python
.venv/bin/pip install -r analysis/requirements.txt
.venv/bin/python analysis/plot_sim.py   # results/sim/plots/
```
