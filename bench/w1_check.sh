#!/usr/bin/env bash
# W1 / midterm check, about 6 minutes: does the fair class (memcached on
# EEVDF) beat SCHED_EXT batch tasks on a contended CPU? (TAXONOMY R-class)
#
# Runs at LOAD_PCT of peak (default 50%):
#   B0-alone    memcached only                         -> best case
#   B1-eevdf    batch on EEVDF next to memcached       -> cost of co-location
#   B4-partial  batch under scx_slo (partial mode)     -> class priority alone
#   B5-fixedk   batch confined to BATCH_K CPUs          -> static split via sched_ext
#
# If B4's p99 is close to B0's, fair-over-ext priority already protects
# memcached, and scx_slo's value has to come from SMT/cache interference and
# batch throughput. If B4 looks like B1, the controller has real work to do.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/../infra/cloudlab/env.sh"
source "$here/config.sh"

[ -f "$REPO/results/peak_qps.txt" ] || "$here/peak.sh"
peak=$(cat "$REPO/results/peak_qps.txt")
qps=$(( peak * ${LOAD_PCT:-50} / 100 ))
log "Peak $peak ops/s; running the W1 check at $qps ops/s."

for v in B0-alone B1-eevdf B4-partial B5-fixedk; do
    "$here/run.sh" "$v" "$qps" 1 w1
done
python3 "$REPO/analysis/parse_runs.py" "$REPO/results/w1"
