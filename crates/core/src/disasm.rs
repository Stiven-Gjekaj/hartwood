//! A disassembler: it writes a decoded instruction as assembly text, in the
//! form that `objdump` uses, with the ABI names of the registers.

use crate::decode::{Fp, Instruction, Op};

const X_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

const F_NAMES: [&str; 32] = [
    "ft0", "ft1", "ft2", "ft3", "ft4", "ft5", "ft6", "ft7", "fs0", "fs1", "fa0", "fa1", "fa2",
    "fa3", "fa4", "fa5", "fa6", "fa7", "fs2", "fs3", "fs4", "fs5", "fs6", "fs7", "fs8", "fs9",
    "fs10", "fs11", "ft8", "ft9", "ft10", "ft11",
];

/// The ABI name of an integer register.
pub fn x_name(r: u8) -> &'static str {
    X_NAMES[r as usize & 31]
}

/// The ABI name of a floating point register.
pub fn f_name(r: u8) -> &'static str {
    F_NAMES[r as usize & 31]
}

/// The name of a CSR, or `None` for one that has no name here.
pub fn csr_name(addr: u16) -> Option<&'static str> {
    Some(match addr {
        0x001 => "fflags",
        0x002 => "frm",
        0x003 => "fcsr",
        0xc00 => "cycle",
        0xc01 => "time",
        0xc02 => "instret",
        0x100 => "sstatus",
        0x104 => "sie",
        0x105 => "stvec",
        0x106 => "scounteren",
        0x10a => "senvcfg",
        0x140 => "sscratch",
        0x141 => "sepc",
        0x142 => "scause",
        0x143 => "stval",
        0x144 => "sip",
        0x14d => "stimecmp",
        0x180 => "satp",
        0x300 => "mstatus",
        0x301 => "misa",
        0x302 => "medeleg",
        0x303 => "mideleg",
        0x304 => "mie",
        0x305 => "mtvec",
        0x306 => "mcounteren",
        0x30a => "menvcfg",
        0x320 => "mcountinhibit",
        0x340 => "mscratch",
        0x341 => "mepc",
        0x342 => "mcause",
        0x343 => "mtval",
        0x344 => "mip",
        0x3a0 => "pmpcfg0",
        0x3a2 => "pmpcfg2",
        0x3b0 => "pmpaddr0",
        0xb00 => "mcycle",
        0xb02 => "minstret",
        0xf11 => "mvendorid",
        0xf12 => "marchid",
        0xf13 => "mimpid",
        0xf14 => "mhartid",
        _ => return None,
    })
}

fn suffix(fp: Fp) -> &'static str {
    match fp {
        Fp::S => "s",
        Fp::D => "d",
    }
}

