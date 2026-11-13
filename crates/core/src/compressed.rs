//! The C extension: each 16-bit instruction becomes its 32-bit form.
//!
//! Every compressed instruction is a short form of a 32-bit instruction, so
//! the expansion is exact. After it, the normal decoder and the normal code
//! run the instruction. Only the length (2 bytes) and the raw bits for
//! `mtval` stay from the short form.

use crate::asm::{b, i, j, r, s, u};

const LOAD: u32 = 0b000_0011;
const LOAD_FP: u32 = 0b000_0111;
const STORE: u32 = 0b010_0011;
const STORE_FP: u32 = 0b010_0111;
const OP_IMM: u32 = 0b001_0011;
const OP_IMM_32: u32 = 0b001_1011;
const OP: u32 = 0b011_0011;
const OP_32: u32 = 0b011_1011;
const LUI: u32 = 0b011_0111;
const JALR: u32 = 0b110_0111;
const EBREAK: u32 = 0x0010_0073;

/// Takes bit `from` of `c` and puts it at bit `to`.
fn bit(c: u32, from: u32, to: u32) -> u32 {
    ((c >> from) & 1) << to
}

/// Takes the bits `hi..=lo` of `c` and puts them at bit `to` and up.
fn bits(c: u32, hi: u32, lo: u32, to: u32) -> u32 {
    ((c >> lo) & ((1 << (hi - lo + 1)) - 1)) << to
}

/// Sign extends the low `n` bits of `v`.
fn sext(v: u32, n: u32) -> i32 {
    ((v << (32 - n)) as i32) >> (32 - n)
}

/// A register field of three bits names x8 to x15.
fn creg(c: u32, lo: u32) -> u32 {
    ((c >> lo) & 7) + 8
}

