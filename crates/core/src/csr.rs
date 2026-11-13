//! The control and status registers.
//!
//! Each CSR that hartwood implements is a named field, and each write goes
//! through the mask of the bits that software can change. `sstatus`, `sie`
//! and `sip` are views of `mstatus`, `mie` and `mip`, so they have no field
//! of their own.

use crate::trap::Interrupt;

/// The privilege level of the hart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    User = 0,
    Supervisor = 1,
    Machine = 3,
}

impl Mode {
    /// The mode for the two bits of `mstatus.MPP`. The value 2 is reserved,
    /// and `mstatus` never holds it.
    pub fn from_bits(bits: u64) -> Mode {
        match bits & 3 {
            0 => Mode::User,
            1 => Mode::Supervisor,
            _ => Mode::Machine,
        }
    }
}

// The addresses of the CSRs.
pub const FFLAGS: u16 = 0x001;
pub const FRM: u16 = 0x002;
pub const FCSR: u16 = 0x003;
pub const CYCLE: u16 = 0xc00;
pub const TIME: u16 = 0xc01;
pub const INSTRET: u16 = 0xc02;
pub const SSTATUS: u16 = 0x100;
pub const SIE: u16 = 0x104;
pub const STVEC: u16 = 0x105;
pub const SCOUNTEREN: u16 = 0x106;
pub const SENVCFG: u16 = 0x10a;
pub const SSCRATCH: u16 = 0x140;
pub const SEPC: u16 = 0x141;
pub const SCAUSE: u16 = 0x142;
pub const STVAL: u16 = 0x143;
pub const SIP: u16 = 0x144;
pub const SATP: u16 = 0x180;
pub const MSTATUS: u16 = 0x300;
pub const MISA: u16 = 0x301;
pub const MEDELEG: u16 = 0x302;
pub const MIDELEG: u16 = 0x303;
pub const MIE: u16 = 0x304;
pub const MTVEC: u16 = 0x305;
pub const MCOUNTEREN: u16 = 0x306;
pub const MENVCFG: u16 = 0x30a;
pub const MCOUNTINHIBIT: u16 = 0x320;
pub const MSCRATCH: u16 = 0x340;
pub const MEPC: u16 = 0x341;
pub const MCAUSE: u16 = 0x342;
pub const MTVAL: u16 = 0x343;
pub const MIP: u16 = 0x344;
pub const MCYCLE: u16 = 0xb00;
pub const MINSTRET: u16 = 0xb02;
pub const MVENDORID: u16 = 0xf11;
pub const MARCHID: u16 = 0xf12;
pub const MIMPID: u16 = 0xf13;
pub const MHARTID: u16 = 0xf14;
pub const MCONFIGPTR: u16 = 0xf15;

// The bits of mstatus.
pub const MSTATUS_SIE: u64 = 1 << 1;
pub const MSTATUS_MIE: u64 = 1 << 3;
pub const MSTATUS_SPIE: u64 = 1 << 5;
pub const MSTATUS_MPIE: u64 = 1 << 7;
pub const MSTATUS_SPP: u64 = 1 << 8;
pub const MSTATUS_MPP_SHIFT: u64 = 11;
pub const MSTATUS_MPP: u64 = 3 << MSTATUS_MPP_SHIFT;
pub const MSTATUS_FS_SHIFT: u64 = 13;
pub const MSTATUS_FS: u64 = 3 << MSTATUS_FS_SHIFT;
pub const MSTATUS_MPRV: u64 = 1 << 17;
pub const MSTATUS_SUM: u64 = 1 << 18;
pub const MSTATUS_MXR: u64 = 1 << 19;
pub const MSTATUS_TVM: u64 = 1 << 20;
pub const MSTATUS_TW: u64 = 1 << 21;
pub const MSTATUS_TSR: u64 = 1 << 22;
pub const MSTATUS_UXL: u64 = 3 << 32;
pub const MSTATUS_SXL: u64 = 3 << 34;
pub const MSTATUS_SD: u64 = 1 << 63;

