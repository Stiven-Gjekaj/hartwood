//! The Sv39 MMU: the page table walk, the permission checks, and the TLB.
//!
//! A virtual address of Sv39 has 39 bits: three 9-bit page numbers and a
//! 12-bit offset. Bits 63..39 must copy bit 38. The walk starts at the page
//! table in `satp` and reads one entry for each level, from level 2 down to
//! level 0. An entry with R, W or X set is a leaf. A leaf at level 2 maps
//! 1 GiB, at level 1 2 MiB, and at level 0 4 KiB.
//!
//! The hart sets the A bit and the D bit of a leaf in memory when it uses the
//! page, as the Svadu extension describes. Software does not take a page
//! fault for them.

use super::{Access, Cpu};
use crate::csr::{MSTATUS_MXR, MSTATUS_SUM, Mode};
use crate::trap::Exception;

const PAGE_SIZE: u64 = 4096;
const LEVELS: u32 = 3;

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;
/// Bits 63..54 of a PTE: N, PBMT and reserved bits. hartwood implements
/// neither Svnapot nor Svpbmt, so each one must be zero.
const PTE_RESERVED: u64 = 0x3ff << 54;

/// The number of entries in the TLB.
const TLB_SIZE: usize = 256;

/// One cached translation of a 4 KiB virtual page.
#[derive(Debug, Clone, Copy, Default)]
struct TlbEntry {
    valid: bool,
    /// The virtual page number: the address shifted right by 12.
    vpn: u64,
    /// The physical address of the start of the 4 KiB page.
    page: u64,
    /// The flags of the leaf PTE: R, W, X, U, A, D.
    flags: u64,
}

/// A TLB that holds one translation for each index. The index is the low
/// bits of the virtual page number.
///
/// A superpage fills one entry for each 4 KiB page that the hart touches, so
/// a lookup never needs the level. A hit checks the permissions again,
/// because they depend on the mode and on SUM and MXR, which can change
/// without an `sfence.vma`.
#[derive(Debug, Clone)]
pub struct Tlb {
    entries: Box<[TlbEntry; TLB_SIZE]>,
}

impl Default for Tlb {
    fn default() -> Self {
        Self {
            entries: Box::new([TlbEntry::default(); TLB_SIZE]),
        }
    }
}

impl Tlb {
    fn index(vpn: u64) -> usize {
        (vpn as usize) % TLB_SIZE
    }

    fn lookup(&self, vpn: u64) -> Option<TlbEntry> {
        let e = self.entries[Self::index(vpn)];
        (e.valid && e.vpn == vpn).then_some(e)
    }

    fn insert(&mut self, entry: TlbEntry) {
        self.entries[Self::index(entry.vpn)] = entry;
    }

    /// Removes every translation.
    pub fn flush(&mut self) {
        for e in self.entries.iter_mut() {
            e.valid = false;
        }
    }

    /// Removes the translation of one virtual address.
    pub fn flush_page(&mut self, vaddr: u64) {
        let vpn = vaddr >> 12;
        let e = &mut self.entries[Self::index(vpn)];
        if e.vpn == vpn {
            e.valid = false;
        }
    }
}

/// The page fault for an access of this kind at `vaddr`.
fn page_fault(access: Access, vaddr: u64) -> Exception {
    match access {
        Access::Fetch => Exception::InstructionPageFault(vaddr),
        Access::Load => Exception::LoadPageFault(vaddr),
        Access::Store => Exception::StorePageFault(vaddr),
    }
}

/// The access fault for an access of this kind at `vaddr`.
fn access_fault(access: Access, vaddr: u64) -> Exception {
    match access {
        Access::Fetch => Exception::InstructionAccessFault(vaddr),
        Access::Load => Exception::LoadAccessFault(vaddr),
        Access::Store => Exception::StoreAccessFault(vaddr),
    }
}

