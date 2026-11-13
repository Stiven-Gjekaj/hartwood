<div align="center">

# hartwood

### An RV64GC emulator in Rust that boots Linux in the browser

_One core for the command line and for the web._

![Rust](https://img.shields.io/badge/rust-09090b?style=for-the-badge&logo=rust&logoColor=f4f4f5)
![RISC-V](https://img.shields.io/badge/risc--v-27272a?style=for-the-badge&logo=riscv&logoColor=f4f4f5)
![WebAssembly](https://img.shields.io/badge/webassembly-3f3f46?style=for-the-badge&logo=webassembly&logoColor=f4f4f5)
[![MIT licence](https://img.shields.io/badge/mit_licence-e4e4e7?style=for-the-badge&logoColor=09090b)](LICENSE)

</div>

---

> [!NOTE]
> **Work has started. Nothing runs yet.**
> The plan has ten milestones, from an ELF loader to Linux in the browser.

## Overview

hartwood emulates one RISC-V hart with the RV64GC instruction set.
It has the M, S and U privilege levels and the Sv39 MMU.
It has the devices of the QEMU `virt` machine: a CLINT, a PLIC, a 16550 UART
and a virtio block device.

It has three goals:

1. Pass the official `riscv-tests` suite.
2. Boot xv6-riscv, then OpenSBI with a Linux kernel to a BusyBox shell.
3. Run in the browser as WebAssembly, with a console and a debugger.

## Build

hartwood needs Rust. The toolchain file installs the pinned version.

    cargo test --workspace
    cargo run -p hartwood-cli -- run <program.elf>

The riscv-tests suite needs a RISC-V compiler for bare metal.
On macOS:

    brew install riscv64-elf-gcc
    scripts/build-riscv-tests.sh
    cargo run --release -p hartwood-testsuite -- rv64ui-p-

On Ubuntu:

    sudo apt-get install gcc-riscv64-unknown-elf
    scripts/build-riscv-tests.sh

The web frontend needs `wasm-pack` and Node.js:

    rustup target add wasm32-unknown-unknown
    brew install wasm-pack

## Licence

hartwood is under the [MIT licence](LICENSE).
