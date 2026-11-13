<div align="center">
  <a href="README.md"><b>hartwood</b></a>
</div>

# Contributing to hartwood

Thanks for your interest in hartwood.
It emulates an RV64GC hart and the devices of the QEMU `virt` machine, and
it boots xv6 and Linux in a terminal and in the browser.
Contributions of all kinds are welcome: bug reports, a guest that does not
boot, documentation fixes, a new device, and arguments against a decision
that is already made.

## The gate

Each change passes these, on macOS and on Linux:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    scripts/build-riscv-tests.sh
    cargo run --release -p hartwood-testsuite -- rv64

The last line runs 244 programs of the official riscv-tests suite, and each
one must pass. The README says how to install the RISC-V compiler that
builds them.

A change to a device, to the MMU or to the trap path must also boot xv6 and
Linux:

    scripts/build-xv6.sh
    cargo run --release -p hartwood-cli -- run build/xv6/kernel \
        --drive build/xv6/fs.img --input 'ls\n' --until zombie

The Linux images build on Linux with `scripts/build-linux.sh`. The images
workflow in CI builds them and boots them on QEMU and on hartwood.

Turn on the format check before each commit once in each clone:

    git config core.hooksPath .githooks

## The rule that comes before all the others

**The tests are the oracle.**

Never edit, skip or weaken a test to make it pass. That holds for the
riscv-tests suite, for the unit tests, and for the boot tests in CI.
If a test seems wrong, write down why in an issue, with the part of the
specification that says so, and stop.

## When a guest goes wrong

Compare it with QEMU. Run the same image on `qemu-system-riscv64 -machine
virt -d in_asm,cpu` and on `hartwood run --trace --trace-from <step>`, and
find the first instruction where the two disagree. That instruction is the
bug, or the instruction before it is.

## Commits

Each commit does one thing. The code and its tests go in the same commit.
The documentation goes in a commit of its own. [AGENTS.md](AGENTS.md) has
the full rules, and they apply to people too.

## Ways to contribute

- Report a guest that boots on QEMU and not on hartwood.
- Add a device of the `virt` machine, such as virtio-net or the RTC.
- Add an extension, such as Zba, Zbb or Svnapot, with its tests.
- Make the emulator faster, with a profile before and after.
- Improve the documentation.

Before you start significant work, open an issue to agree on the approach.