impl Cpu {
    /// Turns a virtual address into a physical address.
    ///
    /// M-mode, and any mode while `satp` is Bare, use the virtual address as
    /// the physical address.
    pub(crate) fn translate(&mut self, vaddr: u64, access: Access) -> Result<u64, Exception> {
        let mode = match access {
            Access::Fetch => self.mode,
            _ => self.data_mode(),
        };
        if mode == Mode::Machine || self.csr.satp >> 60 == 0 {
            return Ok(vaddr);
        }

        // Bits 63..39 must be copies of bit 38.
        let top = (vaddr as i64) >> 38;
        if top != 0 && top != -1 {
            return Err(page_fault(access, vaddr));
        }

        let vpn = (vaddr >> 12) & ((1 << 27) - 1);
        let entry = match self.tlb.lookup(vpn) {
            // A store to a page that is not dirty walks again, so that the
            // walk sets D in memory.
            Some(e) if access != Access::Store || e.flags & PTE_D != 0 => e,
            _ => {
                let e = self.walk(vaddr, access)?;
                self.tlb.insert(e);
                e
            }
        };
        self.check_permission(entry.flags, access, mode)
            .map_err(|()| page_fault(access, vaddr))?;
        Ok(entry.page | (vaddr & (PAGE_SIZE - 1)))
    }

    /// Checks the R, W, X and U bits of a leaf for an access in `mode`.
    fn check_permission(&self, flags: u64, access: Access, mode: Mode) -> Result<(), ()> {
        let sum = self.csr.mstatus & MSTATUS_SUM != 0;
        let mxr = self.csr.mstatus & MSTATUS_MXR != 0;
        let user_page = flags & PTE_U != 0;
        match mode {
            Mode::User if !user_page => return Err(()),
            // S-mode never runs code from a user page, and reads or writes
            // one only when SUM is set.
            Mode::Supervisor if user_page && (access == Access::Fetch || !sum) => return Err(()),
            _ => {}
        }
        let allowed = match access {
            Access::Fetch => flags & PTE_X != 0,
            Access::Load => flags & PTE_R != 0 || (mxr && flags & PTE_X != 0),
            Access::Store => flags & PTE_W != 0,
        };
        if allowed { Ok(()) } else { Err(()) }
    }

    /// Walks the page table for `vaddr`, and gives the TLB entry of its
    /// 4 KiB page.
    fn walk(&mut self, vaddr: u64, access: Access) -> Result<TlbEntry, Exception> {
        let mut table = (self.csr.satp & ((1 << 44) - 1)) * PAGE_SIZE;
        for level in (0..LEVELS).rev() {
            let index = (vaddr >> (12 + 9 * level)) & 0x1ff;
            let pte_addr = table + index * 8;
            // The PMP checks the reads of the walk as S-mode reads.
            if !self
                .csr
                .pmp
                .allows(pte_addr, 8, Access::Load, Mode::Supervisor)
            {
                return Err(access_fault(access, vaddr));
            }
            let pte = self
                .bus
                .read(pte_addr, 8)
                .map_err(|_| access_fault(access, vaddr))?;

            let valid = pte & PTE_V != 0;
            let write_only = pte & (PTE_R | PTE_W) == PTE_W;
            if !valid || write_only || pte & PTE_RESERVED != 0 {
                return Err(page_fault(access, vaddr));
            }
            let ppn = (pte >> 10) & ((1 << 44) - 1);
            if pte & (PTE_R | PTE_X) == 0 {
                // A pointer to the next level. The last level has none.
                if level == 0 {
                    return Err(page_fault(access, vaddr));
                }
                table = ppn * PAGE_SIZE;
                continue;
            }

            // A leaf. A superpage must be aligned to its own size: the low
            // page numbers in its PPN must be zero.
            let low_ppn_mask = (1u64 << (9 * level)) - 1;
            if ppn & low_ppn_mask != 0 {
                return Err(page_fault(access, vaddr));
            }

            // The permission check comes before the update of A and D, so a
            // fault leaves the PTE as it was.
            let mode = match access {
                Access::Fetch => self.mode,
                _ => self.data_mode(),
            };
            self.check_permission(pte, access, mode)
                .map_err(|()| page_fault(access, vaddr))?;

            let mut new_pte = pte | PTE_A;
            if access == Access::Store {
                new_pte |= PTE_D;
            }
            if new_pte != pte {
                if !self
                    .csr
                    .pmp
                    .allows(pte_addr, 8, Access::Store, Mode::Supervisor)
                {
                    return Err(access_fault(access, vaddr));
                }
                self.bus
                    .write(pte_addr, 8, new_pte)
                    .map_err(|_| access_fault(access, vaddr))?;
            }

            // For a superpage, the low page numbers come from the address.
            let vpn = vaddr >> 12;
            let page = ((ppn & !low_ppn_mask) | (vpn & low_ppn_mask)) * PAGE_SIZE;
            return Ok(TlbEntry {
                valid: true,
                vpn: vpn & ((1 << 27) - 1),
                page,
                flags: new_pte & 0xff,
            });
        }
        Err(page_fault(access, vaddr))
    }
}
