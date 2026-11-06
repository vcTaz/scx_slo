#!/usr/bin/env bash
# Find memcached's capacity alone (B0-alone), then its p99 at fractions of
# it. The results set the load axis for every sweep and inform the SLO.
#
#   bench/peak.sh            -> results/peak/, results/peak_qps.txt
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/../infra/cloudlab/env.sh"
source "$here/config.sh"

DURATION=${PEAK_DURATION:-30} "$here/run.sh" B0-alone 0 1 peak
max=$(jq -r '.. | objects | select(has("Totals")) | .Totals["Ops/sec"]' \
      "$REPO/results/peak/B0-alone/qps0/rep1/memtier.json" | head -1)
max=${max%.*}
[ -n "$max" ] && [ "$max" -gt 0 ] || die "could not read Ops/sec from memtier output"
log "Unlimited-rate throughput: $max ops/s"

# Latency curve alone: the knee of this curve is a sensible SLO.
for pct in 20 40 60 70 80 90; do
    qps=$(( max * pct / 100 ))
    DURATION=${PEAK_DURATION:-30} "$here/run.sh" B0-alone "$qps" 1 peak
done

echo "$max" >"$REPO/results/peak_qps.txt"
python3 "$REPO/analysis/parse_runs.py" "$REPO/results/peak"
log "Saved peak $max ops/s to results/peak_qps.txt."
log "Pick the SLO from the table above (e.g. the p99 at the knee), and set it in crates/slo-ctrl (PLACEHOLDER_SLO_NS)."
