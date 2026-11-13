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
    Jalr,
    Beq,
    Bne,
    Blt,
    Bge,
    Bltu,
    Bgeu,
    Lb,
    Lh,
    Lw,
    Ld,
    Lbu,
    Lhu,
    Lwu,
    Sb,
    Sh,
    Sw,
    Sd,
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
    Slli,
    Srli,
    Srai,
    Add,
    Sub,
    Sll,
    Slt,
    Sltu,
    Xor,
    Srl,
    Sra,
    Or,
    And,
    Addiw,
    Slliw,
    Srliw,
    Sraiw,
    Addw,
    Subw,
    Sllw,
    Srlw,
    Sraw,
    Fence,
    Ecall,
    Ebreak,

    // Zifencei
    FenceI,

    // Zicsr. The immediate holds the CSR address. For the forms with `i`,
    // `rs1` holds the 5-bit immediate.
    Csrrw,
    Csrrs,
    Csrrc,
    Csrrwi,
    Csrrsi,
    Csrrci,

    // The privileged instructions.
    Mret,
    Sret,
    Wfi,
    SfenceVma,

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

/// The B-type immediate: a multiple of two, from bits 31..25 and 11..7.
fn imm_b(raw: u32) -> i64 {
    let imm = ((raw & 0x8000_0000) as i32 >> 19) as u32
        | ((raw << 4) & 0x800)
        | ((raw >> 20) & 0x7e0)
        | ((raw >> 7) & 0x1e);
    i64::from(imm as i32)
}

fn funct7(raw: u32) -> u32 {
    raw >> 25
}

/// Decodes one 32-bit instruction.
pub fn decode(raw: u32) -> Instruction {
    let illegal = Instruction::new(Op::Illegal, raw);
    let opcode = raw & 0x7f;
    let f3 = funct3(raw);
    let f7 = funct7(raw);
    match opcode {
        0b011_0111 => Instruction::new(Op::Lui, raw).with_imm(imm_u(raw)),
        0b001_0111 => Instruction::new(Op::Auipc, raw).with_imm(imm_u(raw)),
        0b110_1111 => Instruction::new(Op::Jal, raw).with_imm(imm_j(raw)),
        0b110_0111 if f3 == 0 => Instruction::new(Op::Jalr, raw).with_imm(imm_i(raw)),
        0b110_0011 => {
            let op = match f3 {
                0b000 => Op::Beq,
                0b001 => Op::Bne,
                0b100 => Op::Blt,
                0b101 => Op::Bge,
                0b110 => Op::Bltu,
                0b111 => Op::Bgeu,
                _ => return illegal,
            };
            Instruction::new(op, raw).with_imm(imm_b(raw))
        }
        0b000_0011 => {
            let op = match f3 {
                0b000 => Op::Lb,
                0b001 => Op::Lh,
                0b010 => Op::Lw,
                0b011 => Op::Ld,
                0b100 => Op::Lbu,
                0b101 => Op::Lhu,
                0b110 => Op::Lwu,
                _ => return illegal,
            };
            Instruction::new(op, raw).with_imm(imm_i(raw))
        }
        0b010_0011 => {
            let op = match f3 {
                0b000 => Op::Sb,
                0b001 => Op::Sh,
                0b010 => Op::Sw,
                0b011 => Op::Sd,
                _ => return illegal,
            };
            Instruction::new(op, raw).with_imm(imm_s(raw))
        }
        0b001_0011 => {
            let op = match f3 {
                0b000 => Op::Addi,
                0b010 => Op::Slti,
                0b011 => Op::Sltiu,
                0b100 => Op::Xori,
                0b110 => Op::Ori,
                0b111 => Op::Andi,
                // RV64 shifts by six bits, so bit 25 is part of the amount.
                0b001 if raw >> 26 == 0 => Op::Slli,
                0b101 if raw >> 26 == 0 => Op::Srli,
                0b101 if raw >> 26 == 0b01_0000 => Op::Srai,
                _ => return illegal,
            };
            let imm = match op {
                Op::Slli | Op::Srli | Op::Srai => i64::from((raw >> 20) & 0x3f),
                _ => imm_i(raw),
            };
            Instruction::new(op, raw).with_imm(imm)
        }
        0b011_0011 => {
            let op = match (f7, f3) {
                (0, 0b000) => Op::Add,
                (0b010_0000, 0b000) => Op::Sub,
                (0, 0b001) => Op::Sll,
                (0, 0b010) => Op::Slt,
                (0, 0b011) => Op::Sltu,
                (0, 0b100) => Op::Xor,
                (0, 0b101) => Op::Srl,
                (0b010_0000, 0b101) => Op::Sra,
                (0, 0b110) => Op::Or,
                (0, 0b111) => Op::And,
                _ => return illegal,
            };
            Instruction::new(op, raw)
        }
        0b001_1011 => {
            let op = match (f7, f3) {
                (_, 0b000) => Op::Addiw,
                (0, 0b001) => Op::Slliw,
                (0, 0b101) => Op::Srliw,
                (0b010_0000, 0b101) => Op::Sraiw,
                _ => return illegal,
            };
            let imm = match op {
                Op::Addiw => imm_i(raw),
                _ => i64::from((raw >> 20) & 0x1f),
            };
            Instruction::new(op, raw).with_imm(imm)
        }
        0b011_1011 => {
            let op = match (f7, f3) {
                (0, 0b000) => Op::Addw,
                (0b010_0000, 0b000) => Op::Subw,
                (0, 0b001) => Op::Sllw,
                (0, 0b101) => Op::Srlw,
                (0b010_0000, 0b101) => Op::Sraw,
                _ => return illegal,
            };
            Instruction::new(op, raw)
        }
        0b000_1111 => match f3 {
            0b000 => Instruction::new(Op::Fence, raw),
            0b001 => Instruction::new(Op::FenceI, raw),
            _ => illegal,
        },
        0b111_0011 => decode_system(raw),
        _ => illegal,
    }
}

