<div align="center">
  <a href="../README.md"><b>hartwood</b></a>
</div>

# The roadmap

This file gives the order of the work.
Each milestone has a goal, the work in it, an exit test, and its status.
Do not start a milestone until the milestone that it needs passes its exit
test.
Each milestone is many small commits.
[DECISIONS.md](DECISIONS.md) gives the reason for each choice.

```mermaid
flowchart LR
    M0[M0 scaffold] --> M1[M1 RV64I]
    M1 --> M2[M2 M, A, C]
    M2 --> M3[M3 privilege]
    M3 --> M4[M4 Sv39]
    M4 --> M5[M5 F and D]
    M5 --> M6[M6 devices and xv6]
    M6 --> M7[M7 Linux]
    M7 --> M8[M8 web]
    M8 --> M9[M9 speed and polish]
```

| Milestone | Status | Exit test |
|---|---|---|
| M0 | Done | A test program exits with the code that it writes to `tohost`. |
| M1 | Done | Each `rv64ui-p-*` test passes. |
| M2 | Done | Each `rv64um-p-*`, `rv64ua-p-*` and `rv64uc-p-*` test passes. |
| M3 | Done but for one test | Each `rv64mi-p-*` and `rv64si-p-*` test passes. |
| M4 | Done | The `-v-` form of each test above passes. |
| M5 | Not started | Each `rv64uf-*` and `rv64ud-*` test passes. |
| M6 | Not started | xv6 boots to its shell, and `usertests` passes. |
| M7 | Not started | Linux boots to a BusyBox shell. |
| M8 | Not started | Linux boots in the browser, and the debugger works on xv6. |
| M9 | Not started | The README has the benchmarks and the demo is on GitHub Pages. |

---

## M0: the scaffold

Work:

- The workspace, the pinned toolchain, the lints and the CI.
- The RAM, the bus, an ELF reader, and the HTIF of the riscv-tests programs.
- `hartwood run <elf>` runs a program until it writes to `tohost`.
- The `hartwood-testsuite` harness, and the script that builds riscv-tests.

Exit test: `crates/core/tests/exit_code.rs` and `crates/cli/tests/run.rs`
build a program, run it, and check its exit code. A program that
`riscv64-elf-gcc` builds exits with 42 through `hartwood run`.

Status: done on 2026-11-13. 24 tests pass.

## M1: the RV64I base

Work: the decoder and each RV64I instruction. The riscv-tests environment
uses `csrr`, `csrw`, `ecall` and `mret` in its start code, so M1 also adds
the CSRs and the trap path that the start code needs.

Exit test: `hartwood-testsuite rv64ui-p-` reports no failure.

Status: done on 2026-11-13. 54 of 54 `rv64ui-p` tests pass, and 50 unit
and integration tests pass. The same run shows 12 of 17 `rv64mi-p` tests
and 4 of 7 `rv64si-p` tests that already pass.

## M2: the M, A and C extensions

Work: multiply and divide with the edge cases of the specification, the
atomic instructions with a reservation for LR and SC, and the compressed
instructions, which the decoder expands.

Exit test: `hartwood-testsuite rv64um-p- rv64ua-p- rv64uc-p-` reports no
failure.

Status: done on 2026-11-13. 13 of 13 `rv64um-p`, 19 of 19 `rv64ua-p` and 1
of 1 `rv64uc-p` tests pass, and the 54 `rv64ui-p` tests still pass. 58 unit
and integration tests pass. Each pair in the test of the compressed decoder
comes from `riscv64-elf-as`.

## M3: Zicsr, Zifencei and the privilege levels

Work: the CSR file, the M, S and U modes, traps, `mret` and `sret`,
delegation, the pending and enable bits of interrupts, `wfi` and `fence.i`.

Exit test: `hartwood-testsuite rv64mi-p- rv64si-p-` reports no failure.

Status: 16 of 17 `rv64mi-p` and 7 of 7 `rv64si-p` tests pass. Two of the
`rv64si-p` tests (`dirty` and `icache-alias`) turn on paging, so they pass
only with M4. `rv64mi-p-csr` runs `fmv.s.x` to check that a store of a float
does nothing while `mstatus.FS` is off. It fails until M5 adds F.
M3 also added 16 PMP entries, the trigger CSRs with no trigger, and counters
that a write can set.

## M4: the Sv39 MMU

Work: the page table walk, the permission checks with SUM, MXR and the A and
D bits, page faults, a TLB, and `sfence.vma`.

Exit test: the `-v-` form of each suite above reports no failure.

Status: done on 2026-11-13. Each `-v-` test of `rv64ui`, `rv64um`, `rv64ua`
and `rv64uc` passes: 87 of 87. 73 unit and integration tests pass, and seven
of them build a page table and check the walk, A and D, SUM, the TLB and a
load across two pages.

## M5: the F and D extensions

Work: single and double precision, `fcsr`, the rounding modes, the exception
flags, and NaN boxing. This completes RV64GC.

Exit test: `hartwood-testsuite rv64uf- rv64ud-` reports no failure.

## M6: the devices and xv6

Work: the CLINT, the PLIC, a 16550 UART, a virtio block device, and a
device tree that matches the `virt` map.

Exit test: xv6 boots to its shell, `usertests` passes, and a person can
type commands in `hartwood run`.

## M7: Linux

Work: OpenSBI `fw_jump`, a Linux kernel and a Buildroot initramfs, with the
device tree of M6. The build scripts are in `scripts/`.

Exit test: Linux boots to a BusyBox shell, and `uname -a`, `ls /` and
`cat /proc/cpuinfo` work.

## M8: the web frontend

Work: the WASM bindings, a Vite and TypeScript page with an xterm.js
console, run, pause and step, a breakpoint on a PC, the registers and the
CSRs, a memory view, and a disassembly around the PC. The emulator runs in a
Web Worker.

Exit test: Linux boots in the browser to a shell, and the debugger works on
xv6.

## M9: speed and polish

Work: a profile first, then a cache of decoded instructions and fast paths
for memory. The benchmarks before and after, the README, and the static
site.

Exit test: the README has the architecture diagram, the table of test
results, the benchmarks, a recording of the boot in the browser, and the
design decisions. The demo is on GitHub Pages.
