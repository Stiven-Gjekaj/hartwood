#!/bin/sh
# Builds the three images that boot Linux on hartwood:
#
#   build/linux/fw_jump.bin   OpenSBI, which runs in M-mode at 0x80000000
#   build/linux/Image         the Linux kernel, which OpenSBI starts at
#                             0x80200000 in S-mode
#   build/linux/rootfs.cpio   an initramfs with BusyBox
#
# Each source has a fixed version and a checksum or a commit, so a build
# today and a build next year use the same source.
#
# It runs on Linux. On Ubuntu:
#
#     sudo apt-get install gcc-riscv64-linux-gnu flex bison bc cpio \
#         libssl-dev libelf-dev
#     scripts/build-linux.sh
set -eu

LINUX_VERSION=6.18.63
LINUX_SHA256=81764a545072d160748beff4d9de2e7d95aa9f16e8261110288f067a2cc61d71
BUSYBOX_VERSION=1.37.0
BUSYBOX_SHA256=3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4
OPENSBI_TAG=v1.9
OPENSBI_COMMIT=cbf9f6734dd85a982c63e3cb5db7ffe09da839ca

CROSS=${CROSS_COMPILE:-riscv64-linux-gnu-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC="$ROOT/build/linux-src"
OUT="$ROOT/build/linux"
JOBS=$(nproc 2>/dev/null || sysctl -n hw.ncpu)
mkdir -p "$SRC" "$OUT"

# Downloads a file once, and checks its SHA-256.
fetch() {
    url=$1
    sum=$2
    file="$SRC/$(basename "$url")"
    if [ ! -f "$file" ]; then
        curl -fsSL -o "$file.part" "$url"
        mv "$file.part" "$file"
    fi
    echo "$sum  $file" | sha256sum -c --quiet -
}

# OpenSBI. The generic platform reads the devices from the device tree.
if [ ! -d "$SRC/opensbi" ]; then
    git clone --quiet --depth 1 --branch "$OPENSBI_TAG" \
        https://github.com/riscv-software-src/opensbi "$SRC/opensbi"
fi
test "$(git -C "$SRC/opensbi" rev-parse HEAD)" = "$OPENSBI_COMMIT"
make -C "$SRC/opensbi" -j"$JOBS" CROSS_COMPILE="$CROSS" PLATFORM=generic \
    FW_JUMP=y >/dev/null
cp "$SRC/opensbi/build/platform/generic/firmware/fw_jump.bin" "$OUT/"

# The kernel: the smallest configuration, and then the options in
# scripts/linux.config.
fetch "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-$LINUX_VERSION.tar.xz" "$LINUX_SHA256"
if [ ! -d "$SRC/linux-$LINUX_VERSION" ]; then
    tar -xf "$SRC/linux-$LINUX_VERSION.tar.xz" -C "$SRC"
fi
KERNEL="$SRC/linux-$LINUX_VERSION"
make -C "$KERNEL" ARCH=riscv CROSS_COMPILE="$CROSS" tinyconfig >/dev/null
"$KERNEL/scripts/kconfig/merge_config.sh" -m -O "$KERNEL" \
    "$KERNEL/.config" "$ROOT/scripts/linux.config" >/dev/null
make -C "$KERNEL" ARCH=riscv CROSS_COMPILE="$CROSS" olddefconfig >/dev/null
# Each option in the fragment must survive olddefconfig. An option that a
# dependency removes would be lost with no message.
grep '^CONFIG_' "$ROOT/scripts/linux.config" | while read -r line; do
    if ! grep -qx "$line" "$KERNEL/.config"; then
        echo "The kernel configuration lost $line" >&2
        exit 1
    fi
done
make -C "$KERNEL" ARCH=riscv CROSS_COMPILE="$CROSS" -j"$JOBS" Image >/dev/null
cp "$KERNEL/arch/riscv/boot/Image" "$OUT/"

# BusyBox, linked statically, so the initramfs needs no C library.
fetch "https://busybox.net/downloads/busybox-$BUSYBOX_VERSION.tar.bz2" "$BUSYBOX_SHA256"
if [ ! -d "$SRC/busybox-$BUSYBOX_VERSION" ]; then
    tar -xf "$SRC/busybox-$BUSYBOX_VERSION.tar.bz2" -C "$SRC"
fi
BUSYBOX="$SRC/busybox-$BUSYBOX_VERSION"
make -C "$BUSYBOX" CROSS_COMPILE="$CROSS" defconfig >/dev/null
sed -i 's/^# CONFIG_STATIC is not set/CONFIG_STATIC=y/' "$BUSYBOX/.config"
# The traffic control applet does not build with the headers of new
# kernels, and a shell does not need it.
sed -i 's/^CONFIG_TC=y/# CONFIG_TC is not set/' "$BUSYBOX/.config"
make -C "$BUSYBOX" CROSS_COMPILE="$CROSS" oldconfig </dev/null >/dev/null
make -C "$BUSYBOX" CROSS_COMPILE="$CROSS" -j"$JOBS" >/dev/null

# The initramfs.
ROOTFS="$SRC/rootfs"
rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"
make -C "$BUSYBOX" CROSS_COMPILE="$CROSS" CONFIG_PREFIX="$ROOTFS" install >/dev/null
mkdir -p "$ROOTFS/proc" "$ROOTFS/sys" "$ROOTFS/dev" "$ROOTFS/tmp" "$ROOTFS/etc" "$ROOTFS/root"
cp "$ROOT/scripts/rootfs/init" "$ROOTFS/init"
chmod 755 "$ROOTFS/init"
(cd "$ROOTFS" && find . | sort | cpio --quiet -o -H newc --reproducible) > "$OUT/rootfs.cpio"

ls -l "$OUT"