/// The bits of `mstatus` that a write can change.
const MSTATUS_WRITABLE: u64 = MSTATUS_SIE
    | MSTATUS_MIE
    | MSTATUS_SPIE
    | MSTATUS_MPIE
    | MSTATUS_SPP
    | MSTATUS_MPP
    | MSTATUS_FS
    | MSTATUS_MPRV
    | MSTATUS_SUM
    | MSTATUS_MXR
    | MSTATUS_TVM
    | MSTATUS_TW
    | MSTATUS_TSR;

/// The bits of `mstatus` that `sstatus` shows.
const SSTATUS_VISIBLE: u64 = MSTATUS_SIE
    | MSTATUS_SPIE
    | MSTATUS_SPP
    | MSTATUS_FS
    | MSTATUS_SUM
    | MSTATUS_MXR
    | MSTATUS_UXL
    | MSTATUS_SD;

/// The bits of `sstatus` that a write can change.
const SSTATUS_WRITABLE: u64 =
    MSTATUS_SIE | MSTATUS_SPIE | MSTATUS_SPP | MSTATUS_FS | MSTATUS_SUM | MSTATUS_MXR;

/// RV64 with the extensions I, M, A, F, D, C, and the modes S and U.
const MISA_VALUE: u64 = (2 << 62)
    | (1 << 0)   // A
    | (1 << 2)   // C
    | (1 << 3)   // D
    | (1 << 5)   // F
    | (1 << 8)   // I
    | (1 << 12)  // M
    | (1 << 18)  // S
    | (1 << 20); // U

/// The interrupts that M-mode can give to S-mode.
const DELEGABLE_INTERRUPTS: u64 = (1 << 1) | (1 << 5) | (1 << 9);

/// The exceptions that M-mode can give to S-mode. An `ecall` from M-mode
/// always stays in M-mode.
const DELEGABLE_EXCEPTIONS: u64 = 0xffff & !(1 << 11) & !(1 << 10) & !(1 << 14);

/// The bits of `mip` that software can write. The CLINT and the PLIC drive
/// the other bits.
const MIP_WRITABLE: u64 = (1 << 1) | (1 << 5) | (1 << 9);

/// The interrupt bits that exist.
const INTERRUPT_BITS: u64 = 0xaaa;

/// What a CSR access needs, other than the CSR file.
pub struct CsrContext {
    pub mode: Mode,
    /// The number of instructions that retired, for `cycle` and `instret`.
    pub instret: u64,
    /// The value of `mtime`, for `time`.
    pub time: u64,
}

/// The CSR file.
#[derive(Debug, Clone, Default)]
pub struct Csr {
    pub mstatus: u64,
    pub medeleg: u64,
    pub mideleg: u64,
    pub mie: u64,
    /// The bits of `mip` that software wrote. See [`Csr::mip`].
    pub mip_soft: u64,
    /// The bits of `mip` that the devices drive.
    pub mip_hard: u64,
    pub mtvec: u64,
    pub mcounteren: u64,
    pub menvcfg: u64,
    pub mscratch: u64,
    pub mepc: u64,
    pub mcause: u64,
    pub mtval: u64,
    pub stvec: u64,
    pub scounteren: u64,
    pub senvcfg: u64,
    pub sscratch: u64,
    pub sepc: u64,
    pub scause: u64,
    pub stval: u64,
    pub satp: u64,
    /// The accrued exception flags of the FPU, five bits.
    pub fflags: u64,
    /// The dynamic rounding mode of the FPU, three bits.
    pub frm: u64,
}

impl Csr {
    pub fn new() -> Self {
        Self {
            // UXL and SXL say that U-mode and S-mode are 64-bit.
            mstatus: (2 << 32) | (2 << 34),
            ..Self::default()
        }
    }

    /// The value of `mip`: the software bits and the device bits.
    pub fn mip(&self) -> u64 {
        self.mip_soft | self.mip_hard
    }

    /// Sets or clears one interrupt bit that a device drives.
    pub fn set_pending(&mut self, irq: Interrupt, on: bool) {
        if on {
            self.mip_hard |= irq.bit();
        } else {
            self.mip_hard &= !irq.bit();
        }
    }

