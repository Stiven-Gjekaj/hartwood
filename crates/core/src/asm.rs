//! A small assembler for tests.
//!
//! A test builds its own program from these functions, so it does not need a
//! RISC-V compiler and does not read a file that somebody edits. Each
//! function gives the bits of one instruction in one encoding format.

/// The R-type format: register to register.
pub fn r(opcode: u32, funct3: u32, funct7: u32, rd: u32, rs1: u32, rs2: u32) -> u32 {
    (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

/// The I-type format: a 12-bit immediate.
pub fn i(opcode: u32, funct3: u32, rd: u32, rs1: u32, imm: i32) -> u32 {
    (((imm as u32) & 0xfff) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

/// The S-type format: a store.
pub fn s(opcode: u32, funct3: u32, rs1: u32, rs2: u32, imm: i32) -> u32 {
    let imm = imm as u32;
    (((imm >> 5) & 0x7f) << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (funct3 << 12)
        | ((imm & 0x1f) << 7)
        | opcode
}

/// The B-type format: a branch with an offset that is a multiple of two.
pub fn b(funct3: u32, rs1: u32, rs2: u32, offset: i32) -> u32 {
    let imm = offset as u32;
    (((imm >> 12) & 1) << 31)
        | (((imm >> 5) & 0x3f) << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (funct3 << 12)
        | (((imm >> 1) & 0xf) << 8)
        | (((imm >> 11) & 1) << 7)
        | 0b110_0011
}

/// The U-type format: the upper 20 bits of a value.
pub fn u(opcode: u32, rd: u32, imm: u32) -> u32 {
    (imm & 0xffff_f000) | (rd << 7) | opcode
}

/// The J-type format: `jal`.
pub fn j(rd: u32, offset: i32) -> u32 {
    let imm = offset as u32;
    (((imm >> 20) & 1) << 31)
        | (((imm >> 1) & 0x3ff) << 21)
        | (((imm >> 11) & 1) << 20)
        | (((imm >> 12) & 0xff) << 12)
        | (rd << 7)
        | 0b110_1111
}

pub fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    i(0b001_0011, 0, rd, rs1, imm)
}

pub fn lui(rd: u32, value: u32) -> u32 {
    u(0b011_0111, rd, value)
}

pub fn auipc(rd: u32, value: u32) -> u32 {
    u(0b001_0111, rd, value)
}

pub fn sw(rs2: u32, rs1: u32, imm: i32) -> u32 {
    s(0b010_0011, 0b010, rs1, rs2, imm)
}

pub fn sd(rs2: u32, rs1: u32, imm: i32) -> u32 {
    s(0b010_0011, 0b011, rs1, rs2, imm)
}

/// Joins instructions into the bytes of a program, little endian.
pub fn program(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|w| w.to_le_bytes()).collect()
}
