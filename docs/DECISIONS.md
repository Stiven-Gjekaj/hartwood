<div align="center">
  <a href="../README.md"><b>hartwood</b></a>
</div>

# Decisions

Each entry is one choice that is not obvious.
It gives the options, the choice, and the reason.
The options that lost stay here, because the reason that a choice lost is
the part that a later reader needs.

The newest entry is at the end.

---

## D1: The name is hartwood, not rvemu

**Options:** `rvemu`, `hartwood`, `hartline`, `trapdoor`.

**Choice:** `hartwood`. The owner chose it on 2026-11-13.

**Reason:** `d0iasm/rvemu` is a RISC-V emulator in Rust that runs xv6 in the
browser, and the crate name `rvemu` is taken. A portfolio project with the
same name as a known project with the same goal looks like a copy.
A "hart" is a hardware thread in the RISC-V specification.

## D2: GitHub Actions builds the guest images

**Options:** build each image on the Mac of the author, build them in a
Linux container on the Mac, build them in CI, or commit binary images.

**Choice:** a CI job on Ubuntu builds the test programs and the boot images
from fixed source commits. A script in `scripts/` holds each build, so a
person can run the same steps on Linux.

**Reason:** Buildroot does not build on macOS, and the Mac has no Docker.
A binary in the repository cannot be checked against its source. A script
with a fixed commit can.
The riscv-tests programs are small and need only a bare metal compiler, so
they also build on the Mac with `riscv64-elf-gcc` from Homebrew.

## D3: The core uses std and does no I/O

**Options:** `#![no_std]` with `alloc`, or std with a rule against I/O.

**Choice:** the core crate uses std. It does not touch files, the clock,
threads, the network or a terminal. The host gives it bytes and takes bytes.
A CI job builds the core for `wasm32-unknown-unknown`.

**Reason:** std builds for `wasm32-unknown-unknown`. Only the file system,
threads and the clock are missing there, and those are the things that the
rule forbids anyway. `no_std` gives a second target that nobody needs, and
it makes `Vec`, `String` and `format!` an import each.
The CI job turns a break of the rule into a failed build, and not a failed
page in the browser.

## D4: The bus dispatches with one match on a fixed memory map

**Options:** a list of `Box<dyn Device>` with address ranges, or one
`match` on the address with a field for each device.

**Choice:** one `match` in `Bus::read` and `Bus::write`. The map is the map
of the QEMU `virt` machine.

**Reason:** the map never changes at run time, so a dynamic list gives
nothing. A `match` is plain to read, the compiler checks it, and it has no
indirect call on the path of each load and store. A device test still makes
one device alone.

## D5: The decoder makes a struct with an operation and its operands

**Options:** decode while the instruction runs, one enum variant with fields
for each instruction, or one struct with a field-less `Op` enum and the
operands.

**Choice:** `decode` gives an `Instruction`: an `Op`, the register numbers,
the immediate (sign extended), the raw bits and the length. `execute` is one
`match` on the `Op`. A compressed instruction becomes its 32-bit form before
the decode, so it gets the same `Op`.

**Reason:** all the bit work is in one place, and the code that runs an
instruction reads like the specification. A struct of fixed size is the
right unit for a cache of decoded instructions (M9). The disassembler and the
debugger read the same struct.

## D6: The HTIF looks at tohost after each store that touches it

**Options:** look at `tohost` after each instruction, or after each store
that touches its eight bytes.

**Choice:** `Bus::write` checks if a store overlaps `tohost`. If it does, the
bus reads the whole word, serves the command, and writes zero to `tohost`.

**Reason:** a check after each instruction costs a load on every step.
The riscv-tests environment writes `tohost` with two `sw`, low half first,
so the check must read the whole word after any store that overlaps it.
The environment with virtual memory waits until `tohost` is zero before it
sends the next command, so the bus must clear it.

## D7: The riscv-tests programs build from a fixed commit, and are not in Git

**Options:** commit the 244 programs, or build them with a script.

**Choice:** `scripts/build-riscv-tests.sh` clones the suite at a fixed
commit and builds the RV64 suites into `build/`, which Git ignores.

**Reason:** the tests are the oracle, so their source must be the one that
the commit names. The build takes less than a minute.
The environment with virtual memory includes `<string.h>`, `<stdio.h>` and
`<ctype.h>`. A bare metal compiler with no C library has none of them, so
`scripts/riscv-tests-include` declares the few functions that the
environment defines itself. The script does not change a test.

## D8: The tests compile the emulator with optimisation

**Choice:** `[profile.test] opt-level = 3`.

**Reason:** a test runs a whole program on the emulator. A build with no
optimisation runs it about twenty times slower, and a boot test would take
minutes.

## D9: Many small commits, and not one commit for each milestone

