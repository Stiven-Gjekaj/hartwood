//! A writer of flattened device trees (DTB), and the tree of the machine.
//!
//! OpenSBI and Linux read the devices from a device tree, not from a fixed
//! list. hartwood writes the tree from the same constants as the bus, so the
//! tree and the bus cannot disagree.

use crate::bus::{
    CLINT_BASE, PLIC_BASE, RAM_BASE, SYSCON_BASE, UART_BASE, UART_IRQ, VIRTIO_BASE, VIRTIO_IRQ,
};
use crate::devices::{clint, plic, syscon, uart, virtio};

const MAGIC: u32 = 0xd00d_feed;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const END: u32 = 9;

/// The frequency of `mtime`: 10 MHz, as in the `virt` machine.
pub const TIMEBASE_FREQUENCY: u32 = 10_000_000;

/// Builds the structure block and the strings block of a device tree.
#[derive(Default)]
pub struct FdtWriter {
    structure: Vec<u8>,
    strings: Vec<u8>,
}

impl FdtWriter {
    fn word(&mut self, v: u32) {
        self.structure.extend_from_slice(&v.to_be_bytes());
    }

    fn pad(&mut self) {
        while !self.structure.len().is_multiple_of(4) {
            self.structure.push(0);
        }
    }

    pub fn begin_node(&mut self, name: &str) {
        self.word(BEGIN_NODE);
        self.structure.extend_from_slice(name.as_bytes());
        self.structure.push(0);
        self.pad();
    }

    pub fn end_node(&mut self) {
        self.word(END_NODE);
    }

    /// The offset of a property name in the strings block. Each name is
    /// stored once.
    fn name_offset(&mut self, name: &str) -> u32 {
        let needle: Vec<u8> = name.bytes().chain([0]).collect();
        let mut start = 0;
        for part in self.strings.split_inclusive(|&b| b == 0) {
            if part == needle.as_slice() {
                return start as u32;
            }
            start += part.len();
        }
        let off = self.strings.len() as u32;
        self.strings.extend_from_slice(&needle);
        off
    }

    pub fn prop(&mut self, name: &str, value: &[u8]) {
        let off = self.name_offset(name);
        self.word(PROP);
        self.word(value.len() as u32);
        self.word(off);
        self.structure.extend_from_slice(value);
        self.pad();
    }

    pub fn prop_empty(&mut self, name: &str) {
        self.prop(name, &[]);
    }

    pub fn prop_u32(&mut self, name: &str, v: u32) {
        self.prop(name, &v.to_be_bytes());
    }

    pub fn prop_u64(&mut self, name: &str, v: u64) {
        self.prop(name, &v.to_be_bytes());
    }

    pub fn prop_cells(&mut self, name: &str, cells: &[u32]) {
        let bytes: Vec<u8> = cells.iter().flat_map(|c| c.to_be_bytes()).collect();
        self.prop(name, &bytes);
    }

    pub fn prop_str(&mut self, name: &str, v: &str) {
        self.prop_strs(name, &[v]);
    }

    /// A list of strings, each with its zero byte.
    pub fn prop_strs(&mut self, name: &str, vs: &[&str]) {
        let bytes: Vec<u8> = vs.iter().flat_map(|s| s.bytes().chain([0])).collect();
        self.prop(name, &bytes);
    }

    /// A `reg` property with two address cells and two size cells.
    pub fn prop_reg(&mut self, base: u64, size: u64) {
        let cells = [
            (base >> 32) as u32,
            base as u32,
            (size >> 32) as u32,
            size as u32,
        ];
        self.prop_cells("reg", &cells);
    }

