//! Runs one decoded instruction.

use super::{Cpu, set_bit};
use crate::csr::{
    MSTATUS_MIE, MSTATUS_MPIE, MSTATUS_MPP, MSTATUS_MPRV, MSTATUS_SIE, MSTATUS_SPIE, MSTATUS_SPP,
    MSTATUS_TSR, MSTATUS_TVM, MSTATUS_TW, Mode,
};
use crate::decode::{Instruction, Op};
use crate::trap::Exception;

/// Sign extends the low 32 bits of a value, as each `w` instruction does.
fn sext_w(value: u64) -> u64 {
    value as i32 as i64 as u64
}

impl Cpu {
    /// Runs one instruction. On success, the program counter moves to the
    /// next instruction or to the target of a jump. On an exception, the
    /// program counter and the registers do not change.
    pub(crate) fn execute(&mut self, i: &Instruction) -> Result<(), Exception> {
        let pc = self.pc;
        let mut next_pc = pc.wrapping_add(u64::from(i.len));
        let rs1 = self.reg(i.rs1);
        let rs2 = self.reg(i.rs2);
        let imm = i.imm as u64;
        let illegal = Exception::IllegalInstruction(u64::from(i.raw));

        match i.op {
            Op::Lui => self.set_reg(i.rd, imm),
            Op::Auipc => self.set_reg(i.rd, pc.wrapping_add(imm)),
            Op::Jal => {
                self.set_reg(i.rd, next_pc);
                next_pc = pc.wrapping_add(imm);
            }
            Op::Jalr => {
                // Read rs1 before the write of rd, because the two can be
                // the same register.
                let target = rs1.wrapping_add(imm) & !1;
                self.set_reg(i.rd, next_pc);
                next_pc = target;
            }
            Op::Beq | Op::Bne | Op::Blt | Op::Bge | Op::Bltu | Op::Bgeu => {
                let taken = match i.op {
                    Op::Beq => rs1 == rs2,
                    Op::Bne => rs1 != rs2,
                    Op::Blt => (rs1 as i64) < (rs2 as i64),
                    Op::Bge => (rs1 as i64) >= (rs2 as i64),
                    Op::Bltu => rs1 < rs2,
                    _ => rs1 >= rs2,
                };
                if taken {
                    next_pc = pc.wrapping_add(imm);
                }
            }

            Op::Lb => {
                let v = self.load(rs1.wrapping_add(imm), 1)?;
                self.set_reg(i.rd, v as i8 as i64 as u64);
            }
            Op::Lh => {
                let v = self.load(rs1.wrapping_add(imm), 2)?;
                self.set_reg(i.rd, v as i16 as i64 as u64);
            }
            Op::Lw => {
                let v = self.load(rs1.wrapping_add(imm), 4)?;
                self.set_reg(i.rd, sext_w(v));
            }
            Op::Ld => {
                let v = self.load(rs1.wrapping_add(imm), 8)?;
                self.set_reg(i.rd, v);
            }
            Op::Lbu => {
                let v = self.load(rs1.wrapping_add(imm), 1)?;
                self.set_reg(i.rd, v);
            }
            Op::Lhu => {
                let v = self.load(rs1.wrapping_add(imm), 2)?;
                self.set_reg(i.rd, v);
            }
            Op::Lwu => {
                let v = self.load(rs1.wrapping_add(imm), 4)?;
                self.set_reg(i.rd, v);
            }
            Op::Sb => self.store(rs1.wrapping_add(imm), 1, rs2)?,
            Op::Sh => self.store(rs1.wrapping_add(imm), 2, rs2)?,
            Op::Sw => self.store(rs1.wrapping_add(imm), 4, rs2)?,
            Op::Sd => self.store(rs1.wrapping_add(imm), 8, rs2)?,

            Op::Addi => self.set_reg(i.rd, rs1.wrapping_add(imm)),
            Op::Slti => self.set_reg(i.rd, u64::from((rs1 as i64) < i.imm)),
            Op::Sltiu => self.set_reg(i.rd, u64::from(rs1 < imm)),
            Op::Xori => self.set_reg(i.rd, rs1 ^ imm),
            Op::Ori => self.set_reg(i.rd, rs1 | imm),
            Op::Andi => self.set_reg(i.rd, rs1 & imm),
            Op::Slli => self.set_reg(i.rd, rs1 << (imm & 0x3f)),
            Op::Srli => self.set_reg(i.rd, rs1 >> (imm & 0x3f)),
            Op::Srai => self.set_reg(i.rd, ((rs1 as i64) >> (imm & 0x3f)) as u64),
            Op::Add => self.set_reg(i.rd, rs1.wrapping_add(rs2)),
            Op::Sub => self.set_reg(i.rd, rs1.wrapping_sub(rs2)),
            Op::Sll => self.set_reg(i.rd, rs1 << (rs2 & 0x3f)),
            Op::Slt => self.set_reg(i.rd, u64::from((rs1 as i64) < (rs2 as i64))),
            Op::Sltu => self.set_reg(i.rd, u64::from(rs1 < rs2)),
            Op::Xor => self.set_reg(i.rd, rs1 ^ rs2),
            Op::Srl => self.set_reg(i.rd, rs1 >> (rs2 & 0x3f)),
            Op::Sra => self.set_reg(i.rd, ((rs1 as i64) >> (rs2 & 0x3f)) as u64),
            Op::Or => self.set_reg(i.rd, rs1 | rs2),
            Op::And => self.set_reg(i.rd, rs1 & rs2),
            Op::Addiw => self.set_reg(i.rd, sext_w(rs1.wrapping_add(imm))),
            Op::Slliw => self.set_reg(i.rd, sext_w(rs1 << (imm & 0x1f))),
            Op::Srliw => self.set_reg(i.rd, sext_w((rs1 as u32 >> (imm & 0x1f)) as u64)),
            Op::Sraiw => self.set_reg(i.rd, ((rs1 as i32) >> (imm & 0x1f)) as i64 as u64),
            Op::Addw => self.set_reg(i.rd, sext_w(rs1.wrapping_add(rs2))),
            Op::Subw => self.set_reg(i.rd, sext_w(rs1.wrapping_sub(rs2))),
            Op::Sllw => self.set_reg(i.rd, sext_w(rs1 << (rs2 & 0x1f))),
            Op::Srlw => self.set_reg(i.rd, sext_w((rs1 as u32 >> (rs2 & 0x1f)) as u64)),
            Op::Sraw => self.set_reg(i.rd, ((rs1 as i32) >> (rs2 & 0x1f)) as i64 as u64),

            // One hart, and no cache of instructions that can go stale, so
            // both fences have nothing to order.
            Op::Fence | Op::FenceI => {}
            Op::Ecall => {
                return Err(match self.mode {
                    Mode::User => Exception::EnvironmentCallFromU,
                    Mode::Supervisor => Exception::EnvironmentCallFromS,
                    Mode::Machine => Exception::EnvironmentCallFromM,
                });
            }
            Op::Ebreak => return Err(Exception::Breakpoint(pc)),

            Op::Csrrw | Op::Csrrs | Op::Csrrc | Op::Csrrwi | Op::Csrrsi | Op::Csrrci => {
                self.execute_csr(i)?;
            }

            Op::Mret => {
                if self.mode < Mode::Machine {
                    return Err(illegal);
                }
                let s = &mut self.csr;
                let mpp = s.mpp();
                let mpie = s.mstatus & MSTATUS_MPIE != 0;
                s.mstatus = set_bit(s.mstatus, MSTATUS_MIE, mpie);
                s.mstatus |= MSTATUS_MPIE;
                s.mstatus &= !MSTATUS_MPP;
                if mpp != Mode::Machine {
                    s.mstatus &= !MSTATUS_MPRV;
                }
                next_pc = s.mepc;
                self.mode = mpp;
            }
            Op::Sret => {
                let tsr = self.csr.mstatus & MSTATUS_TSR != 0;
                if self.mode < Mode::Supervisor || (self.mode == Mode::Supervisor && tsr) {
                    return Err(illegal);
                }
                let s = &mut self.csr;
                let spp = if s.mstatus & MSTATUS_SPP != 0 {
                    Mode::Supervisor
                } else {
                    Mode::User
                };
                let spie = s.mstatus & MSTATUS_SPIE != 0;
                s.mstatus = set_bit(s.mstatus, MSTATUS_SIE, spie);
                s.mstatus |= MSTATUS_SPIE;
                s.mstatus &= !MSTATUS_SPP;
                s.mstatus &= !MSTATUS_MPRV;
                next_pc = s.sepc;
                self.mode = spp;
            }
            Op::Wfi => {
                // In U-mode, and in S-mode with mstatus.TW set, WFI traps.
                let tw = self.csr.mstatus & MSTATUS_TW != 0;
                if self.mode == Mode::User || (self.mode == Mode::Supervisor && tw) {
                    return Err(illegal);
                }
            }
            Op::SfenceVma => {
                let tvm = self.csr.mstatus & MSTATUS_TVM != 0;
                if self.mode == Mode::User || (self.mode == Mode::Supervisor && tvm) {
                    return Err(illegal);
                }
            }

            Op::Illegal => return Err(illegal),
        }
        self.pc = next_pc;
        Ok(())
    }

    /// Runs one of the six CSR instructions.
    ///
    /// `csrrw` with `rd = x0` does not read the CSR. `csrrs` and `csrrc` with
    /// `rs1 = x0` (or an immediate of zero) do not write it. A read or a
    /// write that does not happen cannot trap.
    fn execute_csr(&mut self, i: &Instruction) -> Result<(), Exception> {
        let addr = i.imm as u16;
        let illegal = Exception::IllegalInstruction(u64::from(i.raw));
        let operand = match i.op {
            Op::Csrrwi | Op::Csrrsi | Op::Csrrci => u64::from(i.rs1),
            _ => self.reg(i.rs1),
        };
        let writes = match i.op {
            Op::Csrrw | Op::Csrrwi => true,
            _ => i.rs1 != 0,
        };
        let ctx = self.csr_context();
        if !self.csr.accessible(addr, &ctx, writes) {
            return Err(illegal);
        }
        let old = self.csr.read(addr, &ctx).ok_or(illegal)?;
        if writes {
            let new = match i.op {
                Op::Csrrw | Op::Csrrwi => operand,
                Op::Csrrs | Op::Csrrsi => old | operand,
                _ => old & !operand,
            };
            self.csr.write(addr, new);
        }
        self.set_reg(i.rd, old);
        Ok(())
    }
}
