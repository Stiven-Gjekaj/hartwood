//! Runs the instructions of the F and D extensions.
//!
//! The 32 registers `f0` to `f31` are 64 bits wide. A single precision value
//! lives in the low 32 bits, and the high 32 bits are all ones ("NaN
//! boxing"). A single precision operation that reads a register with any
//! other high bits reads the canonical NaN.

use super::Cpu;
use crate::csr::MSTATUS_FS;
use crate::decode::{Fp, Instruction, Op};
use crate::softfloat::{self, F32, F64, Format, Rm};
use crate::trap::Exception;

const BOX: u64 = 0xffff_ffff_0000_0000;

fn format(fp: Fp) -> Format {
    match fp {
        Fp::S => F32,
        Fp::D => F64,
    }
}

impl Cpu {
    /// Reads `f<r>` as a value of the format `fp`.
    fn read_f(&self, r: u8, fp: Fp) -> u64 {
        let v = self.f[r as usize];
        match fp {
            Fp::D => v,
            Fp::S if v & BOX == BOX => v & 0xffff_ffff,
            Fp::S => F32.canonical_nan(),
        }
    }

    /// Writes a value of the format `fp` to `f<r>`, and marks the state of
    /// the FPU as dirty.
    fn write_f(&mut self, r: u8, fp: Fp, value: u64) {
        self.f[r as usize] = match fp {
            Fp::D => value,
            Fp::S => value | BOX,
        };
        self.csr.make_fs_dirty();
    }

    /// Adds exception flags to `fflags`.
    fn raise(&mut self, flags: u8) {
        if flags != 0 {
            self.csr.fflags |= u64::from(flags);
            self.csr.make_fs_dirty();
        }
    }

    /// The rounding mode of an instruction. The value 7 means the mode in
    /// `frm`. A reserved mode is an illegal instruction.
    fn rounding_mode(&self, i: &Instruction) -> Result<Rm, Exception> {
        let bits = if i.rm == 7 {
            self.csr.frm
        } else {
            u64::from(i.rm)
        };
        Rm::from_bits(bits).ok_or(Exception::IllegalInstruction(u64::from(i.raw)))
    }

