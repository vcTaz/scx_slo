#!/usr/bin/env bash
# Sanity checks on the server before running experiments. Exits non-zero on
# the first failure.
set -euo pipefail
source "$(dirname "$0")/env.sh"

ok() { printf '  \033[32mok\033[0m   %s\n' "$*"; }
bad() { printf '  \033[31mFAIL\033[0m %s\n' "$*"; exit 1; }

echo "Server checks ($(hostname), $(uname -r), $(nproc) CPUs)"
[ -e /sys/kernel/sched_ext/state ] && ok "sched_ext present (state: $(cat /sys/kernel/sched_ext/state))" || bad "no /sys/kernel/sched_ext: wrong kernel"
[ "$(cat /sys/kernel/sched_ext/state)" = disabled ] && ok "no sched_ext scheduler loaded" || bad "a sched_ext scheduler is already loaded: $(cat /sys/kernel/sched_ext/root/ops 2>/dev/null)"
[ -x "$SCHED_BIN" ] && ok "scx_slo built" || bad "missing $SCHED_BIN: run setup-server.sh"
[ -x "$RUSTLAND_BIN" ] && ok "scx_rustland built" || bad "missing $RUSTLAND_BIN: run setup-server.sh"
[ -x "$EXT_EXEC" ] && ok "ext_exec built" || bad "missing $EXT_EXEC: run setup-server.sh"
command -v memcached >/dev/null && ok "memcached installed" || bad "memcached missing"
[ -d "$KBUILD_DIR" ] && ok "kernel-build batch source present" || bad "missing $KBUILD_DIR"
[ -f "$FFMPEG_INPUT" ] && ok "ffmpeg batch input present" || bad "missing $FFMPEG_INPUT"
ssh -o BatchMode=yes -o ConnectTimeout=5 "$CLIENT_IP" true 2>/dev/null \
    && ok "passwordless ssh to client $CLIENT_IP" \
    || bad "cannot ssh to $CLIENT_IP without a password (log in with 'ssh -A' so your agent is forwarded)"
ssh -o BatchMode=yes "$CLIENT_IP" 'command -v memtier_benchmark' >/dev/null \
    && ok "memtier_benchmark on client" || bad "memtier_benchmark missing on client: run setup-client.sh there"

# Load scx_slo for 3 s to prove the whole path works.
# shellcheck disable=SC2024
sudo "$SCHED_BIN" >/tmp/scx_slo_check.log 2>&1 &
pid=$!
for _ in $(seq 30); do [ "$(cat /sys/kernel/sched_ext/state)" = enabled ] && break; sleep 0.1; done
state=$(cat /sys/kernel/sched_ext/state)
ops=$(cat /sys/kernel/sched_ext/root/ops 2>/dev/null || echo '?')
sudo kill -INT "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
[ "$state" = enabled ] && ok "scx_slo loads (ops: $ops)" || bad "scx_slo did not load; see /tmp/scx_slo_check.log"
"$EXT_EXEC" true && ok "ext_exec can set SCHED_EXT" || bad "ext_exec failed"
echo "All checks passed."
