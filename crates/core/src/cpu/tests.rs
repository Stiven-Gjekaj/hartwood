//! Tests of the trap path and the privileged instructions. Each test builds
//! its own program with the small assembler.

use super::*;
use crate::asm::{addi, i, program};
use crate::bus::RAM_BASE;
use crate::csr::{MCAUSE, MEPC, MSCRATCH, MSTATUS, MTVAL, MTVEC};

const ECALL: u32 = 0x0000_0073;
const MRET: u32 = 0x3020_0073;

fn csrrw(rd: u32, csr: u16, rs1: u32) -> u32 {
    i(0b111_0011, 0b001, rd, rs1, i32::from(csr))
}

fn csrrs(rd: u32, csr: u16, rs1: u32) -> u32 {
    i(0b111_0011, 0b010, rd, rs1, i32::from(csr))
}

/// Makes a hart with `code` at the base of RAM, and the program counter
/// there. PMP entry 0 gives all memory to each mode, as firmware does.
fn cpu_with(code: &[u32]) -> Cpu {
    let mut cpu = Cpu::new(Bus::new(0x1_0000));
    cpu.csr.write(crate::csr::PMPADDR0, u64::MAX);
    cpu.csr.write(crate::csr::PMPCFG0, 0x1f);
    cpu.bus.load_bytes(RAM_BASE, &program(code)).unwrap();
    cpu.pc = RAM_BASE;
    cpu
}

#[test]
fn a_csr_write_returns_the_old_value() {
    let mut cpu = cpu_with(&[addi(5, 0, 42), csrrw(6, MSCRATCH, 5), csrrw(7, MSCRATCH, 0)]);
    cpu.run(3);
    assert_eq!(cpu.x[6], 0);
    assert_eq!(cpu.x[7], 42);
}

#[test]
fn an_ecall_traps_to_mtvec() {
    let mut cpu = cpu_with(&[ECALL]);
    cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.step();
    assert_eq!(cpu.pc, RAM_BASE + 0x100);
    assert_eq!(cpu.csr.mcause, 11);
    assert_eq!(cpu.csr.mepc, RAM_BASE);
    assert_eq!(cpu.mode, Mode::Machine);
    // A trap is not a retired instruction.
    assert_eq!(cpu.instret, 0);
}

#[test]
fn mret_returns_to_the_mode_in_mpp() {
    let mut cpu = cpu_with(&[MRET]);
    cpu.csr.mepc = RAM_BASE + 0x40;
    cpu.csr.mstatus |= MSTATUS_MPIE;
    cpu.step();
    assert_eq!(cpu.pc, RAM_BASE + 0x40);
    assert_eq!(cpu.mode, Mode::User);
    assert_ne!(cpu.csr.mstatus & MSTATUS_MIE, 0);
    assert_ne!(cpu.csr.mstatus & MSTATUS_MPIE, 0);
}

#[test]
fn an_ecall_from_user_mode_has_its_own_cause() {
    let mut cpu = cpu_with(&[ECALL]);
    cpu.mode = Mode::User;
    cpu.step();
    assert_eq!(cpu.csr.mcause, 8);
    assert_eq!(cpu.csr.mpp(), Mode::User);
}

#[test]
fn a_machine_csr_traps_in_user_mode() {
    let raw = csrrs(5, MSTATUS, 0);
    let mut cpu = cpu_with(&[raw]);
    cpu.mode = Mode::User;
    cpu.step();
    assert_eq!(cpu.csr.mcause, 2);
    assert_eq!(cpu.csr.mtval, u64::from(raw));
}

#[test]
fn a_delegated_exception_goes_to_supervisor_mode() {
    let mut cpu = cpu_with(&[ECALL]);
    cpu.mode = Mode::User;
    cpu.csr.medeleg = 1 << 8;
    cpu.csr.stvec = RAM_BASE + 0x200;
    cpu.step();
    assert_eq!(cpu.mode, Mode::Supervisor);
    assert_eq!(cpu.pc, RAM_BASE + 0x200);
    assert_eq!(cpu.csr.scause, 8);
    assert_eq!(cpu.csr.mstatus & MSTATUS_SPP, 0);
}

#[test]
fn a_delegated_exception_in_machine_mode_stays_there() {
    let mut cpu = cpu_with(&[ECALL]);
    cpu.csr.medeleg = 1 << 11;
    cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.step();
    assert_eq!(cpu.mode, Mode::Machine);
    assert_eq!(cpu.pc, RAM_BASE + 0x100);
}

#[test]
fn a_vectored_interrupt_jumps_into_the_table() {
    let mut cpu = cpu_with(&[addi(0, 0, 0)]);
    cpu.csr.mtvec = (RAM_BASE + 0x100) | 1;
    cpu.csr.mie = Interrupt::MachineTimer.bit();
    cpu.csr.mstatus |= MSTATUS_MIE;
    cpu.csr.set_pending(Interrupt::MachineTimer, true);
    cpu.step();
    assert_eq!(cpu.pc, RAM_BASE + 0x100 + 4 * 7);
    assert_eq!(cpu.csr.mcause, (1 << 63) | 7);
    assert_eq!(cpu.csr.mepc, RAM_BASE);
}

#[test]
fn a_masked_interrupt_does_not_fire_in_its_own_mode() {
    let mut cpu = cpu_with(&[addi(5, 0, 1)]);
    cpu.csr.mie = Interrupt::MachineTimer.bit();
    cpu.csr.set_pending(Interrupt::MachineTimer, true);
    cpu.step();
    assert_eq!(cpu.x[5], 1);
    // In a lower mode, the interrupt for M-mode fires with MIE clear.
    cpu.mode = Mode::User;
    cpu.csr.mtvec = RAM_BASE + 0x80;
    cpu.step();
    assert_eq!(cpu.pc, RAM_BASE + 0x80);
}

#[test]
fn csrrs_with_x0_does_not_write_a_read_only_csr() {
    use crate::csr::MHARTID;
    let mut cpu = cpu_with(&[csrrs(5, MHARTID, 0), csrrs(6, MHARTID, 5)]);
    cpu.csr.mtvec = RAM_BASE + 0x100;
    cpu.step();
    assert_eq!(cpu.pc, RAM_BASE + 4);
    cpu.step();
    // csrrs with rs1 = x5 writes, so it traps, even though x5 is zero.
    assert_eq!(cpu.csr.mcause, 2);
    let _ = (MEPC, MCAUSE, MTVAL, MTVEC);
}

#[test]
fn user_mode_with_no_pmp_entry_gets_an_access_fault() {
    let mut cpu = cpu_with(&[addi(5, 0, 1)]);
    cpu.csr.write(crate::csr::PMPCFG0, 0);
    cpu.mode = Mode::User;
    cpu.step();
    assert_eq!(cpu.csr.mcause, 1);
    assert_eq!(cpu.x[5], 0);
}
