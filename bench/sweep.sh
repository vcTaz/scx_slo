#!/usr/bin/env bash
# Load sweep (E-sweep): every variant x load x repetition.
#
#   bench/sweep.sh                       all variants, 10..90%, 5 reps (~4.5 h)
#   VARIANTS="B1-eevdf B4-partial" LOADS="30 50 70" REPS=3 bench/sweep.sh
#
# Runs are interleaved by repetition so drift over time spreads across
# variants. Existing run folders are skipped, so an interrupted sweep resumes.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/../infra/cloudlab/env.sh"
source "$here/config.sh"

[ -f "$REPO/results/peak_qps.txt" ] || die "run bench/peak.sh first"
peak=$(cat "$REPO/results/peak_qps.txt")
VARIANTS=${VARIANTS:-$ALL_VARIANTS}
LOADS=${LOADS:-"10 20 30 40 50 60 70 80 90"}
REPS=${REPS:-5}
EXP=${EXP:-sweep}

total=0; for _ in $VARIANTS; do for _ in $LOADS; do total=$((total + REPS)); done; done
log "Sweep '$EXP': $total runs of ${DURATION}s, peak $peak ops/s."

n=0
for rep in $(seq "$REPS"); do
    for load in $LOADS; do
        qps=$(( peak * load / 100 ))
        for v in $VARIANTS; do
            n=$((n + 1))
            if [ -f "$REPO/results/$EXP/$v/qps$qps/rep$rep/memtier.json" ]; then
                log "[$n/$total] skip $v load=$load% rep=$rep (done)"
                continue
            fi
            log "[$n/$total] $v load=$load% rep=$rep"
            "$here/run.sh" "$v" "$qps" "$rep" "$EXP" || log "run failed, continuing"
        done
    done
done
python3 "$REPO/analysis/parse_runs.py" "$REPO/results/$EXP"
