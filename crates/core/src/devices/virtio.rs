//! A virtio block device on the MMIO transport, version 2.
//!
//! The driver puts requests in one split virtqueue, and writes the queue
//! number to `QueueNotify`. The device then runs each new request at once:
//! it reads the descriptor chain from RAM, reads or writes the disk image,
//! writes the status byte, adds the chain to the used ring, and raises its
//! interrupt.
//!
//! A request is a chain of three or more descriptors: a 16-byte header with
//! the type and the sector, the data buffers, and one status byte.

use crate::bus::RAM_BASE;
use crate::ram::Ram;

const MAGIC: u64 = 0x000;
const VERSION: u64 = 0x004;
const DEVICE_ID: u64 = 0x008;
const VENDOR_ID: u64 = 0x00c;
const DEVICE_FEATURES: u64 = 0x010;
const DEVICE_FEATURES_SEL: u64 = 0x014;
const DRIVER_FEATURES: u64 = 0x020;
const DRIVER_FEATURES_SEL: u64 = 0x024;
const QUEUE_SEL: u64 = 0x030;
const QUEUE_NUM_MAX: u64 = 0x034;
const QUEUE_NUM: u64 = 0x038;
const QUEUE_READY: u64 = 0x044;
const QUEUE_NOTIFY: u64 = 0x050;
const INTERRUPT_STATUS: u64 = 0x060;
const INTERRUPT_ACK: u64 = 0x064;
const STATUS: u64 = 0x070;
const QUEUE_DESC_LOW: u64 = 0x080;
const QUEUE_DESC_HIGH: u64 = 0x084;
const QUEUE_DRIVER_LOW: u64 = 0x090;
const QUEUE_DRIVER_HIGH: u64 = 0x094;
const QUEUE_DEVICE_LOW: u64 = 0x0a0;
const QUEUE_DEVICE_HIGH: u64 = 0x0a4;
const CONFIG_GENERATION: u64 = 0x0fc;
const CONFIG: u64 = 0x100;

/// The size of the register block.
pub const SIZE: u64 = 0x1000;

const QUEUE_SIZE_MAX: u32 = 256;
/// VIRTIO_F_VERSION_1: the device follows version 1.0 of the specification
/// or later. The only feature that it offers.
const FEATURES: u64 = 1 << 32;

const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;

const REQ_IN: u32 = 0;
const REQ_OUT: u32 = 1;
const REQ_FLUSH: u32 = 4;
const REQ_GET_ID: u32 = 8;

const STATUS_OK: u8 = 0;
const STATUS_IOERR: u8 = 1;
const STATUS_UNSUPP: u8 = 2;

const SECTOR: u64 = 512;

#[derive(Debug, Clone, Default)]
pub struct VirtioBlock {
    /// The disk image. It is empty when no disk is attached.
    pub disk: Vec<u8>,
    device_features_sel: u32,
    driver_features_sel: u32,
    driver_features: u64,
    queue_num: u32,
    queue_ready: bool,
    desc: u64,
    driver: u64,
    device: u64,
    /// The next index of the available ring that the device reads.
    last_avail: u16,
    interrupt_status: u32,
    status: u32,
}

/// One descriptor of a chain.
#[derive(Debug, Clone, Copy)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// Reads and writes RAM by physical address, for the device.
struct Dma<'a>(&'a mut Ram);

impl Dma<'_> {
    fn read(&self, addr: u64, width: usize) -> Option<u64> {
        self.0.read(addr.checked_sub(RAM_BASE)?, width)
    }

    fn write(&mut self, addr: u64, width: usize, value: u64) -> Option<()> {
        self.0.write(addr.checked_sub(RAM_BASE)?, width, value)
    }

    fn bytes(&self, addr: u64, len: usize) -> Option<&[u8]> {
        self.0.slice(addr.checked_sub(RAM_BASE)?, len)
    }

    fn bytes_mut(&mut self, addr: u64, len: usize) -> Option<&mut [u8]> {
        self.0.slice_mut(addr.checked_sub(RAM_BASE)?, len)
    }
}