    /// The privilege mode that `mstatus.MPP` holds.
    pub fn mpp(&self) -> Mode {
        Mode::from_bits(self.mstatus >> MSTATUS_MPP_SHIFT)
    }

    /// Tells if a CSR exists and the hart can access it in this mode. A
    /// `false` means an illegal instruction.
    pub fn accessible(&self, addr: u16, ctx: &CsrContext, write: bool) -> bool {
        let lowest = Mode::from_bits(u64::from(addr >> 8));
        if ctx.mode < lowest {
            return false;
        }
        if write && (addr >> 10) == 3 {
            return false;
        }
        // satp traps in S-mode when mstatus.TVM is set.
        if addr == SATP && ctx.mode == Mode::Supervisor && self.mstatus & MSTATUS_TVM != 0 {
            return false;
        }
        // The F registers trap when mstatus.FS is off.
        if matches!(addr, FFLAGS | FRM | FCSR) && self.mstatus & MSTATUS_FS == 0 {
            return false;
        }
        // A counter in U-mode or S-mode needs its bit in mcounteren, and in
        // U-mode also its bit in scounteren.
        if (CYCLE..=CYCLE + 31).contains(&addr) {
            let bit = 1u64 << (addr - CYCLE);
            if ctx.mode < Mode::Machine && self.mcounteren & bit == 0 {
                return false;
            }
            if ctx.mode == Mode::User && self.scounteren & bit == 0 {
                return false;
            }
        }
        self.read(addr, ctx).is_some()
    }

    /// Reads a CSR. `None` means that it does not exist.
    pub fn read(&self, addr: u16, ctx: &CsrContext) -> Option<u64> {
        let v = match addr {
            FFLAGS => self.fflags,
            FRM => self.frm,
            FCSR => (self.frm << 5) | self.fflags,
            CYCLE | INSTRET | MCYCLE | MINSTRET => ctx.instret,
            TIME => ctx.time,
            // The other performance counters exist, and count nothing.
            0xc03..=0xc1f | 0xb03..=0xb1f | 0x323..=0x33f => 0,
            SSTATUS => self.sstatus(),
            SIE => self.mie & self.mideleg,
            STVEC => self.stvec,
            SCOUNTEREN => self.scounteren,
            SENVCFG => self.senvcfg,
            SSCRATCH => self.sscratch,
            SEPC => self.sepc,
            SCAUSE => self.scause,
            STVAL => self.stval,
            SIP => self.mip() & self.mideleg,
            SATP => self.satp,
            MSTATUS => self.mstatus_with_sd(),
            MISA => MISA_VALUE,
            MEDELEG => self.medeleg,
            MIDELEG => self.mideleg,
            MIE => self.mie,
            MTVEC => self.mtvec,
            MCOUNTEREN => self.mcounteren,
            MENVCFG => self.menvcfg,
            MCOUNTINHIBIT => 0,
            MSCRATCH => self.mscratch,
            MEPC => self.mepc,
            MCAUSE => self.mcause,
            MTVAL => self.mtval,
            MIP => self.mip(),
            // hartwood has no PMP. The registers read as zero, and a write
            // does nothing, as the specification allows.
            0x3a0..=0x3ef => 0,
            MVENDORID | MARCHID | MIMPID | MHARTID | MCONFIGPTR => 0,
            _ => return None,
        };
        Some(v)
    }

