//! The hart: its registers, its privilege mode, and the loop that fetches,
//! decodes and runs one instruction at a time.

mod exec;
mod fpu;
mod mmu;

pub use mmu::Tlb;

use crate::bus::{Bus, BusFault};
use crate::csr::{
    Csr, CsrContext, MSTATUS_MIE, MSTATUS_MPIE, MSTATUS_MPP, MSTATUS_MPP_SHIFT, MSTATUS_MPRV,
    MSTATUS_SIE, MSTATUS_SPIE, MSTATUS_SPP, Mode,
};
use crate::decode::{Instruction, decode, decode_compressed};
use crate::elf::Elf;
use crate::trap::{Exception, Interrupt, Trap};

/// Why [`Cpu::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The program wrote an exit code to `tohost`.
    Exit(u64),
    /// The CPU ran the number of instructions that the caller allowed.
    StepLimit,
}

/// The kind of a memory access, for the MMU and for the fault that it
/// gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Fetch,
    Load,
    Store,
}

/// One RISC-V hart and the bus that it owns.
pub struct Cpu {
    /// The integer registers. `x[0]` is always zero.
    pub x: [u64; 32],
    /// The floating point registers, as raw bits.
    pub f: [u64; 32],
    /// The address of the next instruction.
    pub pc: u64,
    pub mode: Mode,
    pub csr: Csr,
    /// The number of instructions that retired.
    pub instret: u64,
    /// The number of steps: each instruction and each trap. hartwood counts
    /// one cycle for each step.
    pub cycle: u64,
    /// The physical address that `lr` reserved, until an `sc` or a trap.
    pub reservation: Option<u64>,
    pub tlb: Tlb,
    pub bus: Bus,
}

impl Cpu {
    /// Makes a hart that owns `bus`. It starts in M-mode with the program
    /// counter at zero, as after a reset.
    pub fn new(bus: Bus) -> Self {
        Self {
            x: [0; 32],
            f: [0; 32],
            pc: 0,
            mode: Mode::Machine,
            csr: Csr::new(),
            instret: 0,
            cycle: 0,
            reservation: None,
            tlb: Tlb::default(),
            bus,
        }
    }

    /// Copies each segment of an ELF file into memory, sets the program
    /// counter to its entry, and finds `tohost` and `fromhost`.
    pub fn load_elf(&mut self, elf: &Elf) -> Result<(), BusFault> {
        for seg in &elf.segments {
            self.bus.load_bytes(seg.addr, &seg.data)?;
            let zeros = seg.mem_size.saturating_sub(seg.data.len() as u64);
            let zeros = vec![0u8; usize::try_from(zeros).map_err(|_| BusFault)?];
            self.bus
                .load_bytes(seg.addr + seg.data.len() as u64, &zeros)?;
        }
        self.pc = elf.entry;
        self.bus.htif.tohost = elf.symbol("tohost");
        self.bus.htif.fromhost = elf.symbol("fromhost");
        Ok(())
    }

    /// Runs until the program exits, or for at most `max_steps`
    /// instructions.
    pub fn run(&mut self, max_steps: u64) -> Stop {
        for _ in 0..max_steps {
            self.step();
            if let Some(code) = self.bus.htif.exit_code {
                return Stop::Exit(code);
            }
        }
        Stop::StepLimit
    }

    /// Takes a pending interrupt, or runs one instruction. An exception in
    /// the instruction becomes a trap.
    pub fn step(&mut self) {
        if let Some(irq) = self.pending_interrupt() {
            self.take_trap(Trap::Interrupt(irq));
        } else {
            match self.fetch().and_then(|inst| self.execute(&inst)) {
                Ok(()) => self.instret = self.instret.wrapping_add(1),
                Err(e) => self.take_trap(Trap::Exception(e)),
            }
        }
        self.cycle = self.cycle.wrapping_add(1);
    }

    /// Reads and decodes the instruction at the program counter.
    ///
    /// The fetch reads 16 bits first. If the two low bits are not `11`, the
    /// instruction is compressed. Otherwise the fetch reads the next 16
    /// bits, which can be on the next page.
    pub(crate) fn fetch(&mut self) -> Result<Instruction, Exception> {
        let pc = self.pc;
        let low = self.fetch_half(pc)?;
        if low & 3 != 3 {
            return Ok(decode_compressed(low));
        }
        let high = self.fetch_half(pc.wrapping_add(2))?;
        Ok(decode((u32::from(high) << 16) | u32::from(low)))
    }