impl VirtioBlock {
    /// The level of the interrupt line.
    pub fn interrupt(&self) -> bool {
        self.interrupt_status != 0
    }

    fn reset(&mut self) {
        let disk = std::mem::take(&mut self.disk);
        *self = VirtioBlock {
            disk,
            ..VirtioBlock::default()
        };
    }

    fn capacity(&self) -> u64 {
        self.disk.len() as u64 / SECTOR
    }

    pub fn read(&self, offset: u64, width: usize) -> Option<u64> {
        if offset >= CONFIG {
            // The configuration space: the capacity in sectors at 0, and the
            // block size at 20.
            let config_offset = offset - CONFIG;
            let mut config = [0u8; 24];
            config[0..8].copy_from_slice(&self.capacity().to_le_bytes());
            config[20..24].copy_from_slice(&(SECTOR as u32).to_le_bytes());
            let bytes = config.get(config_offset as usize..config_offset as usize + width)?;
            let mut buf = [0u8; 8];
            buf[..width].copy_from_slice(bytes);
            return Some(u64::from_le_bytes(buf));
        }
        if width != 4 {
            return None;
        }
        let v: u32 = match offset {
            MAGIC => 0x7472_6976,
            VERSION => 2,
            // Device 2 is a block device. With no disk, the ID 0 tells the
            // driver that no device is there.
            DEVICE_ID if self.disk.is_empty() => 0,
            DEVICE_ID => 2,
            VENDOR_ID => 0x554d_4551,
            DEVICE_FEATURES => (FEATURES >> (32 * self.device_features_sel.min(1))) as u32,
            QUEUE_NUM_MAX => QUEUE_SIZE_MAX,
            QUEUE_READY => u32::from(self.queue_ready),
            INTERRUPT_STATUS => self.interrupt_status,
            STATUS => self.status,
            CONFIG_GENERATION => 0,
            _ => 0,
        };
        Some(u64::from(v))
    }

    pub fn write(&mut self, offset: u64, width: usize, value: u64, ram: &mut Ram) -> Option<()> {
        if width != 4 {
            return None;
        }
        let v = value as u32;
        let set_low = |old: u64| (old & !0xffff_ffff) | u64::from(v);
        let set_high = |old: u64| (old & 0xffff_ffff) | (u64::from(v) << 32);
        match offset {
            DEVICE_FEATURES_SEL => self.device_features_sel = v,
            DRIVER_FEATURES_SEL => self.driver_features_sel = v,
            DRIVER_FEATURES => {
                let shift = 32 * self.driver_features_sel.min(1);
                self.driver_features &= !(0xffff_ffff << shift);
                self.driver_features |= u64::from(v) << shift;
            }
            QUEUE_SEL => {}
            QUEUE_NUM => self.queue_num = v.min(QUEUE_SIZE_MAX),
            QUEUE_READY => self.queue_ready = v & 1 == 1,
            QUEUE_NOTIFY => self.process(&mut Dma(ram)),
            INTERRUPT_ACK => self.interrupt_status &= !v,
            STATUS => {
                if v == 0 {
                    self.reset();
                } else {
                    self.status = v;
                }
            }
            QUEUE_DESC_LOW => self.desc = set_low(self.desc),
            QUEUE_DESC_HIGH => self.desc = set_high(self.desc),
            QUEUE_DRIVER_LOW => self.driver = set_low(self.driver),
            QUEUE_DRIVER_HIGH => self.driver = set_high(self.driver),
            QUEUE_DEVICE_LOW => self.device = set_low(self.device),
            QUEUE_DEVICE_HIGH => self.device = set_high(self.device),
            _ => {}
        }
        Some(())
    }

    fn read_desc(&self, dma: &Dma<'_>, index: u16) -> Option<Desc> {
        let base = self.desc + 16 * u64::from(index);
        Some(Desc {
            addr: dma.read(base, 8)?,
            len: dma.read(base + 8, 4)? as u32,
            flags: dma.read(base + 12, 2)? as u16,
            next: dma.read(base + 14, 2)? as u16,
        })
    }