    /// Writes a CSR. The caller checked the access with
    /// [`Csr::accessible`]. A bit that software cannot change keeps its
    /// value.
    pub fn write(&mut self, addr: u16, value: u64) {
        match addr {
            FFLAGS => {
                self.fflags = value & 0x1f;
                self.make_fs_dirty();
            }
            FRM => {
                self.frm = value & 0x7;
                self.make_fs_dirty();
            }
            FCSR => {
                self.fflags = value & 0x1f;
                self.frm = (value >> 5) & 0x7;
                self.make_fs_dirty();
            }
            SSTATUS => {
                self.mstatus = (self.mstatus & !SSTATUS_WRITABLE) | (value & SSTATUS_WRITABLE);
            }
            SIE => self.mie = (self.mie & !self.mideleg) | (value & self.mideleg),
            STVEC => self.stvec = legal_tvec(value),
            SCOUNTEREN => self.scounteren = value & 0xffff_ffff,
            SENVCFG => self.senvcfg = 0,
            SSCRATCH => self.sscratch = value,
            SEPC => self.sepc = value & !1,
            SCAUSE => self.scause = value,
            STVAL => self.stval = value,
            SIP => {
                // Only the software interrupt of S-mode is writable here.
                let mask = self.mideleg & (1 << 1);
                self.mip_soft = (self.mip_soft & !mask) | (value & mask);
            }
            SATP => self.write_satp(value),
            MSTATUS => {
                let mut v = (self.mstatus & !MSTATUS_WRITABLE) | (value & MSTATUS_WRITABLE);
                // MPP = 2 is reserved. Keep the old MPP.
                if (v & MSTATUS_MPP) >> MSTATUS_MPP_SHIFT == 2 {
                    v = (v & !MSTATUS_MPP) | (self.mstatus & MSTATUS_MPP);
                }
                self.mstatus = v;
            }
            MEDELEG => self.medeleg = value & DELEGABLE_EXCEPTIONS,
            MIDELEG => self.mideleg = value & DELEGABLE_INTERRUPTS,
            MIE => self.mie = value & INTERRUPT_BITS,
            MTVEC => self.mtvec = legal_tvec(value),
            MCOUNTEREN => self.mcounteren = value & 0xffff_ffff,
            MENVCFG => self.menvcfg = 0,
            MSCRATCH => self.mscratch = value,
            MEPC => self.mepc = value & !1,
            MCAUSE => self.mcause = value,
            MTVAL => self.mtval = value,
            MIP => self.mip_soft = (self.mip_soft & !MIP_WRITABLE) | (value & MIP_WRITABLE),
            _ => {}
        }
    }

    fn sstatus(&self) -> u64 {
        self.mstatus_with_sd() & SSTATUS_VISIBLE
    }

    /// `mstatus` with the SD bit, which is set when FS is dirty.
    fn mstatus_with_sd(&self) -> u64 {
        if self.mstatus & MSTATUS_FS == MSTATUS_FS {
            self.mstatus | MSTATUS_SD
        } else {
            self.mstatus
        }
    }

    /// Marks the state of the FPU as dirty, after a write to an F register
    /// or to `fcsr`.
    pub fn make_fs_dirty(&mut self) {
        self.mstatus |= MSTATUS_FS;
    }

    /// Writes `satp`. A mode other than Bare (0) or Sv39 (8) is not
    /// supported, and the specification says that such a write does
    /// nothing.
    fn write_satp(&mut self, value: u64) {
        let mode = value >> 60;
        if mode == 0 || mode == 8 {
            // hartwood has no address space IDs, so ASID reads as zero.
            self.satp = value & !(0xffff << 44);
        }
    }
}

