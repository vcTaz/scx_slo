# shellcheck shell=bash disable=SC2034
# Helpers for bench/*.sh. Source after infra/cloudlab/env.sh and bench/config.sh.
# Everything runs on the server; the client is driven over ssh.

BATCH_PIDS=()
SCHED_PID=""
MC_PID=""

client() { ssh -o BatchMode=yes "$CLIENT_IP" "$@"; }

sched_state() { cat /sys/kernel/sched_ext/state 2>/dev/null || echo missing; }

wait_sched_state() {   # wait_sched_state enabled|disabled
    for _ in $(seq 100); do [ "$(sched_state)" = "$1" ] && return 0; sleep 0.1; done
    die "sched_ext state is '$(sched_state)', expected '$1'"
}

start_memcached() {
    pkill -x memcached 2>/dev/null || true
    sleep 0.5
    memcached -p "$MC_PORT" -l "$SERVER_IP" -t "$MC_THREADS" -m "$MC_MEMORY_MB" -c 8192 -U 0 &
    MC_PID=$!
    sleep 1
    kill -0 "$MC_PID" 2>/dev/null || die "memcached failed to start"
    # Load every key once so GETs hit.
    client memtier_benchmark -s "$SERVER_IP" -p "$MC_PORT" --protocol=memcache_text \
        -t 1 -c 1 --ratio=1:0 --key-pattern=P:P --key-maximum="$MT_KEYS" \
        -d "$MT_VALUE_BYTES" --requests=allkeys --hide-histogram >/dev/null 2>&1 \
        || die "populating memcached from the client failed"
}

stop_memcached() {
    [ -n "$MC_PID" ] && kill "$MC_PID" 2>/dev/null || true
    MC_PID=""
}

# start_sched VARIANT OUTDIR
start_sched() {
    local variant=$1 out=$2 cmd=()
    case $variant in
        B3-rustland) cmd=("$RUSTLAND_BIN") ;;
        B4-partial)  cmd=("$SCHED_BIN") ;;
        B5-fixedk)   cmd=("$SCHED_BIN" --cpus "$(batch_cpuset)") ;;
        *) return 0 ;;
    esac
    [ "$(sched_state)" = disabled ] || die "a sched_ext scheduler is already loaded"
    # shellcheck disable=SC2024  # logs should be owned by the user
    sudo "${cmd[@]}" >"$out/sched.csv" 2>"$out/sched.err" &
    SCHED_PID=$!
    wait_sched_state enabled
    cat /sys/kernel/sched_ext/root/ops >"$out/sched_ops.txt" 2>/dev/null || true
}

stop_sched() {
    [ -n "$SCHED_PID" ] || return 0
    sudo pkill -INT -P "$SCHED_PID" 2>/dev/null || true   # sudo's child is the scheduler
    sudo kill -INT "$SCHED_PID" 2>/dev/null || true
    wait "$SCHED_PID" 2>/dev/null || true
    SCHED_PID=""
    wait_sched_state disabled
}

# Prefix that places a batch job for VARIANT on the right CPUs/class.
batch_prefix() {
    case $1 in
        B2-cgroup)
            echo "sudo systemd-run --scope --quiet --collect --uid=$(id -u) --gid=$(id -g) -p AllowedCPUs=$(batch_cpuset) --unit=scxslo-batch-$$-$RANDOM" ;;
        B4-partial|B5-fixedk) echo "$EXT_EXEC" ;;
        *) echo "" ;;
    esac
}

# start_batch VARIANT OUTDIR
start_batch() {
    local variant=$1 out=$2 prefix
    [ "$variant" = B0-alone ] && return 0
    prefix=$(batch_prefix "$variant")
    touch "$out/.batch_start"
    for job in $BATCH; do
        case $job in
            kbuild)
                make -C "$KBUILD_DIR" -s clean
                # shellcheck disable=SC2086
                setsid $prefix make -C "$KBUILD_DIR" -s -k -j"$BATCH_JOBS" vmlinux >"$out/kbuild.log" 2>&1 &
                BATCH_PIDS+=($!) ;;
            ffmpeg)
                # shellcheck disable=SC2086
                setsid $prefix ffmpeg -nostdin -loglevel error -y -stream_loop -1 -i "$FFMPEG_INPUT" \
                    -c:v libx264 -preset medium -f null - -progress "$out/ffmpeg_progress.txt" \
                    >"$out/ffmpeg.log" 2>&1 &
                BATCH_PIDS+=($!) ;;
            *) die "unknown batch job '$job'" ;;
        esac
    done
}

stop_batch() {
    local out=$1
    for pid in "${BATCH_PIDS[@]}"; do
        sudo kill -TERM -- "-$pid" 2>/dev/null || sudo kill -TERM "$pid" 2>/dev/null || true
    done
    sudo systemctl stop 'scxslo-batch-*.scope' 2>/dev/null || true
    sleep 1
    sudo pkill -KILL -f "$KBUILD_DIR" 2>/dev/null || true
    sudo pkill -KILL -f "$FFMPEG_INPUT" 2>/dev/null || true
    BATCH_PIDS=()
    if [ -f "$out/.batch_start" ]; then
        # Batch throughput: object files compiled and frames encoded during the run.
        local objs=0 frames=0
        [[ " $BATCH " == *" kbuild "* ]] && objs=$(find "$KBUILD_DIR" -name '*.o' -newer "$out/.batch_start" | wc -l)
        [ -f "$out/ffmpeg_progress.txt" ] && frames=$(grep -o '^frame=[0-9]*' "$out/ffmpeg_progress.txt" | tail -1 | cut -d= -f2)
        printf '{"kbuild_objects": %s, "ffmpeg_frames": %s}\n' "${objs:-0}" "${frames:-0}" >"$out/batch.json"
    fi
}

# run_memtier QPS OUTDIR  (QPS=0 means unlimited)
run_memtier() {
    local qps=$1 out=$2 rate_flag="" conns=$(( MT_THREADS * MT_CONNS ))
    if [ "$qps" -gt 0 ]; then
        rate_flag="--rate-limiting=$(( (qps + conns - 1) / conns ))"
    fi
    client memtier_benchmark -s "$SERVER_IP" -p "$MC_PORT" --protocol=memcache_text \
        -t "$MT_THREADS" -c "$MT_CONNS" --test-time="$DURATION" --ratio="$MT_RATIO" \
        --key-pattern=R:R --key-maximum="$MT_KEYS" -d "$MT_VALUE_BYTES" \
        --distinct-client-seed $rate_flag --print-percentiles=50,90,99,99.9 \
        --hide-histogram --json-out-file=/tmp/memtier.json >"$out/memtier.txt" 2>&1 \
        || die "memtier_benchmark failed; see $out/memtier.txt"
    scp -q -o BatchMode=yes "$CLIENT_IP:/tmp/memtier.json" "$out/memtier.json"
}

cleanup_all() {
    stop_batch "${OUT:-/tmp}" 2>/dev/null || true
    [ -n "$SCHED_PID" ] && stop_sched || true
    stop_memcached
    pkill -x mpstat 2>/dev/null || true
}