/// Gives the 32-bit form of a compressed instruction, or `None` if the
/// encoding is reserved or illegal.
pub fn expand(c16: u16) -> Option<u32> {
    let c = u32::from(c16);
    let funct3 = c >> 13;
    let rd = (c >> 7) & 0x1f;
    let rs2 = (c >> 2) & 0x1f;
    // The 6-bit immediate of C.ADDI, C.LI, C.ANDI and others.
    let imm6 = sext(bit(c, 12, 5) | bits(c, 6, 2, 0), 6);
    // The 6-bit shift amount.
    let shamt = (bit(c, 12, 5) | bits(c, 6, 2, 0)) as i32;

    let inst = match (c & 3, funct3) {
        // Quadrant 0.
        (0, 0b000) => {
            // C.ADDI4SPN
            let imm = bits(c, 12, 11, 4) | bits(c, 10, 7, 6) | bit(c, 6, 2) | bit(c, 5, 3);
            if imm == 0 {
                return None;
            }
            i(OP_IMM, 0, creg(c, 2), 2, imm as i32)
        }
        (0, 0b001) => i(LOAD_FP, 0b011, creg(c, 2), creg(c, 7), ld_off(c)), // C.FLD
        (0, 0b010) => i(LOAD, 0b010, creg(c, 2), creg(c, 7), lw_off(c)),    // C.LW
        (0, 0b011) => i(LOAD, 0b011, creg(c, 2), creg(c, 7), ld_off(c)),    // C.LD
        (0, 0b101) => s(STORE_FP, 0b011, creg(c, 7), creg(c, 2), ld_off(c)), // C.FSD
        (0, 0b110) => s(STORE, 0b010, creg(c, 7), creg(c, 2), lw_off(c)),   // C.SW
        (0, 0b111) => s(STORE, 0b011, creg(c, 7), creg(c, 2), ld_off(c)),   // C.SD

        // Quadrant 1.
        (1, 0b000) => i(OP_IMM, 0, rd, rd, imm6), // C.ADDI and C.NOP
        (1, 0b001) => {
            // C.ADDIW
            if rd == 0 {
                return None;
            }
            i(OP_IMM_32, 0, rd, rd, imm6)
        }
        (1, 0b010) => i(OP_IMM, 0, rd, 0, imm6), // C.LI
        (1, 0b011) if rd == 2 => {
            // C.ADDI16SP
            let imm = bit(c, 12, 9) | bit(c, 6, 4) | bit(c, 5, 6) | bits(c, 4, 3, 7) | bit(c, 2, 5);
            if imm == 0 {
                return None;
            }
            i(OP_IMM, 0, 2, 2, sext(imm, 10))
        }
        (1, 0b011) => {
            // C.LUI
            if imm6 == 0 {
                return None;
            }
            u(LUI, rd, (imm6 << 12) as u32)
        }
        (1, 0b100) => {
            let rd = creg(c, 7);
            let rs2 = creg(c, 2);
            match (bits(c, 11, 10, 0), bit(c, 12, 0), bits(c, 6, 5, 0)) {
                (0b00, _, _) => i(OP_IMM, 0b101, rd, rd, shamt), // C.SRLI
                (0b01, _, _) => i(OP_IMM, 0b101, rd, rd, shamt | 0x400), // C.SRAI
                (0b10, _, _) => i(OP_IMM, 0b111, rd, rd, imm6),  // C.ANDI
                (0b11, 0, 0b00) => r(OP, 0b000, 0b010_0000, rd, rd, rs2), // C.SUB
                (0b11, 0, 0b01) => r(OP, 0b100, 0, rd, rd, rs2), // C.XOR
                (0b11, 0, 0b10) => r(OP, 0b110, 0, rd, rd, rs2), // C.OR
                (0b11, 0, 0b11) => r(OP, 0b111, 0, rd, rd, rs2), // C.AND
                (0b11, 1, 0b00) => r(OP_32, 0b000, 0b010_0000, rd, rd, rs2), // C.SUBW
                (0b11, 1, 0b01) => r(OP_32, 0b000, 0, rd, rd, rs2), // C.ADDW
                _ => return None,
            }
        }
        (1, 0b101) => j(0, j_off(c)),                         // C.J
        (1, 0b110) => b(0b000, creg(c, 7), 0, branch_off(c)), // C.BEQZ
        (1, 0b111) => b(0b001, creg(c, 7), 0, branch_off(c)), // C.BNEZ

        // Quadrant 2.
        (2, 0b000) => i(OP_IMM, 0b001, rd, rd, shamt), // C.SLLI
        (2, 0b001) => i(LOAD_FP, 0b011, rd, 2, ldsp_off(c)), // C.FLDSP
        (2, 0b010) => {
            // C.LWSP
            if rd == 0 {
                return None;
            }
            let off = bit(c, 12, 5) | bits(c, 6, 4, 2) | bits(c, 3, 2, 6);
            i(LOAD, 0b010, rd, 2, off as i32)
        }
        (2, 0b011) => {
            // C.LDSP
            if rd == 0 {
                return None;
            }
            i(LOAD, 0b011, rd, 2, ldsp_off(c))
        }
        (2, 0b100) => match (bit(c, 12, 0), rd, rs2) {
            (0, 0, 0) => return None,
            (0, _, 0) => i(JALR, 0, 0, rd, 0),    // C.JR
            (0, _, _) => r(OP, 0, 0, rd, 0, rs2), // C.MV
            (_, 0, 0) => EBREAK,                  // C.EBREAK
            (_, _, 0) => i(JALR, 0, 1, rd, 0),    // C.JALR
            _ => r(OP, 0, 0, rd, rd, rs2),        // C.ADD
        },
        (2, 0b101) => s(STORE_FP, 0b011, 2, rs2, sdsp_off(c)), // C.FSDSP
        (2, 0b110) => {
            // C.SWSP
            let off = bits(c, 12, 9, 2) | bits(c, 8, 7, 6);
            s(STORE, 0b010, 2, rs2, off as i32)
        }
        (2, 0b111) => s(STORE, 0b011, 2, rs2, sdsp_off(c)), // C.SDSP
        _ => return None,
    };
    Some(inst)
}

/// The offset of C.LW and C.SW: bits 5..3 from 12..10, bit 2 from 6, bit 6
/// from 5.
fn lw_off(c: u32) -> i32 {
    (bits(c, 12, 10, 3) | bit(c, 6, 2) | bit(c, 5, 6)) as i32
}

/// The offset of C.LD, C.SD, C.FLD and C.FSD: bits 5..3 from 12..10, bits
/// 7..6 from 6..5.
fn ld_off(c: u32) -> i32 {
    (bits(c, 12, 10, 3) | bits(c, 6, 5, 6)) as i32
}