    /// Runs each request that the driver made available since the last
    /// notification.
    fn process(&mut self, dma: &mut Dma<'_>) {
        if !self.queue_ready || self.queue_num == 0 {
            return;
        }
        let num = self.queue_num as u16;
        let Some(avail_idx) = dma.read(self.driver + 2, 2) else {
            return;
        };
        let avail_idx = avail_idx as u16;
        let mut used_any = false;
        while self.last_avail != avail_idx {
            let slot = self.driver + 4 + 2 * u64::from(self.last_avail % num);
            let Some(head) = dma.read(slot, 2) else {
                return;
            };
            let head = head as u16;
            let written = self.run_request(dma, head).unwrap_or(0);

            // Add the chain to the used ring.
            let Some(used_idx) = dma.read(self.device + 2, 2) else {
                return;
            };
            let used_idx = used_idx as u16;
            let elem = self.device + 4 + 8 * u64::from(used_idx % num);
            let _ = dma.write(elem, 4, u64::from(head));
            let _ = dma.write(elem + 4, 4, u64::from(written));
            let _ = dma.write(self.device + 2, 2, u64::from(used_idx.wrapping_add(1)));
            self.last_avail = self.last_avail.wrapping_add(1);
            used_any = true;
        }
        if used_any {
            self.interrupt_status |= 1;
        }
    }

    /// Runs one request. Gives the number of bytes that the device wrote to
    /// the buffers of the driver.
    fn run_request(&mut self, dma: &mut Dma<'_>, head: u16) -> Option<u32> {
        // Collect the chain. A chain longer than the queue is a loop.
        let mut chain = Vec::new();
        let mut index = head;
        loop {
            let d = self.read_desc(dma, index)?;
            chain.push(d);
            if d.flags & DESC_NEXT == 0 || chain.len() > self.queue_num as usize {
                break;
            }
            index = d.next;
        }
        let (header, rest) = chain.split_first()?;
        let (status, data) = rest.split_last()?;
        let req_type = dma.read(header.addr, 4)? as u32;
        let sector = dma.read(header.addr + 8, 8)?;

        let mut offset = sector.checked_mul(SECTOR)?;
        let mut written = 0u32;
        let mut result = STATUS_OK;
        match req_type {
            REQ_IN | REQ_OUT => {
                for d in data {
                    let len = d.len as usize;
                    let start = usize::try_from(offset).ok()?;
                    let Some(end) = start.checked_add(len).filter(|&e| e <= self.disk.len()) else {
                        result = STATUS_IOERR;
                        break;
                    };
                    if req_type == REQ_IN {
                        if d.flags & DESC_WRITE == 0 {
                            result = STATUS_IOERR;
                            break;
                        }
                        dma.bytes_mut(d.addr, len)?
                            .copy_from_slice(&self.disk[start..end]);
                        written += d.len;
                    } else {
                        let src = dma.bytes(d.addr, len)?;
                        self.disk[start..end].copy_from_slice(src);
                    }
                    offset += d.len as u64;
                }
            }
            REQ_FLUSH => {}
            REQ_GET_ID => {
                if let Some(d) = data.first() {
                    let id = b"hartwood";
                    let n = id.len().min(d.len as usize);
                    dma.bytes_mut(d.addr, n)?.copy_from_slice(&id[..n]);
                    written += n as u32;
                }
            }
            _ => result = STATUS_UNSUPP,
        }
        dma.write(status.addr, 1, u64::from(result))?;
        Some(written + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC: u64 = RAM_BASE;
    const AVAIL: u64 = RAM_BASE + 0x100;
    const USED: u64 = RAM_BASE + 0x200;
    const HEADER: u64 = RAM_BASE + 0x300;
    const DATA: u64 = RAM_BASE + 0x400;
    const STATUS_BYTE: u64 = RAM_BASE + 0x800;

    /// A device with a disk of four sectors, sector n filled with n + 1,
    /// and a queue of eight entries set up in RAM.
    fn setup() -> (VirtioBlock, Ram) {
        let mut dev = VirtioBlock {
            disk: (0..4u8).flat_map(|n| [n + 1; 512]).collect(),
            ..VirtioBlock::default()
        };
        let mut ram = Ram::new(0x1000);
        for (off, v) in [
            (QUEUE_NUM, 8),
            (QUEUE_DESC_LOW, DESC as u32),
            (QUEUE_DRIVER_LOW, AVAIL as u32),
            (QUEUE_DEVICE_LOW, USED as u32),
            (QUEUE_READY, 1),
        ] {
            dev.write(off, 4, u64::from(v), &mut ram).unwrap();
        }
        (dev, ram)
    }

    /// Writes a request of three descriptors and makes it available.
    fn submit(dev: &mut VirtioBlock, ram: &mut Ram, req_type: u32, sector: u64) {
        let mut dma = Dma(ram);
        let descs = [
            (HEADER, 16, DESC_NEXT, 1),
            (
                DATA,
                512,
                DESC_NEXT | if req_type == REQ_IN { DESC_WRITE } else { 0 },
                2,
            ),
            (STATUS_BYTE, 1, DESC_WRITE, 0),
        ];
        for (i, (addr, len, flags, next)) in descs.into_iter().enumerate() {
            let base = DESC + 16 * i as u64;
            dma.write(base, 8, addr).unwrap();
            dma.write(base + 8, 4, len).unwrap();
            dma.write(base + 12, 2, u64::from(flags)).unwrap();
            dma.write(base + 14, 2, next).unwrap();
        }
        dma.write(HEADER, 4, u64::from(req_type)).unwrap();
        dma.write(HEADER + 8, 8, sector).unwrap();
        dma.write(STATUS_BYTE, 1, 0xff).unwrap();
        let idx = dma.read(AVAIL + 2, 2).unwrap();
        dma.write(AVAIL + 4 + 2 * (idx % 8), 2, 0).unwrap();
        dma.write(AVAIL + 2, 2, idx + 1).unwrap();
        dev.write(QUEUE_NOTIFY, 4, 0, ram).unwrap();
    }

    #[test]
    fn the_identity_registers_name_a_block_device() {
        let (dev, _) = setup();
        assert_eq!(dev.read(MAGIC, 4), Some(0x7472_6976));
        assert_eq!(dev.read(VERSION, 4), Some(2));
        assert_eq!(dev.read(DEVICE_ID, 4), Some(2));
        assert_eq!(dev.read(CONFIG, 8), Some(4));
    }

    #[test]
    fn a_read_request_copies_a_sector_into_ram() {
        let (mut dev, mut ram) = setup();
        submit(&mut dev, &mut ram, REQ_IN, 2);
        let dma = Dma(&mut ram);
        assert_eq!(dma.bytes(DATA, 512).unwrap(), &[3u8; 512]);
        assert_eq!(dma.read(STATUS_BYTE, 1), Some(u64::from(STATUS_OK)));
        assert_eq!(dma.read(USED + 2, 2), Some(1));
        assert_eq!(dma.read(USED + 8, 4), Some(513));
        assert!(dev.interrupt());
    }

    #[test]
    fn a_write_request_changes_the_disk() {
        let (mut dev, mut ram) = setup();
        Dma(&mut ram).bytes_mut(DATA, 512).unwrap().fill(0xaa);
        submit(&mut dev, &mut ram, REQ_OUT, 1);
        assert_eq!(&dev.disk[512..1024], &[0xaa; 512]);
        assert_eq!(&dev.disk[0..512], &[1; 512]);
        dev.write(INTERRUPT_ACK, 4, 1, &mut ram).unwrap();
        assert!(!dev.interrupt());
    }

    #[test]
    fn a_sector_past_the_end_is_an_error() {
        let (mut dev, mut ram) = setup();
        submit(&mut dev, &mut ram, REQ_IN, 4);
        assert_eq!(
            Dma(&mut ram).read(STATUS_BYTE, 1),
            Some(u64::from(STATUS_IOERR))
        );
    }

    #[test]
    fn a_reset_keeps_the_disk() {
        let (mut dev, mut ram) = setup();
        dev.write(STATUS, 4, 0, &mut ram).unwrap();
        assert_eq!(dev.read(QUEUE_READY, 4), Some(0));
        assert_eq!(dev.disk.len(), 2048);
    }
}
