//! Puts boot images in memory and sets the hart up as a boot ROM would.
//!
//! The layout is the layout that QEMU uses for `-bios fw_jump.bin -kernel
//! Image`, so OpenSBI finds the kernel where its build expects it:
//!
//! | Address | Content |
//! |---|---|
//! | `0x8000_0000` | the firmware (OpenSBI) or an ELF program |
//! | `0x8020_0000` | the kernel, if there is one |
//! | below the device tree | the initramfs, if there is one |
//! | the last MiB of RAM | the device tree |
//!
//! The hart starts in M-mode at the entry, with `a0` = 0 (the hart ID) and
//! `a1` = the address of the device tree.

use std::fmt;

use crate::bus::RAM_BASE;
use crate::cpu::Cpu;
use crate::csr::Mode;
use crate::elf::{Elf, ElfError};
use crate::fdt::{BootConfig, virt_dtb};

/// Where OpenSBI `fw_jump` jumps to: its own address plus 2 MiB on RV64.
pub const KERNEL_ADDR: u64 = RAM_BASE + 0x20_0000;

/// The space at the top of RAM for the device tree.
const DTB_SPACE: u64 = 0x10_0000;

/// The images of one boot.
#[derive(Debug, Clone, Default)]
pub struct BootImages {
    /// An ELF file, or a raw image that runs at the start of RAM.
    pub firmware: Vec<u8>,
    /// A raw kernel image for the firmware to start.
    pub kernel: Option<Vec<u8>>,
    pub initrd: Option<Vec<u8>>,
    /// The kernel command line.
    pub bootargs: String,
    /// A device tree that replaces the one that hartwood writes.
    pub dtb: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    Elf(ElfError),
    /// The images do not fit in the RAM.
    TooLarge(&'static str),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootError::Elf(e) => write!(f, "{e}"),
            BootError::TooLarge(what) => write!(f, "the {what} does not fit in the RAM"),
        }
    }
}

impl std::error::Error for BootError {}

/// Tells if the bytes start with the ELF magic number.
pub fn is_elf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF")
}

/// Loads the images, writes the device tree, and sets the registers.
pub fn boot(cpu: &mut Cpu, images: &BootImages) -> Result<(), BootError> {
    let ram_size = cpu.bus.ram.size();
    let ram_end = RAM_BASE + ram_size;

    if is_elf(&images.firmware) {
        let elf = Elf::parse(&images.firmware).map_err(BootError::Elf)?;
        cpu.load_elf(&elf)
            .map_err(|_| BootError::TooLarge("ELF program"))?;
    } else {
        cpu.bus
            .load_bytes(RAM_BASE, &images.firmware)
            .map_err(|_| BootError::TooLarge("firmware"))?;
        cpu.pc = RAM_BASE;
    }
    if let Some(kernel) = &images.kernel {
        cpu.bus
            .load_bytes(KERNEL_ADDR, kernel)
            .map_err(|_| BootError::TooLarge("kernel"))?;
    }

    let dtb_addr = ram_end
        .checked_sub(DTB_SPACE)
        .ok_or(BootError::TooLarge("device tree"))?;
    let initrd = match &images.initrd {
        Some(bytes) => {
            let start = dtb_addr
                .checked_sub(bytes.len() as u64)
                .map(|a| a & !0xfff)
                .filter(|&a| {
                    a >= KERNEL_ADDR + images.kernel.as_ref().map_or(0, |k| k.len() as u64)
                })
                .ok_or(BootError::TooLarge("initramfs"))?;
            cpu.bus
                .load_bytes(start, bytes)
                .map_err(|_| BootError::TooLarge("initramfs"))?;
            Some((start, start + bytes.len() as u64))
        }
        None => None,
    };

    let dtb = match &images.dtb {
        Some(d) => d.clone(),
        None => virt_dtb(&BootConfig {
            ram_size,
            bootargs: images.bootargs.clone(),
            initrd,
        }),
    };
    if dtb.len() as u64 > DTB_SPACE {
        return Err(BootError::TooLarge("device tree"));
    }
    cpu.bus
        .load_bytes(dtb_addr, &dtb)
        .map_err(|_| BootError::TooLarge("device tree"))?;

    cpu.mode = Mode::Machine;
    cpu.x[10] = 0;
    cpu.x[11] = dtb_addr;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    #[test]
    fn a_raw_boot_puts_each_image_in_its_place() {
        let mut cpu = Cpu::new(Bus::new(16 << 20));
        let images = BootImages {
            firmware: vec![0x11; 16],
            kernel: Some(vec![0x22; 16]),
            initrd: Some(vec![0x33; 5000]),
            bootargs: "console=ttyS0".into(),
            dtb: None,
        };
        boot(&mut cpu, &images).unwrap();
        assert_eq!(cpu.pc, RAM_BASE);
        assert_eq!(cpu.bus.read(RAM_BASE, 1), Ok(0x11));
        assert_eq!(cpu.bus.read(KERNEL_ADDR, 1), Ok(0x22));
        let dtb = cpu.x[11];
        assert_eq!(dtb, RAM_BASE + (16 << 20) - DTB_SPACE);
        assert_eq!(cpu.bus.read(dtb, 4), Ok(0xedfe_0dd0));
        // The initramfs ends below the device tree, on a page boundary.
        let start = (dtb - 5000) & !0xfff;
        assert_eq!(cpu.bus.read(start, 1), Ok(0x33));
        assert_eq!(cpu.bus.read(start + 4999, 1), Ok(0x33));
    }

    #[test]
    fn an_initramfs_that_meets_the_kernel_is_refused() {
        let mut cpu = Cpu::new(Bus::new(4 << 20));
        let images = BootImages {
            firmware: vec![0; 16],
            kernel: Some(vec![0; 1 << 20]),
            initrd: Some(vec![0; 2 << 20]),
            ..BootImages::default()
        };
        assert_eq!(
            boot(&mut cpu, &images),
            Err(BootError::TooLarge("initramfs"))
        );
    }
}
