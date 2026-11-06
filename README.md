# scx_slo
SLO-aware CPU scheduling for co-located workloads with sched_ext. See
[docs/TAXONOMY.md](docs/TAXONOMY.md) and [docs/PLAN.md](docs/PLAN.md).

## Layout

- `crates/slo-ctrl`: the feedback controller and latency windows (pure Rust, no BPF).

## Test

```sh
cargo test
```
