#!/usr/bin/env bash
# Set up the client node: memtier_benchmark built from a pinned release.
set -euo pipefail
source "$(dirname "$0")/env.sh"

sudo mkdir -p "$WORK" && sudo chown "$USER" "$WORK"

log "Installing build dependencies."
sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
    build-essential autoconf automake libevent-dev pkg-config zlib1g-dev libssl-dev git jq

if ! command -v memtier_benchmark >/dev/null || ! memtier_benchmark --version | grep -q "$MEMTIER_TAG"; then
    log "Building memtier_benchmark $MEMTIER_TAG."
    rm -rf "$WORK/memtier"
    git clone -q --depth 1 --branch "$MEMTIER_TAG" https://github.com/RedisLabs/memtier_benchmark.git "$WORK/memtier"
    (cd "$WORK/memtier" && autoreconf -ivf >/dev/null && ./configure -q && make -s -j"$(nproc)" && sudo make -s install)
fi
log "$(memtier_benchmark --version | head -1)"

if ping -c1 -W2 "$SERVER_IP" >/dev/null; then
    log "Server $SERVER_IP reachable."
else
    die "Cannot reach the server at $SERVER_IP over the experiment LAN."
fi
log "Client ready."
