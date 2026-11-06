#!/usr/bin/env bash
# Set up the server node: sched_ext kernel, toolchain, schedulers, memcached,
# batch job inputs. Safe to re-run; each step skips what is already done.
#
# Run it twice: the first run installs a sched_ext kernel and asks you to
# reboot; the second run (after the reboot) builds everything else.
set -euo pipefail
source "$(dirname "$0")/env.sh"

sudo mkdir -p "$WORK" && sudo chown "$USER" "$WORK"

# ---- 1. Kernel with sched_ext -------------------------------------------
kernel_ok() { [ -e /sys/kernel/sched_ext/state ]; }

if ! kernel_ok; then
    log "Running $(uname -r), which has no sched_ext. Installing the Ubuntu HWE kernel."
    sudo apt-get update -q
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q linux-generic-hwe-24.04
    newest=$(ls /boot/vmlinuz-* | sed 's|/boot/vmlinuz-||' | sort -V | tail -1)
    if grep -q '^CONFIG_SCHED_CLASS_EXT=y' "/boot/config-$newest"; then
        log "Kernel $newest has CONFIG_SCHED_CLASS_EXT=y."
        log "Reboot now (sudo reboot), then run this script again."
        exit 0
    fi
    die "Kernel $newest lacks CONFIG_SCHED_CLASS_EXT. Build one with infra/cloudlab/build-kernel.sh, reboot, then re-run."
fi
log "Kernel $(uname -r) supports sched_ext (state: $(cat /sys/kernel/sched_ext/state))."

# ---- 2. Packages ---------------------------------------------------------
log "Installing packages."
sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
    build-essential clang llvm lld pkg-config git curl jq \
    libelf-dev zlib1g-dev libzstd-dev libseccomp-dev \
    "linux-tools-$(uname -r)" linux-tools-common \
    memcached ffmpeg sysstat \
    bc flex bison libssl-dev dwarves python3
clang_major=$(clang --version | sed -n 's/.*clang version \([0-9]*\).*/\1/p' | head -1)
[ "${clang_major:-0}" -ge 18 ] || die "scx needs clang >= 18, found $clang_major"

# memcached is started per run by bench/; the system service would compete.
sudo systemctl disable --now memcached >/dev/null 2>&1 || true

# ---- 3. Rust -------------------------------------------------------------
if ! command -v cargo >/dev/null; then
    log "Installing Rust."
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
rustup update stable >/dev/null 2>&1 || true
log "$(rustc --version)"

# ---- 4. Stock scx_rustland (baseline B3-rustland) ------------------------
if [ ! -x "$RUSTLAND_BIN" ]; then
    log "Building scx_rustland from sched-ext/scx $SCX_TAG."
    [ -d "$SCX_DIR" ] || git clone -q --depth 1 --branch "$SCX_TAG" https://github.com/sched-ext/scx.git "$SCX_DIR"
    (cd "$SCX_DIR" && cargo build --release -p scx_rustland)
fi

# ---- 5. Our scheduler (baselines B4-partial, B5-fixedk) ------------------
log "Building scx_slo."
(cd "$REPO/scheduler" && cargo build --release)

# ---- 6. ext_exec: run a command under SCHED_EXT ---------------------------
log "Building ext_exec."
gcc -O2 -Wall -Wextra -o "$EXT_EXEC" "$REPO/tools/ext_exec.c"

# ---- 7. Batch job inputs --------------------------------------------------
mkdir -p "$WORK/batch"
if [ ! -d "$KBUILD_DIR" ]; then
    log "Fetching Linux $KERNEL_SRC_VER source for the kernel-build batch job."
    major=${KERNEL_SRC_VER%%.*}
    curl -sSfL "https://cdn.kernel.org/pub/linux/kernel/v$major.x/linux-$KERNEL_SRC_VER.tar.xz" \
        | tar -xJ -C "$WORK/batch"
    make -C "$KBUILD_DIR" -s defconfig
fi
if [ ! -f "$FFMPEG_INPUT" ]; then
    log "Generating a 60 s 1080p test video for the ffmpeg batch job."
    ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=1920x1080:rate=30 -t 60 \
        -c:v libx264 -preset ultrafast "$FFMPEG_INPUT"
fi

# ---- 8. Less noise -------------------------------------------------------
if [ -d /sys/devices/system/cpu/cpu0/cpufreq ]; then
    log "Setting CPU governor to performance."
    echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor >/dev/null || true
fi

log "Server ready. Next: infra/cloudlab/check.sh, then bench/w1_check.sh (see docs/CLOUDLAB.md)."
