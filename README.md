<div align="center">

<img src="assets/logo.svg" alt="hartwood" height="120">

# hartwood

### An RV64GC emulator in Rust that boots Linux in the browser

_One core, for the terminal and for the web. No JIT, and no unsafe code._

![Rust](https://img.shields.io/badge/rust-09090b?style=for-the-badge&logo=rust&logoColor=f4f4f5)
![RISC-V](https://img.shields.io/badge/risc--v-27272a?style=for-the-badge&logo=riscv&logoColor=f4f4f5)
![WebAssembly](https://img.shields.io/badge/webassembly-3f3f46?style=for-the-badge&logo=webassembly&logoColor=f4f4f5)
[![MIT licence](https://img.shields.io/badge/mit_licence-e4e4e7?style=for-the-badge&logoColor=09090b)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/Stiven-Gjekaj/hartwood/ci.yml?branch=main&style=for-the-badge&label=ci&labelColor=f4f4f5&color=f4f4f5)](https://github.com/Stiven-Gjekaj/hartwood/actions/workflows/ci.yml)

<p align="center">
  <a href="https://stiven-gjekaj.github.io/hartwood/"><b>Live demo</b></a> |
  <a href="#overview"><b>Overview</b></a> |
  <a href="#architecture"><b>Architecture</b></a> |
  <a href="#test-results"><b>Tests</b></a> |
  <a href="#speed"><b>Speed</b></a> |
  <a href="#run-it"><b>Run it</b></a> |
  <a href="#design-decisions"><b>Decisions</b></a> |
  <a href="docs/roadmap.md"><b>Roadmap</b></a>
</p>

</div>

---

STATUS_NOTE

## Overview

hartwood emulates one RISC-V hart with the RV64GC instruction set: the base
integer set, multiply and divide, atomics, single and double precision
floating point, and compressed instructions. It has the M, S and U privilege
levels, the Sv39 MMU, 16 PMP entries and the Sstc timer.
Around the hart are the devices of the QEMU `virt` machine: a CLINT, a PLIC,
a 16550 UART, a virtio block device and the test device that turns the
machine off. hartwood writes the device tree, so OpenSBI and Linux find each
device with no change.

The same core crate builds for a native host and for WebAssembly:

- `hartwood run` boots a program on the terminal, with the UART as the
  console.
- The [web demo](https://stiven-gjekaj.github.io/hartwood/) runs the core in
  a Web Worker, with an xterm.js console and a debugger.

DEMO_GIF

## What it does

| Area | Work |
|---|---|
| ISA | RV64I, M, A, F, D, C, Zicsr, Zifencei. The floating point is IEEE 754 in software, with each rounding mode and each flag. |
| Privilege | M, S and U modes, traps, `medeleg` and `mideleg`, interrupts, `wfi`, `mret` and `sret`, the counters, 16 PMP entries, Sstc. |
| Memory | Sv39 with a TLB and the hardware update of A and D. A misaligned load or store in RAM works. |
| Devices | CLINT, PLIC, 16550 UART, virtio block (MMIO, version 2), the `sifive,test0` power device, and a generated device tree. |
| Guests | The riscv-tests suite, xv6-riscv, and OpenSBI with Linux 6.18 and BusyBox. |
| Debugger | Run, pause, step, breakpoints, the registers and CSRs, memory through the page table, and a disassembly. |

## Architecture

```mermaid
flowchart LR
    subgraph host["Host"]
        CLI["hartwood run<br/>(terminal)"]
        WEB["Web Worker<br/>(wasm-bindgen)"]
        TS["hartwood-testsuite"]
    end
    subgraph core["hartwood-core (no I/O)"]
        CPU["Cpu<br/>fetch, decode cache,<br/>execute, traps"]
        MMU["Sv39 MMU + TLB"]
        PMP["PMP"]
        FPU["softfloat"]
        BUS["Bus<br/>(one match on the virt map)"]
        RAM["RAM"]
        CLINT["CLINT"]
        PLIC["PLIC"]
        UART["UART 16550"]
        VIRTIO["virtio-blk"]
        TEST["test device"]
        FDT["device tree writer"]
    end
    CLI --> CPU
    WEB --> CPU
    TS --> CPU
    CPU --> MMU --> PMP --> BUS
    CPU --> FPU
    BUS --> RAM
    BUS --> CLINT
    BUS --> PLIC
    BUS --> UART
    BUS --> VIRTIO
    BUS --> TEST
    VIRTIO -. DMA .-> RAM
    UART -. line 10 .-> PLIC
    VIRTIO -. line 1 .-> PLIC
    PLIC -. MEIP, SEIP .-> CPU
    CLINT -. MTIP, MSIP .-> CPU
```

| Crate | Work |
|---|---|
| [`crates/core`](crates/core) | The hart, the MMU, the bus, the devices, the ELF reader, the boot layout, the device tree, the disassembler. It reads no file and no clock, so it builds for `wasm32-unknown-unknown`. |
| [`crates/cli`](crates/cli) | `hartwood run`: the terminal console, `--trace`, scripted input, and the boot of an ELF file or of OpenSBI with a kernel. |
| [`crates/testsuite`](crates/testsuite) | Runs the riscv-tests programs and prints a table. |
| [`crates/wasm`](crates/wasm) | The wasm-bindgen API that the web frontend calls. |
| [`web`](web) | Vite, TypeScript and xterm.js. The emulator runs in a Web Worker. |

The time of the machine comes from the count of steps, not from the clock
of the host, so each run of a program is the same run. `wfi` moves the time
to the next timer event, so an idle guest costs nothing.

## Test results

TEST_TABLE

## Speed

SPEED_TABLE

## Run it

hartwood needs Rust. The toolchain file installs the pinned version.

    cargo build --release -p hartwood-cli
    target/release/hartwood run <program.elf>

Ctrl-A then X leaves the emulator, as in QEMU.

### The riscv-tests suite

The suite needs a RISC-V compiler for bare metal.

    brew install riscv64-elf-gcc                     # macOS
    sudo apt-get install gcc-riscv64-unknown-elf     # Ubuntu
    scripts/build-riscv-tests.sh
    cargo run --release -p hartwood-testsuite -- rv64

### xv6

    scripts/build-xv6.sh
    target/release/hartwood run build/xv6/kernel --drive build/xv6/fs.img

### Linux

The images build on Linux, and the images workflow in CI builds them on
each push. On Ubuntu:

    sudo apt-get install gcc-riscv64-linux-gnu flex bison bc cpio libssl-dev libelf-dev
    scripts/build-linux.sh
    target/release/hartwood run build/linux/fw_jump.bin --kernel build/linux/Image \
        --initrd build/linux/rootfs.cpio --ram-mib 256

On a Mac, take them from the last green run of the images workflow:

    gh run download --name linux --dir build/linux \
        $(gh run list --workflow images.yml --status success --limit 1 --json databaseId --jq '.[0].databaseId')

### The web frontend

The web frontend needs `wasm-pack` and Node.js. The boot images go in
`web/public/images/xv6` and `web/public/images/linux`.

    rustup target add wasm32-unknown-unknown
    brew install wasm-pack
    cd web && npm ci && npm run wasm && npm run dev

### Debug a guest

`--trace` prints each instruction, and `--trace-from <step>` starts the trace
late. Compare it with `qemu-system-riscv64 -d in_asm` to find the first
instruction where the two disagree.

## Design decisions

[docs/DECISIONS.md](docs/DECISIONS.md) holds each decision in full, with the
options that lost. In short:

- **One match on a fixed memory map** dispatches each access (D4). The map
  never changes, so a list of devices behind a trait gives nothing.
- **The decoder makes an operation and its operands** in one struct (D5).
  A compressed instruction becomes its 32-bit form first, so the code that
  runs it exists once.
- **A cache of decoded instructions keyed by the bits** (D26). The decode is
  a pure function of the bits, so an entry is never stale and the cache
  needs no flush, not even on `fence.i`.
- **The floating point is a module of its own** (D18). The host FPU has one
  rounding mode and no portable flags. Each result is exact in a `u128`
  first, then rounded once. 200 000 random cases match the host bit for bit.
- **A direct mapped TLB of 4 KiB entries** (D16). A hit checks the rights
  again, because SUM, MXR and the mode can change with no fence.
- **The hart sets A and D** (D17), as `rv64si-p-dirty` expects.
- **16 PMP entries with real checks** (D15), with each decision cached with
  the range that it covers. A first version had no PMP, and
  `rv64mi-p-pmpaddr` showed that this was wrong.
- **Time counts steps; `wfi` jumps to the next timer** (D19). Each run is the
  same run, and an idle guest costs no host time.
- **The device tree comes from the constants of the bus** (D22), so the tree
  and the bus cannot disagree.
- **The core does no I/O and uses std** (D3). CI builds it for wasm32, so a
  use of the file system breaks the build and not the page.
- **Linux comes from three direct builds, not from Buildroot** (D23): six
  minutes in CI instead of an hour, and the same shell.
- **The web worker runs slices of 20 ms and yields through a
  `MessageChannel`** (D25), because a hidden tab slows `setTimeout`.

## Future work

The plan put these out of scope: a JIT, the vector extension, the
hypervisor extension, more than one hart, and networking. Each one is a
good next step. So are the extensions that Linux uses when it finds them
(Zba, Zbb, Svnapot, Zicbom), and a cache of basic blocks.

## Licence

hartwood is under the [MIT licence](LICENSE). [TERMS.md](TERMS.md) says
what the licences of the guest images are.
