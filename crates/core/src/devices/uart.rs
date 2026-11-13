//! A 16550 UART, the console of the `virt` machine.
//!
//! The host puts the bytes that a person types into the receive queue, and
//! takes the bytes that the program writes. A byte that the program writes
//! leaves at once, so the transmit register is always empty.

use std::collections::VecDeque;

const RBR_THR: u64 = 0;
const IER: u64 = 1;
const IIR_FCR: u64 = 2;
const LCR: u64 = 3;
const MCR: u64 = 4;
const LSR: u64 = 5;
const MSR: u64 = 6;
const SCR: u64 = 7;

const IER_RX: u8 = 1 << 0;
const IER_THRE: u8 = 1 << 1;
const LCR_DLAB: u8 = 1 << 7;
const LSR_DR: u8 = 1 << 0;
const LSR_THRE: u8 = 1 << 5;
const LSR_TEMT: u8 = 1 << 6;

const IIR_NONE: u8 = 0x01;
const IIR_THRE: u8 = 0x02;
const IIR_RX: u8 = 0x04;
/// IIR bits 7..6 say that the FIFOs are on.
const IIR_FIFO: u8 = 0xc0;

/// The size of the register block.
pub const SIZE: u64 = 0x100;

#[derive(Debug, Clone, Default)]
pub struct Uart {
    rx: VecDeque<u8>,
    /// The bytes that the program wrote, for the host to take.
    pub output: Vec<u8>,
    ier: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    fifo: bool,
    divisor: u16,
    /// The interrupt for an empty transmit register waits to be read in
    /// IIR.
    thre_pending: bool,
}

impl Uart {
    /// Gives bytes from the host to the receive queue.
    pub fn push_input(&mut self, bytes: &[u8]) {
        self.rx.extend(bytes);
    }

    /// Tells if the receive queue has a byte that the program did not read.
    pub fn has_input(&self) -> bool {
        !self.rx.is_empty()
    }

    /// Tells if the program turned on the interrupt for received bytes. A
    /// driver does that when it is ready to read, so a host can hold its
    /// scripted input until then.
    pub fn receive_enabled(&self) -> bool {
        self.ier & IER_RX != 0
    }

    /// The level of the interrupt line.
    pub fn interrupt(&self) -> bool {
        (self.ier & IER_RX != 0 && !self.rx.is_empty())
            || (self.ier & IER_THRE != 0 && self.thre_pending)
    }

    fn iir(&self) -> u8 {
        let fifo = if self.fifo { IIR_FIFO } else { 0 };
        let id = if self.ier & IER_RX != 0 && !self.rx.is_empty() {
            IIR_RX
        } else if self.ier & IER_THRE != 0 && self.thre_pending {
            IIR_THRE
        } else {
            IIR_NONE
        };
        fifo | id
    }

    pub fn read(&mut self, offset: u64, width: usize) -> Option<u64> {
        if width != 1 {
            return None;
        }
        let dlab = self.lcr & LCR_DLAB != 0;
        let v = match offset {
            RBR_THR if dlab => self.divisor as u8,
            RBR_THR => self.rx.pop_front().unwrap_or(0),
            IER if dlab => (self.divisor >> 8) as u8,
            IER => self.ier,
            IIR_FCR => {
                let iir = self.iir();
                // A read that reports the empty transmit register clears it.
                if iir & 0x0f == IIR_THRE {
                    self.thre_pending = false;
                }
                iir
            }
            LCR => self.lcr,
            MCR => self.mcr,
            LSR => {
                let dr = if self.rx.is_empty() { 0 } else { LSR_DR };
                dr | LSR_THRE | LSR_TEMT
            }
            // CTS, DSR and DCD are on: a cable is connected.
            MSR => 0xb0,
            SCR => self.scr,
            _ => return None,
        };
        Some(u64::from(v))
    }

    pub fn write(&mut self, offset: u64, width: usize, value: u64) -> Option<()> {
        if width != 1 {
            return None;
        }
        let v = value as u8;
        let dlab = self.lcr & LCR_DLAB != 0;
        match offset {
            RBR_THR if dlab => self.divisor = (self.divisor & 0xff00) | u16::from(v),
            RBR_THR => {
                self.output.push(v);
                self.thre_pending = true;
            }
            IER if dlab => self.divisor = (self.divisor & 0x00ff) | (u16::from(v) << 8),
            IER => {
                // Turning on the transmit interrupt with an empty register
                // raises it at once, as a real 16550 does.
                if v & IER_THRE != 0 && self.ier & IER_THRE == 0 {
                    self.thre_pending = true;
                }
                self.ier = v & 0x0f;
            }
            IIR_FCR => {
                self.fifo = v & 1 != 0;
                if v & 0x02 != 0 {
                    self.rx.clear();
                }
            }
            LCR => self.lcr = v,
            MCR => self.mcr = v,
            LSR | MSR => {}
            SCR => self.scr = v,
            _ => return None,
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_byte_reaches_the_output() {
        let mut u = Uart::default();
        u.write(RBR_THR, 1, u64::from(b'h')).unwrap();
        u.write(RBR_THR, 1, u64::from(b'i')).unwrap();
        assert_eq!(u.output, b"hi");
    }

    #[test]
    fn input_sets_data_ready_and_the_receive_interrupt() {
        let mut u = Uart::default();
        u.write(IER, 1, u64::from(IER_RX)).unwrap();
        assert!(!u.interrupt());
        u.push_input(b"ab");
        assert!(u.interrupt());
        assert_eq!(u.read(LSR, 1).unwrap() as u8 & LSR_DR, LSR_DR);
        assert_eq!(u.read(IIR_FCR, 1), Some(u64::from(IIR_RX)));
        assert_eq!(u.read(RBR_THR, 1), Some(u64::from(b'a')));
        assert_eq!(u.read(RBR_THR, 1), Some(u64::from(b'b')));
        assert!(!u.interrupt());
        assert_eq!(u.read(LSR, 1).unwrap() as u8 & LSR_DR, 0);
    }

    #[test]
    fn the_transmit_interrupt_clears_when_iir_reports_it() {
        let mut u = Uart::default();
        u.write(IER, 1, u64::from(IER_THRE)).unwrap();
        assert!(u.interrupt());
        assert_eq!(u.read(IIR_FCR, 1), Some(u64::from(IIR_THRE)));
        assert!(!u.interrupt());
        u.write(RBR_THR, 1, u64::from(b'x')).unwrap();
        assert!(u.interrupt());
    }

    #[test]
    fn dlab_selects_the_divisor() {
        let mut u = Uart::default();
        u.write(LCR, 1, u64::from(LCR_DLAB)).unwrap();
        u.write(RBR_THR, 1, 3).unwrap();
        u.write(IER, 1, 0).unwrap();
        u.write(LCR, 1, 3).unwrap();
        assert_eq!(u.divisor, 3);
        assert!(u.output.is_empty());
        assert_eq!(u.read(IER, 1), Some(0));
    }
}