    /// Runs one floating point instruction.
    pub(crate) fn execute_fp(&mut self, i: &Instruction) -> Result<(), Exception> {
        // With mstatus.FS off, each floating point instruction traps.
        if self.csr.mstatus & MSTATUS_FS == 0 {
            return Err(Exception::IllegalInstruction(u64::from(i.raw)));
        }
        let addr = self.reg(i.rs1).wrapping_add(i.imm as u64);
        match i.op {
            Op::Flw => {
                let v = self.load(addr, 4)?;
                self.write_f(i.rd, Fp::S, v);
            }
            Op::Fld => {
                let v = self.load(addr, 8)?;
                self.write_f(i.rd, Fp::D, v);
            }
            // A store writes the bits as they are, boxed or not.
            Op::Fsw => self.store(addr, 4, self.f[i.rs2 as usize])?,
            Op::Fsd => self.store(addr, 8, self.f[i.rs2 as usize])?,

            Op::Fadd(fp) | Op::Fsub(fp) | Op::Fmul(fp) | Op::Fdiv(fp) => {
                let rm = self.rounding_mode(i)?;
                let (f, a, b) = (format(fp), self.read_f(i.rs1, fp), self.read_f(i.rs2, fp));
                let (v, flags) = match i.op {
                    Op::Fadd(_) => softfloat::add(f, a, b, rm),
                    Op::Fsub(_) => softfloat::sub(f, a, b, rm),
                    Op::Fmul(_) => softfloat::mul(f, a, b, rm),
                    _ => softfloat::div(f, a, b, rm),
                };
                self.write_f(i.rd, fp, v);
                self.raise(flags);
            }
            Op::Fsqrt(fp) => {
                let rm = self.rounding_mode(i)?;
                let (v, flags) = softfloat::sqrt(format(fp), self.read_f(i.rs1, fp), rm);
                self.write_f(i.rd, fp, v);
                self.raise(flags);
            }
            Op::Fmadd(fp) | Op::Fmsub(fp) | Op::Fnmsub(fp) | Op::Fnmadd(fp) => {
                let rm = self.rounding_mode(i)?;
                let (neg_product, neg_addend) = match i.op {
                    Op::Fmadd(_) => (false, false),
                    Op::Fmsub(_) => (false, true),
                    Op::Fnmsub(_) => (true, false),
                    _ => (true, true),
                };
                let (a, b, c) = (
                    self.read_f(i.rs1, fp),
                    self.read_f(i.rs2, fp),
                    self.read_f(i.rs3, fp),
                );
                let (v, flags) = softfloat::fma(format(fp), a, b, c, neg_product, neg_addend, rm);
                self.write_f(i.rd, fp, v);
                self.raise(flags);
            }

            Op::Fsgnj(fp) | Op::Fsgnjn(fp) | Op::Fsgnjx(fp) => {
                let sign = match fp {
                    Fp::S => 1u64 << 31,
                    Fp::D => 1u64 << 63,
                };
                let (a, b) = (self.read_f(i.rs1, fp), self.read_f(i.rs2, fp));
                let new_sign = match i.op {
                    Op::Fsgnj(_) => b & sign,
                    Op::Fsgnjn(_) => !b & sign,
                    _ => (a ^ b) & sign,
                };
                self.write_f(i.rd, fp, (a & !sign) | new_sign);
            }
            Op::Fmin(fp) | Op::Fmax(fp) => {
                let (f, a, b) = (format(fp), self.read_f(i.rs1, fp), self.read_f(i.rs2, fp));
                let (v, flags) = if matches!(i.op, Op::Fmin(_)) {
                    softfloat::min(f, a, b)
                } else {
                    softfloat::max(f, a, b)
                };
                self.write_f(i.rd, fp, v);
                self.raise(flags);
            }

            Op::FcvtW(fp) | Op::FcvtWu(fp) | Op::FcvtL(fp) | Op::FcvtLu(fp) => {
                let rm = self.rounding_mode(i)?;
                let (width, signed) = match i.op {
                    Op::FcvtW(_) => (32, true),
                    Op::FcvtWu(_) => (32, false),
                    Op::FcvtL(_) => (64, true),
                    _ => (64, false),
                };
                let a = self.read_f(i.rs1, fp);
                let (v, flags) = softfloat::to_int(format(fp), a, width, signed, rm);
                self.set_reg(i.rd, v);
                self.raise(flags);
            }
            Op::FcvtFromW(fp) | Op::FcvtFromWu(fp) | Op::FcvtFromL(fp) | Op::FcvtFromLu(fp) => {
                let rm = self.rounding_mode(i)?;
                let x = self.reg(i.rs1);
                let (x, signed) = match i.op {
                    Op::FcvtFromW(_) => (x as i32 as i64 as u64, true),
                    Op::FcvtFromWu(_) => (x & 0xffff_ffff, false),
                    Op::FcvtFromL(_) => (x, true),
                    _ => (x, false),
                };
                let (v, flags) = softfloat::from_int(format(fp), x, signed, rm);
                self.write_f(i.rd, fp, v);
                self.raise(flags);
            }
            Op::FcvtSD => {
                let rm = self.rounding_mode(i)?;
                let (v, flags) = softfloat::convert(F64, F32, self.read_f(i.rs1, Fp::D), rm);
                self.write_f(i.rd, Fp::S, v);
                self.raise(flags);
            }
            Op::FcvtDS => {
                let rm = self.rounding_mode(i)?;
                let (v, flags) = softfloat::convert(F32, F64, self.read_f(i.rs1, Fp::S), rm);
                self.write_f(i.rd, Fp::D, v);
                self.raise(flags);
            }

            // The moves copy bits, with no check of the box.
            Op::FmvToInt(Fp::S) => {
                let v = self.f[i.rs1 as usize] as i32 as i64 as u64;
                self.set_reg(i.rd, v);
            }
            Op::FmvToInt(Fp::D) => self.set_reg(i.rd, self.f[i.rs1 as usize]),
            Op::FmvFromInt(fp) => {
                let x = self.reg(i.rs1);
                let v = if fp == Fp::S { x & 0xffff_ffff } else { x };
                self.write_f(i.rd, fp, v);
            }
            Op::Fclass(fp) => {
                let v = softfloat::classify(format(fp), self.read_f(i.rs1, fp));
                self.set_reg(i.rd, v);
            }
            Op::Feq(fp) | Op::Flt(fp) | Op::Fle(fp) => {
                let (f, a, b) = (format(fp), self.read_f(i.rs1, fp), self.read_f(i.rs2, fp));
                let (v, flags) = match i.op {
                    Op::Feq(_) => softfloat::eq(f, a, b),
                    Op::Flt(_) => softfloat::lt(f, a, b),
                    _ => softfloat::le(f, a, b),
                };
                self.set_reg(i.rd, v);
                self.raise(flags);
            }
            _ => return Err(Exception::IllegalInstruction(u64::from(i.raw))),
        }
        Ok(())
    }
}
