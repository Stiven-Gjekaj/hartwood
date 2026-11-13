//! Physical memory protection: 16 entries, each with a range and the rights
//! of R, W and X on it.
//!
//! An entry in TOR mode covers the addresses from the previous `pmpaddr` up
//! to its own. NA4 covers four bytes. NAPOT covers a power of two that the
//! low one bits of `pmpaddr` encode. The first entry that matches an access
//! decides. With no match, M-mode may do anything, and S-mode and U-mode may
//! do nothing.

use crate::cpu::Access;
use crate::csr::Mode;

pub const ENTRIES: usize = 16;

const R: u8 = 1 << 0;
const W: u8 = 1 << 1;
const X: u8 = 1 << 2;
const A_SHIFT: u8 = 3;
const L: u8 = 1 << 7;

const OFF: u8 = 0;
const TOR: u8 = 1;
const NA4: u8 = 2;
const NAPOT: u8 = 3;

/// The 54 address bits of `pmpaddr`: physical address bits 55..2.
const ADDR_MASK: u64 = (1 << 54) - 1;

#[derive(Debug, Clone, Default)]
pub struct Pmp {
    cfg: [u8; ENTRIES],
    addr: [u64; ENTRIES],
}

impl Pmp {
    /// Reads `pmpcfg0` or `pmpcfg2` (`n` is 0 or 2): eight entries, one byte
    /// each.
    pub fn read_cfg(&self, n: usize) -> u64 {
        let base = n * 4;
        (0..8).fold(0, |v, i| v | (u64::from(self.cfg[base + i]) << (8 * i)))
    }

    /// Writes `pmpcfg0` or `pmpcfg2`. A locked entry keeps its byte. The
    /// combination W without R is reserved, so W is cleared there.
    pub fn write_cfg(&mut self, n: usize, value: u64) {
        let base = n * 4;
        for i in 0..8 {
            if self.cfg[base + i] & L != 0 {
                continue;
            }
            let mut byte = (value >> (8 * i)) as u8 & !(3 << 5);
            if byte & (R | W) == W {
                byte &= !W;
            }
            self.cfg[base + i] = byte;
        }
    }

    pub fn read_addr(&self, i: usize) -> u64 {
        self.addr[i]
    }

    /// Writes `pmpaddr<i>`. A locked entry, or the base of a locked TOR
    /// entry, keeps its value.
    pub fn write_addr(&mut self, i: usize, value: u64) {
        let locked = self.cfg[i] & L != 0;
        let next_is_locked_tor =
            i + 1 < ENTRIES && self.cfg[i + 1] & L != 0 && self.mode(i + 1) == TOR;
        if !locked && !next_is_locked_tor {
            self.addr[i] = value & ADDR_MASK;
        }
    }

    fn mode(&self, i: usize) -> u8 {
        (self.cfg[i] >> A_SHIFT) & 3
    }

    /// Tells if no entry is locked. Then each access from M-mode is allowed,
    /// and the check of M-mode can stop at once.
    pub fn machine_is_free(&self) -> bool {
        self.cfg.iter().all(|c| c & L == 0)
    }

    /// The range `[start, end)` that entry `i` covers, in bytes.
    fn range(&self, i: usize) -> Option<(u64, u64)> {
        let a = self.addr[i];
        match self.mode(i) {
            OFF => None,
            TOR => {
                let start = if i == 0 { 0 } else { self.addr[i - 1] << 2 };
                Some((start, a << 2))
            }
            NA4 => Some((a << 2, (a << 2) + 4)),
            NAPOT => {
                // NAPOT: the number of trailing ones gives the size.
                let ones = a.trailing_ones();
                let size = 1u128 << (ones + 3);
                let start = (a & !((1u64 << ones) - 1)) << 2;
                let end = (u128::from(start) + size).min(1 << 64) as u64;
                Some((start, end.max(start)))
            }
            // The mode has two bits, so the four arms above are all.
            _ => None,
        }
    }