/// The mnemonic of an operation.
pub fn mnemonic(op: Op) -> String {
    use Op::*;
    let fixed = match op {
        Lui => "lui",
        Auipc => "auipc",
        Jal => "jal",
        Jalr => "jalr",
        Beq => "beq",
        Bne => "bne",
        Blt => "blt",
        Bge => "bge",
        Bltu => "bltu",
        Bgeu => "bgeu",
        Lb => "lb",
        Lh => "lh",
        Lw => "lw",
        Ld => "ld",
        Lbu => "lbu",
        Lhu => "lhu",
        Lwu => "lwu",
        Sb => "sb",
        Sh => "sh",
        Sw => "sw",
        Sd => "sd",
        Addi => "addi",
        Slti => "slti",
        Sltiu => "sltiu",
        Xori => "xori",
        Ori => "ori",
        Andi => "andi",
        Slli => "slli",
        Srli => "srli",
        Srai => "srai",
        Add => "add",
        Sub => "sub",
        Sll => "sll",
        Slt => "slt",
        Sltu => "sltu",
        Xor => "xor",
        Srl => "srl",
        Sra => "sra",
        Or => "or",
        And => "and",
        Addiw => "addiw",
        Slliw => "slliw",
        Srliw => "srliw",
        Sraiw => "sraiw",
        Addw => "addw",
        Subw => "subw",
        Sllw => "sllw",
        Srlw => "srlw",
        Sraw => "sraw",
        Fence => "fence",
        Ecall => "ecall",
        Ebreak => "ebreak",
        FenceI => "fence.i",
        Csrrw => "csrrw",
        Csrrs => "csrrs",
        Csrrc => "csrrc",
        Csrrwi => "csrrwi",
        Csrrsi => "csrrsi",
        Csrrci => "csrrci",
        Mret => "mret",
        Sret => "sret",
        Wfi => "wfi",
        SfenceVma => "sfence.vma",
        Mul => "mul",
        Mulh => "mulh",
        Mulhsu => "mulhsu",
        Mulhu => "mulhu",
        Div => "div",
        Divu => "divu",
        Rem => "rem",
        Remu => "remu",
        Mulw => "mulw",
        Divw => "divw",
        Divuw => "divuw",
        Remw => "remw",
        Remuw => "remuw",
        LrW => "lr.w",
        ScW => "sc.w",
        AmoswapW => "amoswap.w",
        AmoaddW => "amoadd.w",
        AmoxorW => "amoxor.w",
        AmoandW => "amoand.w",
        AmoorW => "amoor.w",
        AmominW => "amomin.w",
        AmomaxW => "amomax.w",
        AmominuW => "amominu.w",
        AmomaxuW => "amomaxu.w",
        LrD => "lr.d",
        ScD => "sc.d",
        AmoswapD => "amoswap.d",
        AmoaddD => "amoadd.d",
        AmoxorD => "amoxor.d",
        AmoandD => "amoand.d",
        AmoorD => "amoor.d",
        AmominD => "amomin.d",
        AmomaxD => "amomax.d",
        AmominuD => "amominu.d",
        AmomaxuD => "amomaxu.d",
        Flw => "flw",
        Fld => "fld",
        Fsw => "fsw",
        Fsd => "fsd",
        FcvtSD => "fcvt.s.d",
        FcvtDS => "fcvt.d.s",
        Illegal => "unknown",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    let (base, fp) = match op {
        Fadd(f) => ("fadd", f),
        Fsub(f) => ("fsub", f),
        Fmul(f) => ("fmul", f),
        Fdiv(f) => ("fdiv", f),
        Fsqrt(f) => ("fsqrt", f),
        Fsgnj(f) => ("fsgnj", f),
        Fsgnjn(f) => ("fsgnjn", f),
        Fsgnjx(f) => ("fsgnjx", f),
        Fmin(f) => ("fmin", f),
        Fmax(f) => ("fmax", f),
        Fclass(f) => ("fclass", f),
        Feq(f) => ("feq", f),
        Flt(f) => ("flt", f),
        Fle(f) => ("fle", f),
        Fmadd(f) => ("fmadd", f),
        Fmsub(f) => ("fmsub", f),
        Fnmsub(f) => ("fnmsub", f),
        Fnmadd(f) => ("fnmadd", f),
        FcvtW(f) => return format!("fcvt.w.{}", suffix(f)),
        FcvtWu(f) => return format!("fcvt.wu.{}", suffix(f)),
        FcvtL(f) => return format!("fcvt.l.{}", suffix(f)),
        FcvtLu(f) => return format!("fcvt.lu.{}", suffix(f)),
        FcvtFromW(f) => return format!("fcvt.{}.w", suffix(f)),
        FcvtFromWu(f) => return format!("fcvt.{}.wu", suffix(f)),
        FcvtFromL(f) => return format!("fcvt.{}.l", suffix(f)),
        FcvtFromLu(f) => return format!("fcvt.{}.lu", suffix(f)),
        FmvToInt(Fp::S) => return "fmv.x.w".into(),
        FmvToInt(Fp::D) => return "fmv.x.d".into(),
        FmvFromInt(Fp::S) => return "fmv.w.x".into(),
        FmvFromInt(Fp::D) => return "fmv.d.x".into(),
        _ => return "unknown".into(),
    };
    format!("{base}.{}", suffix(fp))
}

/// Writes one instruction at `pc` as text.
pub fn disassemble(i: &Instruction, pc: u64) -> String {
    use Op::*;
    let m = mnemonic(i.op);
    let (rd, rs1, rs2) = (x_name(i.rd), x_name(i.rs1), x_name(i.rs2));
    let (fd, fs1, fs2, fs3) = (f_name(i.rd), f_name(i.rs1), f_name(i.rs2), f_name(i.rs3));
    let imm = i.imm;
    let target = pc.wrapping_add(imm as u64);
    let csr = || csr_name(i.imm as u16).map_or_else(|| format!("{:#x}", i.imm), str::to_string);
    let operands = match i.op {
        Lui | Auipc => format!("{rd}, {:#x}", (imm >> 12) & 0xfffff),
        Jal => format!("{rd}, {target:#x}"),
        Jalr | Lb | Lh | Lw | Ld | Lbu | Lhu | Lwu => format!("{rd}, {imm}({rs1})"),
        Beq | Bne | Blt | Bge | Bltu | Bgeu => format!("{rs1}, {rs2}, {target:#x}"),
        Sb | Sh | Sw | Sd => format!("{rs2}, {imm}({rs1})"),
        Addi | Slti | Sltiu | Xori | Ori | Andi | Addiw => format!("{rd}, {rs1}, {imm}"),
        Slli | Srli | Srai | Slliw | Srliw | Sraiw => format!("{rd}, {rs1}, {imm:#x}"),
        Add | Sub | Sll | Slt | Sltu | Xor | Srl | Sra | Or | And | Addw | Subw | Sllw | Srlw
        | Sraw | Mul | Mulh | Mulhsu | Mulhu | Div | Divu | Rem | Remu | Mulw | Divw | Divuw
        | Remw | Remuw => format!("{rd}, {rs1}, {rs2}"),
        Csrrw | Csrrs | Csrrc => format!("{rd}, {}, {rs1}", csr()),
        Csrrwi | Csrrsi | Csrrci => format!("{rd}, {}, {}", csr(), i.rs1),
        SfenceVma => format!("{rs1}, {rs2}"),
        LrW | LrD => format!("{rd}, ({rs1})"),
        ScW | ScD | AmoswapW | AmoaddW | AmoxorW | AmoandW | AmoorW | AmominW | AmomaxW
        | AmominuW | AmomaxuW | AmoswapD | AmoaddD | AmoxorD | AmoandD | AmoorD | AmominD
        | AmomaxD | AmominuD | AmomaxuD => format!("{rd}, {rs2}, ({rs1})"),
        Flw | Fld => format!("{fd}, {imm}({rs1})"),
        Fsw | Fsd => format!("{fs2}, {imm}({rs1})"),
        Fadd(_) | Fsub(_) | Fmul(_) | Fdiv(_) | Fsgnj(_) | Fsgnjn(_) | Fsgnjx(_) | Fmin(_)
        | Fmax(_) => format!("{fd}, {fs1}, {fs2}"),
        Fsqrt(_) | FcvtSD | FcvtDS => format!("{fd}, {fs1}"),
        Fmadd(_) | Fmsub(_) | Fnmsub(_) | Fnmadd(_) => format!("{fd}, {fs1}, {fs2}, {fs3}"),
        FcvtW(_) | FcvtWu(_) | FcvtL(_) | FcvtLu(_) | FmvToInt(_) | Fclass(_) => {
            format!("{rd}, {fs1}")
        }
        FcvtFromW(_) | FcvtFromWu(_) | FcvtFromL(_) | FcvtFromLu(_) | FmvFromInt(_) => {
            format!("{fd}, {rs1}")
        }
        Feq(_) | Flt(_) | Fle(_) => format!("{rd}, {fs1}, {fs2}"),
        Fence | FenceI | Ecall | Ebreak | Mret | Sret | Wfi => String::new(),
        Illegal => format!("{:#x}", i.raw),
    };
    if operands.is_empty() {
        m
    } else {
        format!("{m} {operands}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{decode, decode_compressed};

    /// Each line is an instruction at address 0x1000, with its bits and the
    /// text of `riscv64-elf-objdump -M no-aliases`. objdump writes no space
    /// after a comma and puts no `0x` before a jump target. The other parts
    /// are the same.
    #[test]
    fn the_text_matches_objdump() {
        let cases: &[(u32, &str)] = &[
            (0xfff5_8513, "addi a0, a1, -1"),
            (0xfea1_3c23, "sd a0, -8(sp)"),
            (0x3405_9573, "csrrw a0, mscratch, a1"),
            (0x0ec5_a52f, "amoswap.w a0, a2, (a1)"),
            (0x6ac5_9543, "fmadd.d fa0, fa1, fa2, fa3"),
            (0xc035_b553, "fcvt.lu.s a0, fa1"),
            (0x1050_0073, "wfi"),
            (0x0010_00ef, "jal ra, 0x1800"),
            (0x43f5_5513, "srai a0, a0, 0x3f"),
        ];
        for &(raw, text) in cases {
            assert_eq!(disassemble(&decode(raw), 0x1000), text);
        }
    }

    #[test]
    fn a_compressed_instruction_shows_its_long_form() {
        assert_eq!(
            disassemble(&decode_compressed(0x7139), 0),
            "addi sp, sp, -64"
        );
    }
}