/// Decodes the SYSTEM opcode: the CSR instructions and the privileged
/// instructions.
fn decode_system(raw: u32) -> Instruction {
    let illegal = Instruction::new(Op::Illegal, raw);
    let csr = i64::from(raw >> 20);
    let op = match funct3(raw) {
        0b001 => Op::Csrrw,
        0b010 => Op::Csrrs,
        0b011 => Op::Csrrc,
        0b101 => Op::Csrrwi,
        0b110 => Op::Csrrsi,
        0b111 => Op::Csrrci,
        0b000 => {
            if rd(raw) != 0 {
                return illegal;
            }
            let op = match (funct7(raw), rs2(raw), rs1(raw)) {
                (0, 0, 0) => Op::Ecall,
                (0, 1, 0) => Op::Ebreak,
                (0b000_1000, 0b00010, 0) => Op::Sret,
                (0b001_1000, 0b00010, 0) => Op::Mret,
                (0b000_1000, 0b00101, 0) => Op::Wfi,
                (0b000_1001, _, _) => Op::SfenceVma,
                _ => return illegal,
            };
            return Instruction::new(op, raw);
        }
        _ => return illegal,
    };
    Instruction::new(op, raw).with_imm(csr)
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
    fn a_branch_has_a_signed_offset() {
        // beq a0, a1, -8
        let i = decode(0xfeb5_0ce3);
        assert_eq!(i.op, Op::Beq);
        assert_eq!((i.rs1, i.rs2, i.imm), (10, 11, -8));
        // bltu t0, t1, 4094
        let i = decode(0x7e62_efe3);
        assert_eq!(i.op, Op::Bltu);
        assert_eq!(i.imm, 4094);
    }

    #[test]
    fn srai_and_srli_use_six_bits_of_shift() {
        // srai a0, a0, 63
        let i = decode(0x43f5_5513);
        assert_eq!((i.op, i.imm), (Op::Srai, 63));
        // srli a0, a0, 32
        let i = decode(0x0205_5513);
        assert_eq!((i.op, i.imm), (Op::Srli, 32));
    }

    #[test]
    fn a_word_shift_with_bit_25_set_is_illegal() {
        // slliw with shamt[5] set is reserved.
        assert_eq!(decode(0x0205_151b).op, Op::Illegal);
        assert_eq!(decode(0x01f5_151b).op, Op::Slliw);
    }

    #[test]
    fn csrrw_holds_the_csr_address() {
        // csrrw a0, mscratch, a1
        let i = decode(0x3405_9573);
        assert_eq!(i.op, Op::Csrrw);
        assert_eq!((i.rd, i.rs1, i.imm), (10, 11, 0x340));
    }

    #[test]
    fn the_privileged_instructions_decode() {
        assert_eq!(decode(0x0000_0073).op, Op::Ecall);
        assert_eq!(decode(0x0010_0073).op, Op::Ebreak);
        assert_eq!(decode(0x3020_0073).op, Op::Mret);
        assert_eq!(decode(0x1020_0073).op, Op::Sret);
        assert_eq!(decode(0x1050_0073).op, Op::Wfi);
        assert_eq!(decode(0x1200_0073).op, Op::SfenceVma);
        assert_eq!(decode(0x0000_100f).op, Op::FenceI);
    }

    #[test]
    fn an_unknown_opcode_is_illegal() {
        assert_eq!(decode(0).op, Op::Illegal);
        assert_eq!(decode(0xffff_ffff).op, Op::Illegal);
    }
}
