//! The physical address space.
//!
//! The memory map is the map of the QEMU `virt` machine, so an OpenSBI, an
//! xv6 or a Linux that is built for `virt` runs with no change. The bus
//! dispatches an access with one `match` on the address ranges. The map is
//! fixed, so a table of devices that a program can change at run time gives
//! nothing.

use crate::htif::{Htif, HtifAction};
use crate::ram::Ram;

/// The physical address of the first byte of RAM.
pub const RAM_BASE: u64 = 0x8000_0000;

/// An access that no device answers, or that a device refuses. The CPU turns
/// it into an access fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusFault;

/// Each device on the bus.
pub struct Bus {
    pub ram: Ram,
    pub htif: Htif,
}

impl Bus {
    /// Makes a bus with `ram_size` bytes of RAM at [`RAM_BASE`].
    pub fn new(ram_size: usize) -> Self {
        Self {
            ram: Ram::new(ram_size),
            htif: Htif::default(),
        }
    }

    /// Reads `width` bytes (1, 2, 4 or 8) at a physical address.
    ///
    /// A read takes `&mut self` because a read from a device can change it:
    /// a read of the receive register of the UART takes a byte from it.
    pub fn read(&mut self, addr: u64, width: usize) -> Result<u64, BusFault> {
        if addr >= RAM_BASE {
            return self.ram.read(addr - RAM_BASE, width).ok_or(BusFault);
        }
        Err(BusFault)
    }

    /// Writes the low `width` bytes (1, 2, 4 or 8) of `value` at a physical
    /// address.
    pub fn write(&mut self, addr: u64, width: usize, value: u64) -> Result<(), BusFault> {
        if addr >= RAM_BASE {
            self.ram
                .write(addr - RAM_BASE, width, value)
                .ok_or(BusFault)?;
            if self.htif.touches_tohost(addr, width) {
                self.serve_htif();
            }
            return Ok(());
        }
        Err(BusFault)
    }

    /// Copies bytes into RAM. The loader uses it, so it does not go through
    /// the devices.
    pub fn load_bytes(&mut self, addr: u64, bytes: &[u8]) -> Result<(), BusFault> {
        let offset = addr.checked_sub(RAM_BASE).ok_or(BusFault)?;
        let dest = self.ram.slice_mut(offset, bytes.len()).ok_or(BusFault)?;
        dest.copy_from_slice(bytes);
        Ok(())
    }

    /// Reads the command in `tohost`, serves it, and clears `tohost`.
    fn serve_htif(&mut self) {
        let Some(tohost) = self.htif.tohost else {
            return;
        };
        let value = match self.read(tohost, 8) {
            Ok(v) if v != 0 => v,
            _ => return,
        };
        let action = self.htif.command(value);
        let _ = self.write_ram(tohost, 0);
        if let HtifAction::Syscall(block_addr) = action {
            let mut block = [0u64; 8];
            for (i, word) in block.iter_mut().enumerate() {
                *word = self.read(block_addr + 8 * i as u64, 8).unwrap_or(0);
            }
            let ram = &self.ram;
            let answer = self.htif.syscall(block, |addr, len| {
                let offset = addr.checked_sub(RAM_BASE)?;
                ram.slice(offset, usize::try_from(len).ok()?)
                    .map(<[u8]>::to_vec)
            });
            if let Some(fromhost) = self.htif.fromhost {
                let _ = self.write_ram(fromhost, answer);
            }
        }
    }

    /// Writes eight bytes to RAM and does not look at `tohost`.
    fn write_ram(&mut self, addr: u64, value: u64) -> Result<(), BusFault> {
        let offset = addr.checked_sub(RAM_BASE).ok_or(BusFault)?;
        self.ram.write(offset, 8, value).ok_or(BusFault)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_starts_at_its_base_address() {
        let mut bus = Bus::new(0x1000);
        bus.write(RAM_BASE + 8, 8, 0xdead_beef).unwrap();
        assert_eq!(bus.read(RAM_BASE + 8, 8), Ok(0xdead_beef));
        assert_eq!(bus.read(RAM_BASE + 0x1000, 1), Err(BusFault));
        assert_eq!(bus.read(0x10, 4), Err(BusFault));
    }

    #[test]
    fn a_store_to_tohost_is_served_and_cleared() {
        let mut bus = Bus::new(0x1000);
        bus.htif.tohost = Some(RAM_BASE + 0x100);
        // The riscv-tests environment writes the two halves with `sw`.
        bus.write(RAM_BASE + 0x100, 4, 1).unwrap();
        assert_eq!(bus.htif.exit_code, Some(0));
        assert_eq!(bus.read(RAM_BASE + 0x100, 8), Ok(0));
    }

    #[test]
    fn a_write_system_call_reaches_the_output() {
        let mut bus = Bus::new(0x1000);
        bus.htif.tohost = Some(RAM_BASE + 0x100);
        bus.htif.fromhost = Some(RAM_BASE + 0x140);
        bus.load_bytes(RAM_BASE + 0x200, b"hi").unwrap();
        let block = RAM_BASE + 0x300;
        for (i, word) in [64, 1, RAM_BASE + 0x200, 2].iter().enumerate() {
            bus.write(block + 8 * i as u64, 8, *word).unwrap();
        }
        bus.write(RAM_BASE + 0x100, 8, block).unwrap();
        assert_eq!(bus.htif.output, b"hi");
        assert_eq!(bus.read(RAM_BASE + 0x140, 8), Ok(1));
        assert_eq!(bus.read(RAM_BASE + 0x100, 8), Ok(0));
    }
}
