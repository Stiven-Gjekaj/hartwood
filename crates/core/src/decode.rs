//! The instruction decoder.
//!
//! The decoder turns the bits of one instruction into an [`Instruction`]: an
//! operation and its operands, with the immediate already sign extended. The
//! CPU then runs it with one `match` on the operation. The decoder does all
//! the bit work, so the code that runs an instruction reads like the
//! specification.

/// An operation. Each name is the mnemonic in the specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    // RV64I
    Lui,
    Auipc,
    Jal,
    Addi,
    Sw,
    Sd,

    /// An encoding that names no operation that hartwood implements.
    Illegal,
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub op: Op,
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    /// The immediate, sign extended to 64 bits.
    pub imm: i64,
    /// The bits of the instruction, for `mtval` and for the disassembler.
    pub raw: u32,
    /// The length in bytes: 4, or 2 for a compressed instruction.
    pub len: u8,
}

impl Instruction {
    fn new(op: Op, raw: u32) -> Self {
        Self {
            op,
            rd: rd(raw),
            rs1: rs1(raw),
            rs2: rs2(raw),
            imm: 0,
            raw,
            len: 4,
        }
    }

    fn with_imm(mut self, imm: i64) -> Self {
        self.imm = imm;
        self
    }
}

fn rd(raw: u32) -> u8 {
    ((raw >> 7) & 0x1f) as u8
}

fn rs1(raw: u32) -> u8 {
    ((raw >> 15) & 0x1f) as u8
}

fn rs2(raw: u32) -> u8 {
    ((raw >> 20) & 0x1f) as u8
}

fn funct3(raw: u32) -> u32 {
    (raw >> 12) & 0x7
}

/// The I-type immediate: bits 31..20.
fn imm_i(raw: u32) -> i64 {
    i64::from((raw as i32) >> 20)
}

/// The S-type immediate: bits 31..25 and 11..7.
fn imm_s(raw: u32) -> i64 {
    i64::from(((raw & 0xfe00_0000) as i32 >> 20) | ((raw >> 7) & 0x1f) as i32)
}

/// The U-type immediate: bits 31..12, in place.
fn imm_u(raw: u32) -> i64 {
    i64::from((raw & 0xffff_f000) as i32)
}

/// The J-type immediate: a multiple of two, from bits 31..12 in a mixed
/// order.
fn imm_j(raw: u32) -> i64 {
    let imm = ((raw & 0x8000_0000) as i32 >> 11) as u32
        | (raw & 0x000f_f000)
        | ((raw >> 9) & 0x800)
        | ((raw >> 20) & 0x7fe);
    i64::from(imm as i32)
}

/// Decodes one 32-bit instruction.
pub fn decode(raw: u32) -> Instruction {
    let illegal = Instruction::new(Op::Illegal, raw);
    let opcode = raw & 0x7f;
    match opcode {
        0b011_0111 => Instruction::new(Op::Lui, raw).with_imm(imm_u(raw)),
        0b001_0111 => Instruction::new(Op::Auipc, raw).with_imm(imm_u(raw)),
        0b110_1111 => Instruction::new(Op::Jal, raw).with_imm(imm_j(raw)),
        0b010_0011 => {
            let op = match funct3(raw) {
                0b010 => Op::Sw,
                0b011 => Op::Sd,
                _ => return illegal,
            };
            Instruction::new(op, raw).with_imm(imm_s(raw))
        }
        0b001_0011 => match funct3(raw) {
            0b000 => Instruction::new(Op::Addi, raw).with_imm(imm_i(raw)),
            _ => illegal,
        },
        _ => illegal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addi_has_a_negative_immediate() {
        // addi a0, a1, -1
        let i = decode(0xfff5_8513);
        assert_eq!(i.op, Op::Addi);
        assert_eq!((i.rd, i.rs1, i.imm), (10, 11, -1));
    }

    #[test]
    fn sd_joins_its_two_immediate_fields() {
        // sd a0, -8(sp)
        let i = decode(0xfea1_3c23);
        assert_eq!(i.op, Op::Sd);
        assert_eq!((i.rs1, i.rs2, i.imm), (2, 10, -8));
    }

    #[test]
    fn jal_decodes_a_backward_jump() {
        // jal zero, -4
        let i = decode(0xffdf_f06f);
        assert_eq!(i.op, Op::Jal);
        assert_eq!((i.rd, i.imm), (0, -4));
        // jal ra, 2048
        let i = decode(0x0010_00ef);
        assert_eq!((i.rd, i.imm), (1, 2048));
    }

    #[test]
    fn lui_keeps_the_upper_bits_and_sign_extends() {
        // lui a0, 0x80000
        let i = decode(0x8000_0537);
        assert_eq!(i.op, Op::Lui);
        assert_eq!(i.imm, -0x8000_0000);
    }

    #[test]
    fn an_unknown_opcode_is_illegal() {
        assert_eq!(decode(0).op, Op::Illegal);
        assert_eq!(decode(0xffff_ffff).op, Op::Illegal);
    }
}