    /// Joins the header, an empty memory reservation map, the structure
    /// and the strings into a DTB.
    pub fn finish(mut self) -> Vec<u8> {
        self.word(END);
        let header_len = 40u32;
        let rsvmap_off = header_len;
        let struct_off = rsvmap_off + 16;
        let strings_off = struct_off + self.structure.len() as u32;
        let total = strings_off + self.strings.len() as u32;
        let mut out = Vec::with_capacity(total as usize);
        for v in [
            MAGIC,
            total,
            struct_off,
            strings_off,
            rsvmap_off,
            17, // version
            16, // last compatible version
            0,  // the boot CPU
            self.strings.len() as u32,
            self.structure.len() as u32,
        ] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&self.structure);
        out.extend_from_slice(&self.strings);
        out
    }
}

/// What the device tree of a boot holds, other than the fixed devices.
#[derive(Debug, Clone, Default)]
pub struct BootConfig {
    pub ram_size: u64,
    pub bootargs: String,
    /// The first and the last-plus-one address of an initramfs in RAM.
    pub initrd: Option<(u64, u64)>,
}

const CPU_INTC: u32 = 1;
const PLIC_PHANDLE: u32 = 2;
const SYSCON_PHANDLE: u32 = 3;

/// The ISA of the hart, as OpenSBI and Linux read it.
const ISA: &str = "rv64imafdc_zicntr_zicsr_zifencei_sstc";