    fn fetch_half(&mut self, vaddr: u64) -> Result<u16, Exception> {
        let paddr = self.translate(vaddr, Access::Fetch)?;
        if !self.pmp_allows(paddr, 2, Access::Fetch) {
            return Err(Exception::InstructionAccessFault(vaddr));
        }
        self.bus
            .read(paddr, 2)
            .map(|v| v as u16)
            .map_err(|_| Exception::InstructionAccessFault(vaddr))
    }

    /// Runs an atomic access: checks the alignment, translates the address
    /// as a store, because every AMO writes, and gives the physical address.
    pub(crate) fn atomic_address(&mut self, vaddr: u64, width: usize) -> Result<u64, Exception> {
        if !vaddr.is_multiple_of(width as u64) {
            return Err(Exception::StoreAddressMisaligned(vaddr));
        }
        let paddr = self.translate(vaddr, Access::Store)?;
        if !self.pmp_allows(paddr, width, Access::Store) {
            return Err(Exception::StoreAccessFault(vaddr));
        }
        Ok(paddr)
    }

    /// The privilege mode that a load or a store uses: the mode in MPP when
    /// `mstatus.MPRV` is set in M-mode, and the current mode otherwise.
    pub(crate) fn data_mode(&self) -> Mode {
        if self.mode == Mode::Machine && self.csr.mstatus & MSTATUS_MPRV != 0 {
            self.csr.mpp()
        } else {
            self.mode
        }
    }

    /// Asks the PMP if an access to a physical address is allowed.
    pub(crate) fn pmp_allows(&self, paddr: u64, width: usize, access: Access) -> bool {
        let mode = match access {
            Access::Fetch => self.mode,
            _ => self.data_mode(),
        };
        self.csr.pmp.allows(paddr, width as u64, access, mode)
    }

    /// Reads `width` bytes at a virtual address.
    pub(crate) fn load(&mut self, vaddr: u64, width: usize) -> Result<u64, Exception> {
        if crosses_page(vaddr, width) {
            return self.load_across_pages(vaddr, width);
        }
        let paddr = self.translate(vaddr, Access::Load)?;
        if !self.pmp_allows(paddr, width, Access::Load) {
            return Err(Exception::LoadAccessFault(vaddr));
        }
        self.bus
            .read(paddr, width)
            .map_err(|_| Exception::LoadAccessFault(vaddr))
    }

    /// Writes `width` bytes at a virtual address.
    pub(crate) fn store(&mut self, vaddr: u64, width: usize, value: u64) -> Result<(), Exception> {
        if crosses_page(vaddr, width) {
            return self.store_across_pages(vaddr, width, value);
        }
        let paddr = self.translate(vaddr, Access::Store)?;
        if !self.pmp_allows(paddr, width, Access::Store) {
            return Err(Exception::StoreAccessFault(vaddr));
        }
        self.bus
            .write(paddr, width, value)
            .map_err(|_| Exception::StoreAccessFault(vaddr))
    }

    /// Reads a misaligned value that starts on one page and ends on the
    /// next. Both pages are translated before any byte is read.
    fn load_across_pages(&mut self, vaddr: u64, width: usize) -> Result<u64, Exception> {
        let paddrs = self.translate_bytes(vaddr, width, Access::Load)?;
        let mut value = 0;
        for (n, paddr) in paddrs.into_iter().enumerate() {
            let byte = self
                .bus
                .read(paddr, 1)
                .map_err(|_| Exception::LoadAccessFault(vaddr))?;
            value |= byte << (8 * n);
        }
        Ok(value)
    }

    /// Writes a misaligned value that starts on one page and ends on the
    /// next. Both pages are translated before any byte is written, so a
    /// fault on the second page writes nothing.
    fn store_across_pages(
        &mut self,
        vaddr: u64,
        width: usize,
        value: u64,
    ) -> Result<(), Exception> {
        let paddrs = self.translate_bytes(vaddr, width, Access::Store)?;
        for (n, paddr) in paddrs.into_iter().enumerate() {
            self.bus
                .write(paddr, 1, value >> (8 * n))
                .map_err(|_| Exception::StoreAccessFault(vaddr))?;
        }
        Ok(())
    }