**Choice:** each commit does one step, as AGENTS.md says. A milestone ends
with an update of [the roadmap](roadmap.md).

**Reason:** the plan asked for at least one commit for each milestone.
AGENTS.md asks for one step in each commit. Both are true when a milestone
is many commits.

## D10: The roadmap holds the milestones

**Choice:** the status of each milestone and its test counts are in
`docs/roadmap.md`, and the decisions are in this file.

**Reason:** the other projects of the author keep the order of the work in
`docs/roadmap.md`. The plan named the file `MILESTONES.md`. One name for one
job in each project of the author makes each project easier to read.

## D11: M1 includes the CSR file and the trap path

**Options:** add only RV64I in M1 and the CSRs in M3, or add the CSRs and
traps in M1.

**Choice:** M1 adds the whole CSR file, the trap path, `mret` and `sret`.

**Reason:** the start code of each riscv-tests program writes `mtvec`,
`mstatus` and `mepc`, then uses `mret` to enter the test. Each test ends with
`ecall`, and the trap handler writes `tohost`. No `rv64ui-p` test can pass
without them. M3 then adds what only its own tests need.

## D12: hartwood has no PMP

**Options:** implement the physical memory protection registers and check
each access, or give zero PMP entries.

**Choice:** `pmpcfg` and `pmpaddr` read as zero, and a write does nothing.

**Reason:** the specification allows zero PMP entries. OpenSBI and Linux
read the registers back to count the entries, and then use none. A check of
each access costs time on every load and store, and protects nothing in an
emulator with one program.

## D13: A misaligned load or store in RAM works, and does not trap

**Options:** trap on each misaligned access, or do it.

**Choice:** a misaligned load or store to RAM works. An atomic access that
is misaligned traps, as the A extension requires. An access to a device
must be aligned.

**Reason:** the specification allows both. Linux and OpenSBI can emulate a
misaligned access in a trap handler, but that path is slow and seldom
tested. The riscv-tests `ma_data` test accepts both.

## D14: The reservation of LR holds a physical address, and a trap ends it

**Options:** reserve the virtual address or the physical address. End the
reservation only at an `sc`, or also at a trap.

**Choice:** `lr` keeps the physical address. An `sc` or any trap ends the
reservation.

**Reason:** two virtual addresses can map one physical address, and the
reservation is on memory, not on a name for it. With one hart, no other
hart can break a reservation, so a trap is the event that must: after a
context switch, the `sc` of the old thread must fail. The specification
allows an implementation to fail an `sc` for more reasons than it lists.

## D15: hartwood has 16 PMP entries, and checks them

This entry replaces D12.

**Options:** zero PMP entries, entries that hold values and check nothing,
or 16 entries with the checks.

**Choice:** 16 entries with TOR, NA4 and NAPOT, the L bit, and a check of
each fetch, load, store and page table read. With no lock, an access from
M-mode skips the check at once.

**Reason:** D12 chose zero entries, which the specification allows. The
`rv64mi-p-pmpaddr` test then fails: it needs a `pmpaddr0` that holds a
value. The tests are the oracle, so D12 was wrong for this project.
Registers that hold a value and check nothing would pass the test and tell a
lie to OpenSBI, which then trusts its protection of its own memory. The
checks cost a loop over 16 entries on each access from S-mode or U-mode.
M9 measures that cost.

## D16: The TLB is direct mapped, and holds 4 KiB pages only

**Options:** no TLB, a fully associative TLB, or a direct mapped TLB.
For a superpage: one entry for the whole page, or one entry for each 4 KiB
page that the hart touches.

**Choice:** 256 entries, indexed by the low bits of the virtual page number.
A superpage fills one entry for each 4 KiB page. A hit checks the
permissions again. A store to a page that is not dirty walks again.
`sfence.vma` with `rs1 = x0` flushes each entry, and with an address it
flushes that page. A write to `satp` flushes each entry.

**Reason:** a walk is three reads of memory, and a hit is one compare.
A direct mapped TLB needs no search and no policy to replace an entry. One
size of page makes the lookup one index and one compare. The permissions
depend on the mode, SUM and MXR, which software can change with no fence,
so the entry keeps the bits of the PTE and not a decision. hartwood has no
ASIDs, so `rs2` of `sfence.vma` does not matter. The flush on a write to
`satp` costs little, and protects a program that changes the table and
forgets the fence.

## D17: The hart sets A and D in memory

**Options:** take a page fault when A or D is clear, and let software set
them (Svade), or set them in the page table (Svadu).

**Choice:** the walk sets A on each use and D on each store, after the
permission check passes.

**Reason:** `rv64si-p-dirty` checks that a store sets D. Linux works with
both. A page fault for each first use of a page makes a boot slower.
