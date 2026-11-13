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

#[test]
fn a_write_to_minstret_is_the_next_value_read() {
    use crate::csr::MINSTRET;
    let mut cpu = cpu_with(&[csrrw(0, MINSTRET, 5), csrrs(6, MINSTRET, 0)]);
    cpu.x[5] = 100;
    cpu.run(2);
    assert_eq!(cpu.x[6], 100);
}

/// Builds a page table in RAM that maps the virtual page `va` to the
/// physical page `pa` at level 0, with the leaf flags `flags`. Gives the
/// address of the leaf PTE. The three tables sit at the start of RAM + 64 KiB.
fn map_page(cpu: &mut Cpu, va: u64, pa: u64, flags: u64) -> u64 {
    let root = RAM_BASE + 0x1_0000;
    let mid = root + 0x1000;
    let leaf = root + 0x2000;
    let vpn = |level: u32| (va >> (12 + 9 * level)) & 0x1ff;
    let pointer = |table: u64| ((table >> 12) << 10) | 1;
    cpu.bus.write(root + vpn(2) * 8, 8, pointer(mid)).unwrap();
    cpu.bus.write(mid + vpn(1) * 8, 8, pointer(leaf)).unwrap();
    let pte_addr = leaf + vpn(0) * 8;
    cpu.bus
        .write(pte_addr, 8, ((pa >> 12) << 10) | flags | 1)
        .unwrap();
    cpu.csr.satp = (8 << 60) | (root >> 12);
    pte_addr
}

/// A hart with 128 KiB of RAM, in S-mode, with PMP open.
fn supervisor_cpu() -> Cpu {
    let mut cpu = Cpu::new(Bus::new(0x2_0000));
    cpu.csr.write(crate::csr::PMPADDR0, u64::MAX);
    cpu.csr.write(crate::csr::PMPCFG0, 0x1f);
    cpu.mode = Mode::Supervisor;
    cpu
}

const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const U: u64 = 1 << 4;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;

#[test]
fn a_load_goes_through_the_page_table_and_sets_a() {
    let mut cpu = supervisor_cpu();
    let pte = map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R);
    cpu.bus.write(RAM_BASE + 0x3008, 8, 0x1234).unwrap();
    assert_eq!(cpu.load(0x4000_0008, 8), Ok(0x1234));
    let flags = cpu.bus.read(pte, 8).unwrap() & 0xff;
    assert_eq!(flags & (A | D), A);
}

#[test]
fn a_store_sets_d_even_after_a_load_filled_the_tlb() {
    let mut cpu = supervisor_cpu();
    let pte = map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R | W);
    cpu.load(0x4000_0000, 8).unwrap();
    cpu.store(0x4000_0000, 8, 5).unwrap();
    assert_eq!(cpu.bus.read(pte, 8).unwrap() & (A | D), A | D);
    assert_eq!(cpu.bus.read(RAM_BASE + 0x3000, 8), Ok(5));
}

#[test]
fn a_store_to_a_read_only_page_faults_and_leaves_d_clear() {
    let mut cpu = supervisor_cpu();
    let pte = map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R);
    assert_eq!(
        cpu.store(0x4000_0010, 4, 1),
        Err(Exception::StorePageFault(0x4000_0010))
    );
    assert_eq!(cpu.bus.read(pte, 8).unwrap() & D, 0);
}

#[test]
fn supervisor_mode_reads_a_user_page_only_with_sum() {
    let mut cpu = supervisor_cpu();
    map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R | U);
    assert_eq!(
        cpu.load(0x4000_0000, 4),
        Err(Exception::LoadPageFault(0x4000_0000))
    );
    cpu.csr.mstatus |= crate::csr::MSTATUS_SUM;
    assert_eq!(cpu.load(0x4000_0000, 4), Ok(0));
}

#[test]
fn an_address_that_does_not_copy_bit_38_faults() {
    let mut cpu = supervisor_cpu();
    map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R);
    let bad = 0x0000_8000_4000_0000;
    assert_eq!(cpu.load(bad, 4), Err(Exception::LoadPageFault(bad)));
}

#[test]
fn sfence_vma_drops_a_stale_translation() {
    let mut cpu = supervisor_cpu();
    let pte = map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R);
    cpu.bus.write(RAM_BASE + 0x4000, 8, 7).unwrap();
    assert_eq!(cpu.load(0x4000_0000, 8), Ok(0));
    // Point the page somewhere else. The TLB still has the old page.
    cpu.bus
        .write(pte, 8, (((RAM_BASE + 0x4000) >> 12) << 10) | R | A | 1)
        .unwrap();
    assert_eq!(cpu.load(0x4000_0000, 8), Ok(0));
    cpu.tlb.flush();
    assert_eq!(cpu.load(0x4000_0000, 8), Ok(7));
}

#[test]
fn a_misaligned_load_reads_across_two_pages() {
    let mut cpu = supervisor_cpu();
    map_page(&mut cpu, 0x4000_0000, RAM_BASE + 0x3000, R);
    // The second page has no mapping.
    assert_eq!(
        cpu.load(0x4000_0ffe, 4),
        Err(Exception::LoadPageFault(0x4000_1000))
    );
    cpu.bus.write(RAM_BASE + 0x3ffc, 4, 0xaabb_ccdd).unwrap();
    assert_eq!(cpu.load(0x4000_0ffc, 4), Ok(0xaabb_ccdd));
}
