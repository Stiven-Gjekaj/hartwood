<div align="center">
  <a href="README.md"><img src="assets/logo.svg" alt="hartwood" height="44"></a>
</div>

# Changelog

Every release is written here, newest first. A version is `MAJOR.MINOR.PATCH`.

## Unreleased

## 0.1.0 - 2026-11-13

The first release.

### Added

- An RV64GC hart: RV64I, M, A, F, D, C, Zicsr and Zifencei, with the M, S
  and U modes, traps, delegation and interrupts. Each of the 244 programs of
  the riscv-tests suite for RV64GC passes.
- The Sv39 MMU with a TLB, and the hardware update of the A and D bits.
- 16 PMP entries, the Sstc timer, and the counters.
- The devices of the QEMU `virt` machine: the CLINT, the PLIC, a 16550
  UART, a virtio block device, and the test device for power off and reset.
  hartwood writes the device tree of the machine.
- `hartwood run`: boots an ELF program, or OpenSBI with a Linux kernel and
  an initramfs, with the UART on the terminal. `--trace`, `--input`,
  `--until` and `--stats` help to debug and to test.
- `hartwood-testsuite`: runs the riscv-tests programs and prints a table.
- A web frontend: the emulator in a Web Worker, an xterm.js console, run,
  pause and step, breakpoints, the registers, the CSRs, the memory and a
  disassembly.
- Scripts that build the riscv-tests suite, xv6, and OpenSBI with Linux and
  BusyBox, from fixed sources.