    /// Checks one access of `width` bytes at the physical address `addr`.
    pub fn allows(&self, addr: u64, width: u64, access: Access, mode: Mode) -> bool {
        if mode == Mode::Machine && self.machine_is_free() {
            return true;
        }
        let last = addr.saturating_add(width - 1);
        for i in 0..ENTRIES {
            let Some((start, end)) = self.range(i) else {
                continue;
            };
            let first_in = addr >= start && addr < end;
            let last_in = last >= start && last < end;
            if !first_in && !last_in {
                continue;
            }
            // An access that is only partly inside an entry fails.
            if first_in != last_in {
                return false;
            }
            let cfg = self.cfg[i];
            if mode == Mode::Machine && cfg & L == 0 {
                return true;
            }
            let need = match access {
                Access::Fetch => X,
                Access::Load => R,
                Access::Store => W,
            };
            return cfg & need != 0;
        }
        mode == Mode::Machine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configuration that OpenSBI and the riscv-tests start code write:
    /// entry 0 covers all memory with R, W and X.
    fn all_memory() -> Pmp {
        let mut p = Pmp::default();
        p.write_addr(0, u64::MAX);
        p.write_cfg(0, u64::from(R | W | X | (NAPOT << A_SHIFT)));
        p
    }

    #[test]
    fn with_no_entry_only_machine_mode_has_access() {
        let p = Pmp::default();
        assert!(p.allows(0x8000_0000, 8, Access::Load, Mode::Machine));
        assert!(!p.allows(0x8000_0000, 8, Access::Load, Mode::Supervisor));
    }

    #[test]
    fn a_napot_entry_of_all_ones_covers_all_memory() {
        let p = all_memory();
        assert!(p.allows(0, 1, Access::Fetch, Mode::User));
        assert!(p.allows((1 << 56) - 8, 8, Access::Store, Mode::Supervisor));
    }

    #[test]
    fn a_tor_entry_covers_up_to_its_address() {
        let mut p = Pmp::default();
        p.write_addr(0, 0x8000_0000 >> 2);
        p.write_addr(1, 0x8000_1000 >> 2);
        p.write_cfg(0, u64::from(R | (TOR << A_SHIFT)) << 8);
        assert!(p.allows(0x8000_0000, 4, Access::Load, Mode::User));
        assert!(!p.allows(0x8000_0000, 4, Access::Store, Mode::User));
        assert!(!p.allows(0x8000_1000, 4, Access::Load, Mode::User));
        // An access across the end of the entry fails.
        assert!(!p.allows(0x8000_0ffc, 8, Access::Load, Mode::User));
    }

    #[test]
    fn a_locked_entry_applies_to_machine_mode_and_keeps_its_value() {
        let mut p = Pmp::default();
        p.write_addr(0, 0x1000 >> 2);
        p.write_cfg(0, u64::from(L | NA4 << A_SHIFT));
        assert!(!p.allows(0x1000, 4, Access::Load, Mode::Machine));
        assert!(p.allows(0x2000, 4, Access::Load, Mode::Machine));
        p.write_cfg(0, 0);
        p.write_addr(0, 0);
        assert_eq!(p.read_cfg(0), u64::from(L | NA4 << A_SHIFT));
        assert_eq!(p.read_addr(0), 0x1000 >> 2);
    }

    #[test]
    fn write_without_read_is_reserved() {
        let mut p = Pmp::default();
        p.write_cfg(0, u64::from(W));
        assert_eq!(p.read_cfg(0), 0);
    }

    #[test]
    fn the_first_matching_entry_decides() {
        let mut p = all_memory();
        // Entry 0 now covers 4 KiB with no rights, before entry 1 with all.
        p.write_addr(1, u64::MAX);
        p.write_addr(0, (0x8000_0000 >> 2) | 0x1ff);
        p.write_cfg(
            0,
            u64::from(NAPOT << A_SHIFT) | (u64::from(R | W | X | NAPOT << A_SHIFT) << 8),
        );
        assert!(!p.allows(0x8000_0800, 4, Access::Load, Mode::Supervisor));
        assert!(p.allows(0x8000_1000, 4, Access::Load, Mode::Supervisor));
    }
}