/// The offset of C.LDSP and C.FLDSP.
fn ldsp_off(c: u32) -> i32 {
    (bit(c, 12, 5) | bits(c, 6, 5, 3) | bits(c, 4, 2, 6)) as i32
}

/// The offset of C.SDSP and C.FSDSP.
fn sdsp_off(c: u32) -> i32 {
    (bits(c, 12, 10, 3) | bits(c, 9, 7, 6)) as i32
}

/// The offset of C.J: a signed 12-bit value in a mixed order.
fn j_off(c: u32) -> i32 {
    let v = bit(c, 12, 11)
        | bit(c, 11, 4)
        | bits(c, 10, 9, 8)
        | bit(c, 8, 10)
        | bit(c, 7, 6)
        | bit(c, 6, 7)
        | bits(c, 5, 3, 1)
        | bit(c, 2, 5);
    sext(v, 12)
}

/// The offset of C.BEQZ and C.BNEZ: a signed 9-bit value.
fn branch_off(c: u32) -> i32 {
    let v = bit(c, 12, 8) | bits(c, 11, 10, 3) | bits(c, 6, 5, 6) | bits(c, 4, 3, 1) | bit(c, 2, 5);
    sext(v, 9)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pair is a compressed instruction and its 32-bit form, both from
    /// `riscv64-elf-as`. The comment names the instruction.
    const PAIRS: &[(u16, u32)] = &[
        (0x0800, 0x0101_0413), // c.addi4spn s0, sp, 16
        (0x6580, 0x0085_b403), // c.ld s0, 8(a1)
        (0x41c8, 0x0045_a503), // c.lw a0, 4(a1)
        (0xe188, 0x00a5_b023), // c.sd a0, 0(a1)
        (0x357d, 0xfff5_051b), // c.addiw a0, -1
        (0x557d, 0xfff0_0513), // c.li a0, -1
        (0x7139, 0xfc01_0113), // c.addi16sp sp, -64
        (0x7579, 0xffff_e537), // c.lui a0, 0xffffe
        (0x9101, 0x0205_5513), // c.srli a0, 32
        (0x9501, 0x4205_5513), // c.srai a0, 32
        (0x8905, 0x0015_7513), // c.andi a0, 1
        (0x8d0d, 0x40b5_0533), // c.sub a0, a1
        (0x9d2d, 0x00b5_053b), // c.addw a0, a1
        (0xa001, 0x0000_006f), // c.j .
        (0xb001, 0x801f_f06f), // c.j .-2048
        (0xaffd, 0x7fe0_006f), // c.j .+2046
        (0xc101, 0x0005_0063), // c.beqz a0, .
        (0xd101, 0xf005_00e3), // c.beqz a0, .-256
        (0xe091, 0x0004_9263), // c.bnez s1, .+4
        (0xecfd, 0x0e04_9f63), // c.bnez s1, .+254
        (0x0506, 0x0015_1513), // c.slli a0, 1
        (0x6522, 0x0081_3503), // c.ldsp a0, 8(sp)
        (0x557e, 0x0fc1_2503), // c.lwsp a0, 252(sp)
        (0x8082, 0x0000_8067), // c.jr ra
        (0x852e, 0x00b0_0533), // c.mv a0, a1
        (0x9502, 0x0005_00e7), // c.jalr a0
        (0x952e, 0x00b5_0533), // c.add a0, a1
        (0x9002, 0x0010_0073), // c.ebreak
        (0xe42a, 0x00a1_3423), // c.sdsp a0, 8(sp)
        (0xc22a, 0x00a1_2223), // c.swsp a0, 4(sp)
        (0x2522, 0x0081_3507), // c.fldsp fa0, 8(sp)
        (0xa588, 0x00a5_b427), // c.fsd fa0, 8(a1)
        (0x0001, 0x0000_0013), // c.nop
    ];

    #[test]
    fn each_form_matches_the_assembler() {
        for &(c, full) in PAIRS {
            assert_eq!(expand(c), Some(full), "{c:#06x}");
        }
    }

    #[test]
    fn the_reserved_encodings_are_illegal() {
        assert_eq!(expand(0x0000), None); // all zeros
        assert_eq!(expand(0x6101), None); // c.addi16sp with zero
        assert_eq!(expand(0x8002), None); // c.jr with rs1 = x0
        assert_eq!(expand(0x4002), None); // c.lwsp with rd = x0
    }
}
