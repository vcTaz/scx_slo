#!/usr/bin/env bash
# One run point: bench/run.sh VARIANT QPS REP [EXPERIMENT]
#
#   VARIANT     B0-alone | B1-eevdf | B2-cgroup | B3-rustland | B4-partial | B5-fixedk
#   QPS         total memcached requests/s offered by the client (0 = unlimited)
#   REP         repetition number
#   EXPERIMENT  results subfolder (default: adhoc)
#
# Writes results/EXPERIMENT/VARIANT/qpsQPS/repREP/.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/../infra/cloudlab/env.sh"
source "$here/config.sh"
source "$here/lib.sh"

[ $# -ge 3 ] || die "usage: $0 VARIANT QPS REP [EXPERIMENT]"
variant=$1 qps=$2 rep=$3 exp=${4:-adhoc}
[[ " $ALL_VARIANTS " == *" $variant "* ]] || die "unknown variant '$variant' (one of: $ALL_VARIANTS)"

OUT="$REPO/results/$exp/$variant/qps$qps/rep$rep"
rm -rf "$OUT" && mkdir -p "$OUT"
trap cleanup_all EXIT

cat >"$OUT/meta.json" <<EOF
{"variant": "$variant", "qps": $qps, "rep": $rep, "experiment": "$exp",
 "duration_s": $DURATION, "batch": "$BATCH", "batch_jobs": $BATCH_JOBS,
 "batch_cpus": "$( [[ $variant == B2-cgroup || $variant == B5-fixedk ]] && batch_cpuset || echo all)",
 "kernel": "$(uname -r)", "nproc": $(nproc), "git": "$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)",
 "started": "$(date -Is)"}
EOF

log "$variant qps=$qps rep=$rep -> $OUT"
start_memcached
start_sched "$variant" "$OUT"
start_batch "$variant" "$OUT"
sleep "$SETTLE"

mpstat -P ALL 1 "$DURATION" >"$OUT/mpstat.txt" 2>&1 &
mpstat_pid=$!
run_memtier "$qps" "$OUT"
wait "$mpstat_pid" 2>/dev/null || true

stop_batch "$OUT"
stop_sched
stop_memcached
trap - EXIT

python3 "$REPO/analysis/parse_runs.py" --one "$OUT"
