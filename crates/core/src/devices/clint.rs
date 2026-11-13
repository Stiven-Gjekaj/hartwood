//! The CLINT: the timer and the software interrupt of M-mode.
//!
//! `mtime` counts steps of the hart, and not the time of the host. A step is
//! one instruction or one trap. So two runs of the same program see the same
//! times, and a run in the browser is the same as a run in a terminal.

/// The offsets of the registers.
const MSIP: u64 = 0x0000;
const MTIMECMP: u64 = 0x4000;
const MTIME: u64 = 0xbff8;

/// The size of the register block.
pub const SIZE: u64 = 0x1_0000;

#[derive(Debug, Clone)]
pub struct Clint {
    pub msip: bool,
    pub mtimecmp: u64,
    pub mtime: u64,
}

impl Default for Clint {
    fn default() -> Self {
        Self {
            msip: false,
            // No timer interrupt until software sets a compare value.
            mtimecmp: u64::MAX,
            mtime: 0,
        }
    }
}

impl Clint {
    /// Reads a register. The 64-bit registers can be read in two halves.
    pub fn read(&self, offset: u64, width: usize) -> Option<u64> {
        let (base, value) = match offset {
            MSIP..=0x3 => (MSIP, u64::from(self.msip)),
            MTIMECMP..=0x4007 => (MTIMECMP, self.mtimecmp),
            MTIME..=0xbfff => (MTIME, self.mtime),
            _ => return None,
        };
        read_part(value, offset - base, width)
    }

    pub fn write(&mut self, offset: u64, width: usize, value: u64) -> Option<()> {
        match offset {
            MSIP..=0x3 => self.msip = value & 1 == 1,
            MTIMECMP..=0x4007 => {
                self.mtimecmp = write_part(self.mtimecmp, offset - MTIMECMP, width, value)?;
            }
            MTIME..=0xbfff => self.mtime = write_part(self.mtime, offset - MTIME, width, value)?,
            _ => return None,
        }
        Some(())
    }

    /// Tells if the timer interrupt of M-mode is pending.
    pub fn timer_pending(&self) -> bool {
        self.mtime >= self.mtimecmp
    }
}

/// Reads `width` bytes at byte `offset` of a 64-bit register.
pub(crate) fn read_part(value: u64, offset: u64, width: usize) -> Option<u64> {
    if offset as usize + width > 8 {
        return None;
    }
    let shifted = value >> (8 * offset);
    Some(if width == 8 {
        shifted
    } else {
        shifted & ((1 << (8 * width)) - 1)
    })
}

/// Writes `width` bytes at byte `offset` of a 64-bit register.
pub(crate) fn write_part(old: u64, offset: u64, width: usize, value: u64) -> Option<u64> {
    if offset as usize + width > 8 {
        return None;
    }
    let mask = if width == 8 {
        u64::MAX
    } else {
        ((1u64 << (8 * width)) - 1) << (8 * offset)
    };
    Some((old & !mask) | ((value << (8 * offset)) & mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timer_fires_when_mtime_reaches_mtimecmp() {
        let mut c = Clint::default();
        assert!(!c.timer_pending());
        c.write(MTIMECMP, 8, 100).unwrap();
        c.mtime = 99;
        assert!(!c.timer_pending());
        c.mtime = 100;
        assert!(c.timer_pending());
    }

    #[test]
    fn a_64_bit_register_has_two_halves() {
        let mut c = Clint::default();
        c.write(MTIMECMP, 4, 0x1111_2222).unwrap();
        c.write(MTIMECMP + 4, 4, 0x3333_4444).unwrap();
        assert_eq!(c.mtimecmp, 0x3333_4444_1111_2222);
        assert_eq!(c.read(MTIMECMP + 4, 4), Some(0x3333_4444));
        c.mtime = 0xaaaa_bbbb_cccc_dddd;
        assert_eq!(c.read(MTIME, 4), Some(0xcccc_dddd));
        assert_eq!(c.read(MTIME, 8), Some(0xaaaa_bbbb_cccc_dddd));
    }

    #[test]
    fn msip_holds_one_bit() {
        let mut c = Clint::default();
        c.write(MSIP, 4, 0xff).unwrap();
        assert!(c.msip);
        assert_eq!(c.read(MSIP, 4), Some(1));
    }
}
