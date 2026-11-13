//! The exit test of M0: a program that the test writes runs, and stops with
//! the code that it writes to `tohost`.

use hartwood_core::asm::{addi, auipc, program, sd};
use hartwood_core::bus::{Bus, RAM_BASE};
use hartwood_core::cpu::{Cpu, Stop};
use hartwood_core::elf::{Elf, builder};

/// Builds a program that writes `(code << 1) | 1` to `tohost`, the HTIF
/// command for an exit with `code`.
fn exit_program(code: i32) -> (Vec<u32>, u64) {
    let tohost = RAM_BASE + 0x1000;
    let text = vec![
        // t0 = tohost. `lui` sign extends on RV64, so the address comes
        // from the program counter, as a compiler makes it.
        auipc(5, 0x1000),
        addi(6, 0, (code << 1) | 1),
        sd(6, 5, 0),
    ];
    (text, tohost)
}

fn run(code: i32) -> Stop {
    let (text, tohost) = exit_program(code);
    let bytes = builder::build(RAM_BASE, &program(&text), &[("tohost", tohost)]);
    let elf = Elf::parse(&bytes).expect("the built ELF file reads");
    let mut cpu = Cpu::new(Bus::new(0x10_0000));
    cpu.load_elf(&elf).expect("the program fits in the RAM");
    cpu.run(100)
}

#[test]
fn the_program_exits_with_the_code_that_it_writes() {
    assert_eq!(run(0), Stop::Exit(0));
    assert_eq!(run(42), Stop::Exit(42));
}

#[test]
fn a_program_with_no_tohost_runs_to_the_step_limit() {
    // jal zero, 0: a loop on one instruction.
    let bytes = builder::build(RAM_BASE, &program(&[0x0000_006f]), &[]);
    let elf = Elf::parse(&bytes).expect("the built ELF file reads");
    let mut cpu = Cpu::new(Bus::new(0x1000));
    cpu.load_elf(&elf).unwrap();
    assert_eq!(cpu.run(1000), Stop::StepLimit);
    assert_eq!(cpu.instret, 1000);
}
