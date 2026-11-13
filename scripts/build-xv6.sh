#!/bin/sh
# Builds xv6-riscv at a fixed commit: build/xv6/kernel and build/xv6/fs.img.
#
# It needs a RISC-V compiler for bare metal, and a C compiler for the host,
# which builds the tool that makes the file system.
#
#     scripts/build-xv6.sh
set -eu

COMMIT=fa2ddcb7ca87a77af579380a6eafd5aff163b44f
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC="$ROOT/build/xv6-riscv"
OUT="$ROOT/build/xv6"

if [ -z "${TOOLPREFIX:-}" ]; then
    for p in riscv64-unknown-elf- riscv64-elf- riscv64-linux-gnu-; do
        if command -v "${p}gcc" >/dev/null 2>&1; then
            TOOLPREFIX=$p
            break
        fi
    done
fi

if [ ! -d "$SRC/.git" ]; then
    git clone --quiet https://github.com/mit-pdos/xv6-riscv "$SRC"
fi
git -C "$SRC" fetch --quiet origin "$COMMIT"
git -C "$SRC" checkout --quiet "$COMMIT"
make -C "$SRC" TOOLPREFIX="$TOOLPREFIX" kernel/kernel fs.img >/dev/null
mkdir -p "$OUT"
cp "$SRC/kernel/kernel" "$SRC/fs.img" "$OUT/"
ls -l "$OUT"
