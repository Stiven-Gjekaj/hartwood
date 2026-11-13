//! The PLIC: it routes the interrupt lines of the devices to the hart.
//!
//! The hart has two contexts: 0 is M-mode and 1 is S-mode. Each source has a
//! priority, and each context has an enable bit for each source and a
//! threshold. A context gets an interrupt while one enabled source is
//! pending with a priority above its threshold.
//!
//! Each line is level triggered. A source that a context claimed is not
//! pending again until the context completes it.

/// The number of sources, with source 0, which does not exist.
pub const SOURCES: usize = 32;
pub const CONTEXTS: usize = 2;

const PRIORITY: u64 = 0x00_0000;
const PENDING: u64 = 0x00_1000;
const ENABLE: u64 = 0x00_2000;
const ENABLE_STRIDE: u64 = 0x80;
const CONTEXT: u64 = 0x20_0000;
const CONTEXT_STRIDE: u64 = 0x1000;

/// The size of the register block.
pub const SIZE: u64 = 0x400_0000;

#[derive(Debug, Clone, Default)]
pub struct Plic {
    priority: [u32; SOURCES],
    /// The level of each line, as the device drives it.
    level: u32,
    pending: u32,
    /// The sources that a context claimed and did not complete.
    in_service: u32,
    enable: [u32; CONTEXTS],
    threshold: [u32; CONTEXTS],
}

impl Plic {
    /// Sets the level of the line of source `irq`.
    pub fn set_level(&mut self, irq: usize, high: bool) {
        let bit = 1u32 << irq;
        if high {
            self.level |= bit;
            if self.in_service & bit == 0 {
                self.pending |= bit;
            }
        } else {
            self.level &= !bit;
            self.pending &= !bit;
        }
    }

    /// The best source for a context: the enabled pending source with the
    /// highest priority above the threshold. A tie goes to the lower number.
    fn best(&self, ctx: usize) -> Option<usize> {
        let candidates = self.pending & self.enable[ctx];
        let mut best: Option<(usize, u32)> = None;
        for irq in 1..SOURCES {
            if candidates & (1 << irq) == 0 {
                continue;
            }
            let p = self.priority[irq];
            if p > self.threshold[ctx] && best.is_none_or(|(_, bp)| p > bp) {
                best = Some((irq, p));
            }
        }
        best.map(|(irq, _)| irq)
    }

    /// Tells if a context has an interrupt to take.
    pub fn interrupt(&self, ctx: usize) -> bool {
        self.best(ctx).is_some()
    }

    fn claim(&mut self, ctx: usize) -> u32 {
        match self.best(ctx) {
            Some(irq) => {
                self.pending &= !(1 << irq);
                self.in_service |= 1 << irq;
                irq as u32
            }
            None => 0,
        }
    }

    fn complete(&mut self, irq: u32) {
        let Some(bit) = 1u32.checked_shl(irq) else {
            return;
        };
        self.in_service &= !bit;
        if self.level & bit != 0 {
            self.pending |= bit;
        }
    }

    /// Reads a 32-bit register. A read of the claim register claims.
    pub fn read(&mut self, offset: u64, width: usize) -> Option<u64> {
        if width != 4 {
            return None;
        }
        let v = match offset {
            o if (PRIORITY..PENDING).contains(&o) => *self.priority.get((o / 4) as usize)?,
            o if o == PENDING => self.pending,
            o if (PENDING + 4..ENABLE).contains(&o) => 0,
            o if (ENABLE..CONTEXT).contains(&o) => {
                let ctx = ((o - ENABLE) / ENABLE_STRIDE) as usize;
                let word = (o - ENABLE) % ENABLE_STRIDE;
                if ctx >= CONTEXTS {
                    return None;
                }
                if word == 0 { self.enable[ctx] } else { 0 }
            }
            o if o >= CONTEXT => {
                let ctx = ((o - CONTEXT) / CONTEXT_STRIDE) as usize;
                if ctx >= CONTEXTS {
                    return None;
                }
                match (o - CONTEXT) % CONTEXT_STRIDE {
                    0 => self.threshold[ctx],
                    4 => self.claim(ctx),
                    _ => 0,
                }
            }
            _ => return None,
        };
        Some(u64::from(v))
    }

    pub fn write(&mut self, offset: u64, width: usize, value: u64) -> Option<()> {
        if width != 4 {
            return None;
        }
        let v = value as u32;
        match offset {
            o if (PRIORITY..PENDING).contains(&o) => {
                let irq = (o / 4) as usize;
                if irq > 0 && irq < SOURCES {
                    self.priority[irq] = v & 7;
                }
            }
            o if (PENDING..ENABLE).contains(&o) => {}
            o if (ENABLE..CONTEXT).contains(&o) => {
                let ctx = ((o - ENABLE) / ENABLE_STRIDE) as usize;
                if ctx >= CONTEXTS {
                    return None;
                }
                if (o - ENABLE).is_multiple_of(ENABLE_STRIDE) {
                    // Source 0 does not exist.
                    self.enable[ctx] = v & !1;
                }
            }
            o if o >= CONTEXT => {
                let ctx = ((o - CONTEXT) / CONTEXT_STRIDE) as usize;
                if ctx >= CONTEXTS {
                    return None;
                }
                match (o - CONTEXT) % CONTEXT_STRIDE {
                    0 => self.threshold[ctx] = v & 7,
                    4 => self.complete(v),
                    _ => {}
                }
            }
            _ => return None,
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S_CLAIM: u64 = CONTEXT + CONTEXT_STRIDE + 4;

    fn plic_with_uart_enabled() -> Plic {
        let mut p = Plic::default();
        p.write(PRIORITY + 4 * 10, 4, 1).unwrap();
        p.write(ENABLE + ENABLE_STRIDE, 4, 1 << 10).unwrap();
        p
    }

    #[test]
    fn an_enabled_source_reaches_its_context_only() {
        let mut p = plic_with_uart_enabled();
        p.set_level(10, true);
        assert!(p.interrupt(1));
        assert!(!p.interrupt(0));
    }

    #[test]
    fn a_claimed_source_waits_for_its_completion() {
        let mut p = plic_with_uart_enabled();
        p.set_level(10, true);
        assert_eq!(p.read(S_CLAIM, 4), Some(10));
        assert!(!p.interrupt(1));
        assert_eq!(p.read(S_CLAIM, 4), Some(0));
        // The line is still high, so the completion makes it pending again.
        p.write(S_CLAIM, 4, 10).unwrap();
        assert!(p.interrupt(1));
        p.set_level(10, false);
        assert!(!p.interrupt(1));
    }

    #[test]
    fn the_threshold_masks_a_low_priority() {
        let mut p = plic_with_uart_enabled();
        p.set_level(10, true);
        p.write(CONTEXT + CONTEXT_STRIDE, 4, 1).unwrap();
        assert!(!p.interrupt(1));
    }

    #[test]
    fn the_highest_priority_is_claimed_first() {
        let mut p = plic_with_uart_enabled();
        p.write(PRIORITY + 4, 4, 5).unwrap();
        p.write(ENABLE + ENABLE_STRIDE, 4, (1 << 10) | (1 << 1))
            .unwrap();
        p.set_level(10, true);
        p.set_level(1, true);
        assert_eq!(p.read(S_CLAIM, 4), Some(1));
        assert_eq!(p.read(S_CLAIM, 4), Some(10));
    }
}