/// Writes the device tree of the machine.
pub fn virt_dtb(cfg: &BootConfig) -> Vec<u8> {
    let mut w = FdtWriter::default();
    w.begin_node("");
    w.prop_u32("#address-cells", 2);
    w.prop_u32("#size-cells", 2);
    w.prop_str("compatible", "riscv-virtio");
    w.prop_str("model", "hartwood,virt");

    w.begin_node("chosen");
    w.prop_str("bootargs", &cfg.bootargs);
    w.prop_str("stdout-path", &format!("/soc/serial@{UART_BASE:x}"));
    if let Some((start, end)) = cfg.initrd {
        w.prop_u64("linux,initrd-start", start);
        w.prop_u64("linux,initrd-end", end);
    }
    w.end_node();

    w.begin_node(&format!("memory@{RAM_BASE:x}"));
    w.prop_str("device_type", "memory");
    w.prop_reg(RAM_BASE, cfg.ram_size);
    w.end_node();

    w.begin_node("cpus");
    w.prop_u32("#address-cells", 1);
    w.prop_u32("#size-cells", 0);
    w.prop_u32("timebase-frequency", TIMEBASE_FREQUENCY);
    w.begin_node("cpu@0");
    w.prop_str("device_type", "cpu");
    w.prop_u32("reg", 0);
    w.prop_str("status", "okay");
    w.prop_str("compatible", "riscv");
    w.prop_str("riscv,isa", ISA);
    w.prop_str("riscv,isa-base", "rv64i");
    w.prop_strs(
        "riscv,isa-extensions",
        &[
            "i", "m", "a", "f", "d", "c", "zicntr", "zicsr", "zifencei", "sstc",
        ],
    );
    w.prop_str("mmu-type", "riscv,sv39");
    w.begin_node("interrupt-controller");
    w.prop_u32("#interrupt-cells", 1);
    w.prop_empty("interrupt-controller");
    w.prop_str("compatible", "riscv,cpu-intc");
    w.prop_u32("phandle", CPU_INTC);
    w.end_node();
    w.end_node();
    w.end_node();

    w.begin_node("soc");
    w.prop_u32("#address-cells", 2);
    w.prop_u32("#size-cells", 2);
    w.prop_str("compatible", "simple-bus");
    w.prop_empty("ranges");

    w.begin_node(&format!("clint@{CLINT_BASE:x}"));
    w.prop_strs("compatible", &["sifive,clint0", "riscv,clint0"]);
    w.prop_reg(CLINT_BASE, clint::SIZE);
    // The software interrupt (3) and the timer interrupt (7) of M-mode.
    w.prop_cells("interrupts-extended", &[CPU_INTC, 3, CPU_INTC, 7]);
    w.end_node();

    w.begin_node(&format!("plic@{PLIC_BASE:x}"));
    w.prop_strs("compatible", &["sifive,plic-1.0.0", "riscv,plic0"]);
    w.prop_reg(PLIC_BASE, plic::SIZE);
    w.prop_u32("#address-cells", 0);
    w.prop_u32("#interrupt-cells", 1);
    w.prop_empty("interrupt-controller");
    w.prop_u32("riscv,ndev", plic::SOURCES as u32 - 1);
    // Context 0 is the external interrupt of M-mode (11), context 1 of
    // S-mode (9).
    w.prop_cells("interrupts-extended", &[CPU_INTC, 11, CPU_INTC, 9]);
    w.prop_u32("phandle", PLIC_PHANDLE);
    w.end_node();

    w.begin_node(&format!("serial@{UART_BASE:x}"));
    w.prop_str("compatible", "ns16550a");
    w.prop_reg(UART_BASE, uart::SIZE);
    w.prop_u32("clock-frequency", 3_686_400);
    w.prop_u32("interrupt-parent", PLIC_PHANDLE);
    w.prop_u32("interrupts", UART_IRQ as u32);
    w.end_node();

    w.begin_node(&format!("virtio_mmio@{VIRTIO_BASE:x}"));
    w.prop_str("compatible", "virtio,mmio");
    w.prop_reg(VIRTIO_BASE, virtio::SIZE);
    w.prop_u32("interrupt-parent", PLIC_PHANDLE);
    w.prop_u32("interrupts", VIRTIO_IRQ as u32);
    w.end_node();

    w.begin_node(&format!("test@{SYSCON_BASE:x}"));
    w.prop_strs("compatible", &["sifive,test1", "sifive,test0", "syscon"]);
    w.prop_reg(SYSCON_BASE, syscon::SIZE);
    w.prop_u32("phandle", SYSCON_PHANDLE);
    w.end_node();
    w.end_node();

    w.begin_node("poweroff");
    w.prop_str("compatible", "syscon-poweroff");
    w.prop_u32("regmap", SYSCON_PHANDLE);
    w.prop_u32("offset", 0);
    w.prop_u32("value", 0x5555);
    w.end_node();

    w.begin_node("reboot");
    w.prop_str("compatible", "syscon-reboot");
    w.prop_u32("regmap", SYSCON_PHANDLE);
    w.prop_u32("offset", 0);
    w.prop_u32("value", 0x7777);
    w.end_node();

    w.end_node();
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(b: &[u8], off: usize) -> u32 {
        u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
    }

    #[test]
    fn the_header_describes_the_blocks() {
        let dtb = virt_dtb(&BootConfig {
            ram_size: 128 << 20,
            ..BootConfig::default()
        });
        assert_eq!(be32(&dtb, 0), MAGIC);
        assert_eq!(be32(&dtb, 4) as usize, dtb.len());
        let struct_off = be32(&dtb, 8) as usize;
        let strings_off = be32(&dtb, 12) as usize;
        assert_eq!(be32(&dtb, struct_off), BEGIN_NODE);
        assert_eq!(be32(&dtb, strings_off - 4), END);
        assert_eq!(be32(&dtb, 36) as usize, strings_off - struct_off);
    }

    #[test]
    fn a_property_name_is_stored_once() {
        let mut w = FdtWriter::default();
        w.begin_node("");
        w.prop_u32("reg", 1);
        w.prop_u32("reg", 2);
        w.end_node();
        let dtb = w.finish();
        let strings_off = be32(&dtb, 12) as usize;
        assert_eq!(&dtb[strings_off..], b"reg\0");
    }

    #[test]
    fn the_tree_names_the_initrd_when_there_is_one() {
        let with = virt_dtb(&BootConfig {
            ram_size: 128 << 20,
            bootargs: "console=ttyS0".into(),
            initrd: Some((0x8800_0000, 0x8810_0000)),
        });
        let text = String::from_utf8_lossy(&with);
        assert!(text.contains("linux,initrd-start"));
        assert!(text.contains("console=ttyS0"));
        let without = virt_dtb(&BootConfig::default());
        assert!(!String::from_utf8_lossy(&without).contains("linux,initrd-start"));
    }
}
