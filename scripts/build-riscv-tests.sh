#!/bin/sh
# Downloads the official riscv-tests suite at a fixed commit and builds the
# RV64 ISA tests into build/riscv-tests/isa.
#
# The tests are the oracle of this project, so the commit is fixed. A new
# commit of the suite is a commit of its own in this repository.
#
# It needs a RISC-V compiler for bare metal. The prefix comes from
# RISCV_PREFIX, or the script finds one of the usual names.
#
#     scripts/build-riscv-tests.sh
set -eu

COMMIT=3df703cd52d5f0d8224e3c50bf03c6cb70efff98
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIR="$ROOT/build/riscv-tests"

if [ -z "${RISCV_PREFIX:-}" ]; then
    for p in riscv64-unknown-elf- riscv64-elf- riscv64-linux-gnu-; do
        if command -v "${p}gcc" >/dev/null 2>&1; then
            RISCV_PREFIX=$p
            break
        fi
    done
fi
if [ -z "${RISCV_PREFIX:-}" ]; then
    echo "No RISC-V compiler found. Install one, or set RISCV_PREFIX." >&2
    exit 1
fi

if [ ! -d "$DIR/.git" ]; then
    git clone --quiet https://github.com/riscv-software-src/riscv-tests "$DIR"
fi
git -C "$DIR" fetch --quiet origin "$COMMIT"
git -C "$DIR" checkout --quiet "$COMMIT"
git -C "$DIR" submodule update --init --quiet

# Only the suites that hartwood implements. Each one names the extension that
# it tests, and RV64GC holds all of them.
SUITES="rv64ui rv64um rv64ua rv64uc rv64uf rv64ud rv64mi rv64si"
# The tests with virtual memory include three headers of the C library. A
# compiler for bare metal can come with no C library, so the headers in
# scripts/riscv-tests-include declare what the tests use. -ffreestanding
# makes the compiler give its own stdint.h.
OPTS="-static -mcmodel=medany -fvisibility=hidden -nostdlib -nostartfiles"
OPTS="$OPTS -Wl,--no-warn-rwx-segments -ffreestanding -I$ROOT/scripts/riscv-tests-include"
make -C "$DIR/isa" XLEN=64 RISCV_PREFIX="$RISCV_PREFIX" RISCV_GCC_OPTS="$OPTS" \
    $SUITES >/dev/null
count=$(find "$DIR/isa" -maxdepth 1 -name 'rv64*-[pv]-*' ! -name '*.dump' | wc -l)
echo "Built $count test programs in $DIR/isa"