/// Makes a legal value for `mtvec` or `stvec`: the modes Direct (0) and
/// Vectored (1) exist. A reserved mode becomes Direct.
fn legal_tvec(value: u64) -> u64 {
    if value & 3 >= 2 { value & !3 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(mode: Mode) -> CsrContext {
        CsrContext {
            mode,
            instret: 7,
            time: 9,
        }
    }

    #[test]
    fn a_machine_csr_is_not_accessible_from_supervisor_mode() {
        let csr = Csr::new();
        assert!(csr.accessible(MSTATUS, &ctx(Mode::Machine), true));
        assert!(!csr.accessible(MSTATUS, &ctx(Mode::Supervisor), false));
        assert!(csr.accessible(SSTATUS, &ctx(Mode::Supervisor), true));
        assert!(!csr.accessible(SSTATUS, &ctx(Mode::User), false));
    }

    #[test]
    fn a_read_only_csr_refuses_a_write() {
        let csr = Csr::new();
        assert!(csr.accessible(MHARTID, &ctx(Mode::Machine), false));
        assert!(!csr.accessible(MHARTID, &ctx(Mode::Machine), true));
    }

    #[test]
    fn an_unknown_csr_is_not_accessible() {
        let csr = Csr::new();
        assert!(!csr.accessible(0x7c0, &ctx(Mode::Machine), false));
    }

    #[test]
    fn sstatus_is_a_view_of_mstatus() {
        let mut csr = Csr::new();
        csr.write(MSTATUS, MSTATUS_MIE | MSTATUS_SIE | MSTATUS_MPP);
        let s = csr.read(SSTATUS, &ctx(Mode::Machine)).unwrap();
        assert_eq!(s & MSTATUS_SIE, MSTATUS_SIE);
        assert_eq!(s & (MSTATUS_MIE | MSTATUS_MPP), 0);
        csr.write(SSTATUS, 0);
        assert_eq!(csr.mstatus & MSTATUS_SIE, 0);
        assert_eq!(csr.mstatus & MSTATUS_MIE, MSTATUS_MIE);
    }

    #[test]
    fn the_reserved_mpp_value_keeps_the_old_value() {
        let mut csr = Csr::new();
        csr.write(MSTATUS, 1 << MSTATUS_MPP_SHIFT);
        csr.write(MSTATUS, 2 << MSTATUS_MPP_SHIFT);
        assert_eq!(csr.mpp(), Mode::Supervisor);
    }

    #[test]
    fn sd_follows_a_dirty_fs() {
        let mut csr = Csr::new();
        assert_eq!(
            csr.read(MSTATUS, &ctx(Mode::Machine)).unwrap() & MSTATUS_SD,
            0
        );
        csr.make_fs_dirty();
        assert_ne!(
            csr.read(MSTATUS, &ctx(Mode::Machine)).unwrap() & MSTATUS_SD,
            0
        );
    }

    #[test]
    fn sie_shows_only_the_delegated_bits() {
        let mut csr = Csr::new();
        csr.write(MIE, 0xaaa);
        csr.write(MIDELEG, 1 << 5);
        assert_eq!(csr.read(SIE, &ctx(Mode::Machine)), Some(1 << 5));
        csr.write(SIE, 0);
        assert_eq!(csr.mie, 0xaaa & !(1 << 5));
    }

    #[test]
    fn mip_combines_software_and_device_bits() {
        let mut csr = Csr::new();
        csr.set_pending(Interrupt::MachineTimer, true);
        csr.write(MIP, 0xfff);
        assert_eq!(csr.mip(), (1 << 7) | MIP_WRITABLE);
    }

    #[test]
    fn a_counter_needs_its_enable_bit_below_machine_mode() {
        let mut csr = Csr::new();
        assert!(!csr.accessible(CYCLE, &ctx(Mode::Supervisor), false));
        csr.write(MCOUNTEREN, 1);
        assert!(csr.accessible(CYCLE, &ctx(Mode::Supervisor), false));
        assert!(!csr.accessible(CYCLE, &ctx(Mode::User), false));
        csr.write(SCOUNTEREN, 1);
        assert!(csr.accessible(CYCLE, &ctx(Mode::User), false));
        assert_eq!(csr.read(CYCLE, &ctx(Mode::User)), Some(7));
    }

    #[test]
    fn satp_refuses_an_unsupported_mode() {
        let mut csr = Csr::new();
        csr.write(SATP, (8 << 60) | 0x1234);
        assert_eq!(csr.satp, (8 << 60) | 0x1234);
        csr.write(SATP, (9 << 60) | 0x5678);
        assert_eq!(csr.satp, (8 << 60) | 0x1234);
    }

    #[test]
    fn the_fpu_csrs_trap_while_fs_is_off() {
        let mut csr = Csr::new();
        assert!(!csr.accessible(FCSR, &ctx(Mode::Machine), false));
        csr.write(MSTATUS, 1 << MSTATUS_FS_SHIFT);
        assert!(csr.accessible(FCSR, &ctx(Mode::Machine), false));
        csr.write(FCSR, 0xff);
        assert_eq!((csr.frm, csr.fflags), (7, 0x1f));
    }
}
