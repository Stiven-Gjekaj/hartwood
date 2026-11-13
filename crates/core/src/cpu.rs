//! The hart: its registers, and the loop that fetches, decodes and runs one
//! instruction at a time.

use crate::bus::{Bus, BusFault};
use crate::decode::{Instruction, Op, decode};
use crate::elf::Elf;
use crate::trap::Exception;

/// Why [`Cpu::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The program wrote an exit code to `tohost`.
    Exit(u64),
    /// The CPU ran the number of instructions that the caller allowed.
    StepLimit,
    /// An exception that the CPU cannot take yet.
    Exception(Exception),
}

/// One RISC-V hart and the bus that it owns.
pub struct Cpu {
    /// The integer registers. `x[0]` is always zero.
    pub x: [u64; 32],
    /// The address of the next instruction.
    pub pc: u64,
    /// The number of instructions that retired.
    pub instret: u64,
    pub bus: Bus,
}

impl Cpu {
    /// Makes a hart that owns `bus`. The program counter starts at zero.
    pub fn new(bus: Bus) -> Self {
        Self {
            x: [0; 32],
            pc: 0,
            instret: 0,
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
            if let Err(e) = self.step() {
                return Stop::Exception(e);
            }
            if let Some(code) = self.bus.htif.exit_code {
                return Stop::Exit(code);
            }
        }
        Stop::StepLimit
    }

    /// Fetches, decodes and runs one instruction.
    pub fn step(&mut self) -> Result<(), Exception> {
        let raw = self.fetch()?;
        let inst = decode(raw);
        self.execute(&inst)?;
        self.instret += 1;
        Ok(())
    }

    fn fetch(&mut self) -> Result<u32, Exception> {
        let pc = self.pc;
        self.bus
            .read(pc, 4)
            .map(|v| v as u32)
            .map_err(|_| Exception::InstructionAccessFault(pc))
    }

    fn reg(&self, r: u8) -> u64 {
        self.x[r as usize]
    }

    fn set_reg(&mut self, r: u8, value: u64) {
        if r != 0 {
            self.x[r as usize] = value;
        }
    }

    fn store(&mut self, addr: u64, width: usize, value: u64) -> Result<(), Exception> {
        self.bus
            .write(addr, width, value)
            .map_err(|_| Exception::StoreAccessFault(addr))
    }

    fn execute(&mut self, i: &Instruction) -> Result<(), Exception> {
        let pc = self.pc;
        let mut next_pc = pc.wrapping_add(u64::from(i.len));
        let rs1 = self.reg(i.rs1);
        let rs2 = self.reg(i.rs2);
        let imm = i.imm as u64;
        match i.op {
            Op::Lui => self.set_reg(i.rd, imm),
            Op::Auipc => self.set_reg(i.rd, pc.wrapping_add(imm)),
            Op::Jal => {
                self.set_reg(i.rd, next_pc);
                next_pc = pc.wrapping_add(imm);
            }
            Op::Addi => self.set_reg(i.rd, rs1.wrapping_add(imm)),
            Op::Sw => self.store(rs1.wrapping_add(imm), 4, rs2)?,
            Op::Sd => self.store(rs1.wrapping_add(imm), 8, rs2)?,
            Op::Illegal => return Err(Exception::IllegalInstruction(u64::from(i.raw))),
        }
        self.pc = next_pc;
        Ok(())
    }
}