    /// Gives the physical address of each byte of an access.
    fn translate_bytes(
        &mut self,
        vaddr: u64,
        width: usize,
        access: Access,
    ) -> Result<Vec<u64>, Exception> {
        let fault = |a: u64| match access {
            Access::Load => Exception::LoadAccessFault(a),
            _ => Exception::StoreAccessFault(a),
        };
        let first = self.translate(vaddr, access)?;
        let second_v = (vaddr | 0xfff) + 1;
        let second = self.translate(second_v, access)?;
        let split = (second_v - vaddr) as usize;
        if !self.pmp_allows(first, split, access) {
            return Err(fault(vaddr));
        }
        if !self.pmp_allows(second, width - split, access) {
            return Err(fault(second_v));
        }
        Ok((0..width)
            .map(|n| {
                if n < split {
                    first + n as u64
                } else {
                    second + (n - split) as u64
                }
            })
            .collect())
    }

    pub(crate) fn reg(&self, r: u8) -> u64 {
        self.x[r as usize]
    }

    pub(crate) fn set_reg(&mut self, r: u8, value: u64) {
        if r != 0 {
            self.x[r as usize] = value;
        }
    }

    /// The context that a CSR access needs.
    pub(crate) fn csr_context(&self) -> CsrContext {
        CsrContext {
            mode: self.mode,
            cycle: self.cycle,
            instret: self.instret,
            time: 0,
        }
    }

    /// Finds the interrupt to take now, if any.
    ///
    /// An interrupt goes to S-mode when `mideleg` delegates it, and to
    /// M-mode otherwise. An interrupt for a higher mode than the current one
    /// is always enabled. An interrupt for the current mode needs the global
    /// enable bit of that mode. An interrupt for a lower mode never fires.
    fn pending_interrupt(&self) -> Option<Interrupt> {
        let pending = self.csr.mip() & self.csr.mie;
        if pending == 0 {
            return None;
        }
        let m_enabled = self.mode < Mode::Machine || self.csr.mstatus & MSTATUS_MIE != 0;
        let s_enabled = self.mode < Mode::Supervisor
            || (self.mode == Mode::Supervisor && self.csr.mstatus & MSTATUS_SIE != 0);
        Interrupt::PRIORITY.into_iter().find(|irq| {
            if pending & irq.bit() == 0 {
                return false;
            }
            if self.csr.mideleg & irq.bit() != 0 {
                s_enabled
            } else {
                m_enabled
            }
        })
    }

    /// Takes a trap: saves the state, and jumps to the handler of M-mode or
    /// of S-mode.
    fn take_trap(&mut self, trap: Trap) {
        // A trap ends the reservation, so an `sc` after a context switch
        // fails.
        self.reservation = None;
        let (interrupt, code, tval) = match trap {
            Trap::Exception(e) => (false, e.code(), e.tval()),
            Trap::Interrupt(i) => (true, i as u64, 0),
        };
        let cause = (u64::from(interrupt) << 63) | code;
        let deleg = if interrupt {
            self.csr.mideleg
        } else {
            self.csr.medeleg
        };
        let to_supervisor = self.mode <= Mode::Supervisor && (deleg >> code) & 1 == 1;

        // In the vectored mode, an interrupt jumps to base + 4 * cause.
        let target = |tvec: u64| {
            let base = tvec & !3;
            if tvec & 3 == 1 && interrupt {
                base + 4 * code
            } else {
                base
            }
        };

        let s = &mut self.csr;
        if to_supervisor {
            s.sepc = self.pc;
            s.scause = cause;
            s.stval = tval;
            let sie = s.mstatus & MSTATUS_SIE != 0;
            s.mstatus = set_bit(s.mstatus, MSTATUS_SPIE, sie);
            s.mstatus &= !MSTATUS_SIE;
            s.mstatus = set_bit(s.mstatus, MSTATUS_SPP, self.mode == Mode::Supervisor);
            self.pc = target(s.stvec);
            self.mode = Mode::Supervisor;
        } else {
            s.mepc = self.pc;
            s.mcause = cause;
            s.mtval = tval;
            let mie = s.mstatus & MSTATUS_MIE != 0;
            s.mstatus = set_bit(s.mstatus, MSTATUS_MPIE, mie);
            s.mstatus &= !MSTATUS_MIE;
            s.mstatus = (s.mstatus & !MSTATUS_MPP) | ((self.mode as u64) << MSTATUS_MPP_SHIFT);
            self.pc = target(s.mtvec);
            self.mode = Mode::Machine;
        }
    }
}

/// Tells if an access of `width` bytes at `vaddr` ends on the next page.
fn crosses_page(vaddr: u64, width: usize) -> bool {
    (vaddr & 0xfff) + width as u64 > 0x1000
}

/// Sets or clears the bits of `mask` in `value`.
fn set_bit(value: u64, mask: u64, on: bool) -> u64 {
    if on { value | mask } else { value & !mask }
}

#[cfg(test)]
mod tests;
