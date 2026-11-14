#!/bin/sh
# Measures the speed of a hartwood binary on two fixed workloads, and prints
# the median of five runs of each.
#
#     scripts/bench.sh target/release/hartwood
#
# The workloads: xv6 boots and runs `ls`, and Linux boots and runs three
# commands. Each run has the same steps on each host, because the time of
# the machine counts steps (D19). The speed is steps divided by the CPU time
# of the process, which a busy host disturbs less than the wall time.
#
# It needs build/xv6 from scripts/build-xv6.sh and build/linux from
# scripts/build-linux.sh or from the artifact of the images workflow.
set -eu

BIN=${1:?usage: scripts/bench.sh <hartwood binary>}
RUNS=5

run() {
    name=$1
    shift
    for i in $(seq $RUNS); do
        out=$( { /usr/bin/time -p "$BIN" run "$@" --stats >/dev/null; } 2>&1 )
        steps=$(echo "$out" | sed -n 's/^hartwood: \([0-9]*\) steps.*/\1/p')
        user=$(echo "$out" | awk '$1 == "user" { print $2 }')
        echo "$steps $user"
    done | sort -k2 -n | sed -n "$(( (RUNS + 1) / 2 ))p" | {
        read -r steps user
        awk -v n="$name" -v s="$steps" -v u="$user" \
            'BEGIN { printf "%-6s %12d steps  %6.2f s CPU  %6.1f million steps a second\n", n, s, u, s / u / 1e6 }'
    }
}

run xv6 build/xv6/kernel --drive build/xv6/fs.img --input 'ls\n' --until zombie
run linux build/linux/fw_jump.bin --kernel build/linux/Image --initrd build/linux/rootfs.cpio \
    --ram-mib 256 --input 'uname -a; ls /; cat /proc/cpuinfo; echo DONE-$((6*7))\n' --until DONE-42
