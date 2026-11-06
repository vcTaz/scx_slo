# shellcheck shell=bash disable=SC2034
# Shared settings for the CloudLab scripts. Source it; don't run it.
# Override any value by exporting it before sourcing.

REPO="${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
WORK="${WORK:-/mydata}"                 # extra disk from profile.py

SERVER_IP="${SERVER_IP:-10.10.1.1}"
CLIENT_IP="${CLIENT_IP:-10.10.1.2}"

SCX_TAG="${SCX_TAG:-v1.1.4}"            # pinned sched-ext/scx release
MEMTIER_TAG="${MEMTIER_TAG:-2.5.2}"     # pinned memtier_benchmark release
KERNEL_SRC_VER="${KERNEL_SRC_VER:-6.12.57}"   # kernel tree compiled as the batch job (and fallback kernel)

SCX_DIR="$WORK/scx"
SCHED_BIN="$REPO/scheduler/target/release/scx_slo"
RUSTLAND_BIN="$SCX_DIR/target/release/scx_rustland"
EXT_EXEC="$REPO/tools/ext_exec"
KBUILD_DIR="$WORK/batch/linux-$KERNEL_SRC_VER"
FFMPEG_INPUT="$WORK/batch/input.mp4"

export PATH="$HOME/.cargo/bin:$PATH"

log() { printf '\033[1m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }
