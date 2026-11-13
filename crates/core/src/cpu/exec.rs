//! Runs one decoded instruction.

use super::{Access, Cpu, set_bit};
use crate::csr::{
    MCYCLE, MINSTRET, MSTATUS_MIE, MSTATUS_MPIE, MSTATUS_MPP, MSTATUS_MPRV, MSTATUS_SIE,
    MSTATUS_SPIE, MSTATUS_SPP, MSTATUS_TSR, MSTATUS_TVM, MSTATUS_TW, Mode,
};
use crate::decode::{Instruction, Op};
use crate::trap::Exception;

/// Sign extends the low 32 bits of a value, as each `w` instruction does.
fn sext_w(value: u64) -> u64 {
    value as i32 as i64 as u64
}

/// Signed division as the M extension defines it. A division by zero gives
/// -1. The one overflow, the most negative value divided by -1, gives the
/// most negative value. Neither traps.
fn div(a: i64, b: i64) -> i64 {
    if b == 0 { -1 } else { a.wrapping_div(b) }
}

/// The remainder that goes with [`div`]. A division by zero gives the
/// dividend. The overflow gives zero.
fn rem(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { a.wrapping_rem(b) }
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

            Op::Mul => self.set_reg(i.rd, rs1.wrapping_mul(rs2)),
            Op::Mulh => {
                let v = (i128::from(rs1 as i64) * i128::from(rs2 as i64)) >> 64;
                self.set_reg(i.rd, v as u64);
            }
            Op::Mulhsu => {
                let v = (i128::from(rs1 as i64) * i128::from(rs2)) >> 64;
                self.set_reg(i.rd, v as u64);
            }
            Op::Mulhu => {
                let v = (u128::from(rs1) * u128::from(rs2)) >> 64;
                self.set_reg(i.rd, v as u64);
            }
            Op::Div => self.set_reg(i.rd, div(rs1 as i64, rs2 as i64) as u64),
            Op::Divu => self.set_reg(i.rd, rs1.checked_div(rs2).unwrap_or(u64::MAX)),
            Op::Rem => self.set_reg(i.rd, rem(rs1 as i64, rs2 as i64) as u64),
            Op::Remu => self.set_reg(i.rd, rs1.checked_rem(rs2).unwrap_or(rs1)),
            Op::Mulw => self.set_reg(i.rd, sext_w(rs1.wrapping_mul(rs2))),
            Op::Divw => {
                let v = div(i64::from(rs1 as i32), i64::from(rs2 as i32));
                self.set_reg(i.rd, sext_w(v as u64));
            }
            Op::Divuw => {
                let (a, b) = (rs1 as u32, rs2 as u32);
                self.set_reg(
                    i.rd,
                    sext_w(u64::from(a.checked_div(b).unwrap_or(u32::MAX))),
                );
            }
            Op::Remw => {
                let v = rem(i64::from(rs1 as i32), i64::from(rs2 as i32));
                self.set_reg(i.rd, sext_w(v as u64));
            }
            Op::Remuw => {
                let (a, b) = (rs1 as u32, rs2 as u32);
                self.set_reg(i.rd, sext_w(u64::from(a.checked_rem(b).unwrap_or(a))));
            }

            Op::LrW | Op::LrD => {
                let width = if i.op == Op::LrW { 4 } else { 8 };
                if !rs1.is_multiple_of(width as u64) {
                    return Err(Exception::LoadAddressMisaligned(rs1));
                }
                let paddr = self.translate(rs1, Access::Load)?;
                if !self.pmp_allows(paddr, width, Access::Load) {
                    return Err(Exception::LoadAccessFault(rs1));
                }
                let v = self
                    .bus
                    .read(paddr, width)
                    .map_err(|_| Exception::LoadAccessFault(rs1))?;
                self.reservation = Some(paddr);
                self.set_reg(i.rd, if width == 4 { sext_w(v) } else { v });
            }
            Op::ScW | Op::ScD => {
                let width = if i.op == Op::ScW { 4 } else { 8 };
                let paddr = self.atomic_address(rs1, width)?;
                let ok = self.reservation.take() == Some(paddr);
                if ok {
                    self.bus
                        .write(paddr, width, rs2)
                        .map_err(|_| Exception::StoreAccessFault(rs1))?;
                }
                self.set_reg(i.rd, u64::from(!ok));
            }
            Op::AmoswapW
            | Op::AmoaddW
            | Op::AmoxorW
            | Op::AmoandW
            | Op::AmoorW
            | Op::AmominW
            | Op::AmomaxW
            | Op::AmominuW
            | Op::AmomaxuW
            | Op::AmoswapD
            | Op::AmoaddD
            | Op::AmoxorD
            | Op::AmoandD
            | Op::AmoorD
            | Op::AmominD
            | Op::AmomaxD
            | Op::AmominuD
            | Op::AmomaxuD => self.execute_amo(i.op, i.rd, rs1, rs2)?,

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

    /// Runs one AMO: reads the value at `addr`, writes the result of the
    /// operation on it and `rs2`, and puts the old value in `rd`.
    fn execute_amo(&mut self, op: Op, rd: u8, addr: u64, rs2: u64) -> Result<(), Exception> {
        use Op::*;
        let word = matches!(
            op,
            AmoswapW
                | AmoaddW
                | AmoxorW
                | AmoandW
                | AmoorW
                | AmominW
                | AmomaxW
                | AmominuW
                | AmomaxuW
        );
        let width = if word { 4 } else { 8 };
        let paddr = self.atomic_address(addr, width)?;
        let fault = |_| Exception::StoreAccessFault(addr);
        let raw = self.bus.read(paddr, width).map_err(fault)?;
        // A word works on the sign extended value, so that the signed
        // minimum and maximum compare the right numbers.
        let (old, b) = if word {
            (sext_w(raw), sext_w(rs2))
        } else {
            (raw, rs2)
        };
        let new = match op {
            AmoswapW | AmoswapD => b,
            AmoaddW | AmoaddD => old.wrapping_add(b),
            AmoxorW | AmoxorD => old ^ b,
            AmoandW | AmoandD => old & b,
            AmoorW | AmoorD => old | b,
            AmominW | AmominD => (old as i64).min(b as i64) as u64,
            AmomaxW | AmomaxD => (old as i64).max(b as i64) as u64,
            AmominuW => u64::from((old as u32).min(b as u32)),
            AmomaxuW => u64::from((old as u32).max(b as u32)),
            AmominuD => old.min(b),
            _ => old.max(b),
        };
        self.bus.write(paddr, width, new).map_err(fault)?;
        self.set_reg(rd, old);
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
            // A write to a counter sets the value that the next instruction
            // reads, so the count of this instruction is taken off here. The
            // step adds it back.
            match addr {
                MCYCLE => self.cycle = new.wrapping_sub(1),
                MINSTRET => self.instret = new.wrapping_sub(1),
                _ => {}
            }
        }
        self.set_reg(i.rd, old);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_division_by_zero_does_not_trap() {
        assert_eq!(div(7, 0), -1);
        assert_eq!(rem(7, 0), 7);
    }

    #[test]
    fn the_overflow_of_a_division_gives_the_dividend() {
        assert_eq!(div(i64::MIN, -1), i64::MIN);
        assert_eq!(rem(i64::MIN, -1), 0);
        // The word forms use the same functions on 32-bit values.
        assert_eq!(div(i64::from(i32::MIN), -1) as i32, i32::MIN);
    }

    #[test]
    fn a_division_rounds_toward_zero() {
        assert_eq!(div(-7, 2), -3);
        assert_eq!(rem(-7, 2), -1);
    }
}
