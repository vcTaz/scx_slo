#!/usr/bin/env bash
# Fallback only: build and install a kernel with sched_ext when the Ubuntu
# HWE kernel lacks CONFIG_SCHED_CLASS_EXT. Takes 20-40 minutes.
# Uses the running kernel's config plus scx's required options.
set -euo pipefail
source "$(dirname "$0")/env.sh"

sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
    build-essential bc flex bison libssl-dev libelf-dev dwarves zstd curl git

src="$WORK/kernel/linux-$KERNEL_SRC_VER"
if [ ! -d "$src" ]; then
    mkdir -p "$WORK/kernel"
    major=${KERNEL_SRC_VER%%.*}
    curl -sSfL "https://cdn.kernel.org/pub/linux/kernel/v$major.x/linux-$KERNEL_SRC_VER.tar.xz" | tar -xJ -C "$WORK/kernel"
fi
cd "$src"

cp "/boot/config-$(uname -r)" .config
# Ubuntu configs point at signing keys that are not on this machine.
scripts/config --set-str SYSTEM_TRUSTED_KEYS "" --set-str SYSTEM_REVOCATION_KEYS "" \
    --disable MODULE_SIG_FORCE
# Required by sched_ext (see sched-ext/scx kernel.config).
scripts/config --enable BPF --enable BPF_SYSCALL --enable BPF_JIT \
    --enable DEBUG_INFO --enable DEBUG_INFO_DWARF_TOOLCHAIN_DEFAULT --enable DEBUG_INFO_BTF \
    --enable BPF_JIT_ALWAYS_ON --enable BPF_JIT_DEFAULT_ON --enable SCHED_CLASS_EXT \
    --enable KALLSYMS_ALL
make olddefconfig
grep -q '^CONFIG_SCHED_CLASS_EXT=y' .config || die "CONFIG_SCHED_CLASS_EXT did not stick; check its dependencies in .config"

log "Building Linux $KERNEL_SRC_VER with sched_ext."
make -j"$(nproc)" LOCALVERSION=-scx
sudo make modules_install
sudo make install
log "Installed. Reboot (sudo reboot), check 'uname -r' ends in -scx, then re-run setup-server.sh."
