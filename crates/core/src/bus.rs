//! The physical address space.
//!
//! The memory map is the map of the QEMU `virt` machine, so an OpenSBI, an
//! xv6 or a Linux that is built for `virt` runs with no change. The bus
//! dispatches an access with one `match` on the address ranges. The map is
//! fixed, so a table of devices that a program can change at run time gives
//! nothing.

use crate::devices::clint::{self, Clint};
use crate::devices::plic::{self, Plic};
use crate::devices::syscon::{self, Syscon};
use crate::devices::uart::{self, Uart};
use crate::devices::virtio::{self, VirtioBlock};
use crate::htif::{Htif, HtifAction};
use crate::ram::Ram;

pub const SYSCON_BASE: u64 = 0x0010_0000;
pub const CLINT_BASE: u64 = 0x0200_0000;
pub const PLIC_BASE: u64 = 0x0c00_0000;
pub const UART_BASE: u64 = 0x1000_0000;
pub const VIRTIO_BASE: u64 = 0x1000_1000;
/// The physical address of the first byte of RAM.
pub const RAM_BASE: u64 = 0x8000_0000;

/// The PLIC source of each device, as in the `virt` machine.
pub const VIRTIO_IRQ: usize = 1;
pub const UART_IRQ: usize = 10;

/// An access that no device answers, or that a device refuses. The CPU turns
/// it into an access fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusFault;

/// Each device on the bus.
pub struct Bus {
    pub ram: Ram,
    pub htif: Htif,
    pub syscon: Syscon,
    pub clint: Clint,
    pub plic: Plic,
    pub uart: Uart,
    pub virtio: VirtioBlock,
    /// The output of the PLIC for context 0 (M-mode) and 1 (S-mode). The
    /// bus computes it again after each change of a line or a register, so
    /// the hart can read it on each step for free.
    pub external: [bool; 2],
}

/// Tells if `addr` falls in the block of `size` bytes at `base`, and gives
/// the offset.
fn within(addr: u64, base: u64, size: u64) -> Option<u64> {
    addr.checked_sub(base).filter(|&off| off < size)
}

impl Bus {
    /// Makes a bus with `ram_size` bytes of RAM at [`RAM_BASE`].
    pub fn new(ram_size: usize) -> Self {
        Self {
            ram: Ram::new(ram_size),
            htif: Htif::default(),
            syscon: Syscon::default(),
            clint: Clint::default(),
            plic: Plic::default(),
            uart: Uart::default(),
            virtio: VirtioBlock::default(),
            external: [false; 2],
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
        let value = if let Some(off) = within(addr, CLINT_BASE, clint::SIZE) {
            self.clint.read(off, width)
        } else if let Some(off) = within(addr, PLIC_BASE, plic::SIZE) {
            let v = self.plic.read(off, width);
            self.sync_interrupts();
            v
        } else if let Some(off) = within(addr, UART_BASE, uart::SIZE) {
            let v = self.uart.read(off, width);
            self.sync_interrupts();
            v
        } else if let Some(off) = within(addr, VIRTIO_BASE, virtio::SIZE) {
            self.virtio.read(off, width)
        } else if let Some(off) = within(addr, SYSCON_BASE, syscon::SIZE) {
            self.syscon.read(off, width)
        } else {
            None
        };
        value.ok_or(BusFault)
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
        let done = if let Some(off) = within(addr, CLINT_BASE, clint::SIZE) {
            self.clint.write(off, width, value)
        } else if let Some(off) = within(addr, PLIC_BASE, plic::SIZE) {
            self.plic.write(off, width, value)
        } else if let Some(off) = within(addr, UART_BASE, uart::SIZE) {
            self.uart.write(off, width, value)
        } else if let Some(off) = within(addr, VIRTIO_BASE, virtio::SIZE) {
            self.virtio.write(off, width, value, &mut self.ram)
        } else if let Some(off) = within(addr, SYSCON_BASE, syscon::SIZE) {
            self.syscon.write(off, width, value)
        } else {
            None
        };
        self.sync_interrupts();
        done.ok_or(BusFault)
    }

    /// Gives bytes that a person typed to the UART.
    pub fn push_input(&mut self, bytes: &[u8]) {
        self.uart.push_input(bytes);
        self.sync_interrupts();
    }

    /// Copies the level of each device line into the PLIC, and the output
    /// of the PLIC into [`Bus::external`].
    pub fn sync_interrupts(&mut self) {
        self.plic.set_level(UART_IRQ, self.uart.interrupt());
        self.plic.set_level(VIRTIO_IRQ, self.virtio.interrupt());
        self.external = [self.plic.interrupt(0), self.plic.interrupt(1)];
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
    fn each_device_answers_at_its_address() {
        let mut bus = Bus::new(0x1000);
        bus.write(CLINT_BASE + 0x4000, 8, 1234).unwrap();
        assert_eq!(bus.clint.mtimecmp, 1234);
        bus.write(UART_BASE, 1, u64::from(b'x')).unwrap();
        assert_eq!(bus.uart.output, b"x");
        assert_eq!(bus.read(VIRTIO_BASE, 4), Ok(0x7472_6976));
        assert_eq!(bus.read(PLIC_BASE + 0x1000, 4), Ok(0));
        assert_eq!(bus.read(0x3000_0000, 4), Err(BusFault));
    }

    #[test]
    fn a_uart_byte_reaches_the_supervisor_context_through_the_plic() {
        let mut bus = Bus::new(0x1000);
        bus.write(PLIC_BASE + 4 * UART_IRQ as u64, 4, 1).unwrap();
        bus.write(PLIC_BASE + 0x2080, 4, 1 << UART_IRQ).unwrap();
        bus.write(UART_BASE + 1, 1, 1).unwrap();
        assert_eq!(bus.external, [false, false]);
        bus.push_input(b"a");
        assert_eq!(bus.external, [false, true]);
        assert_eq!(bus.read(PLIC_BASE + 0x20_1004, 4), Ok(UART_IRQ as u64));
        assert_eq!(bus.external, [false, false]);
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
