//! The test device of the `virt` machine (`sifive,test0`). A write of
//! 0x5555 turns the machine off, and a write of 0x3333 resets it. OpenSBI
//! uses it for the SBI system reset call, so `poweroff` in Linux stops the
//! emulator.

/// The size of the register block.
pub const SIZE: u64 = 0x1000;

const FAIL: u64 = 0x3333;
const PASS: u64 = 0x5555;
const RESET: u64 = 0x7777;

/// What the program asked the machine to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Stop, with this exit code.
    PowerOff(u64),
    Reset,
}

#[derive(Debug, Clone, Default)]
pub struct Syscon {
    pub request: Option<Request>,
}

impl Syscon {
    pub fn read(&self, offset: u64, _width: usize) -> Option<u64> {
        (offset < SIZE).then_some(0)
    }

    pub fn write(&mut self, offset: u64, _width: usize, value: u64) -> Option<()> {
        if offset != 0 {
            return Some(());
        }
        self.request = match value & 0xffff {
            PASS => Some(Request::PowerOff(0)),
            // The upper bits of a failure hold the exit code.
            FAIL => Some(Request::PowerOff((value >> 16).max(1))),
            RESET => Some(Request::Reset),
            _ => self.request,
        };
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_command_makes_its_request() {
        let mut s = Syscon::default();
        s.write(0, 4, 0x5555).unwrap();
        assert_eq!(s.request, Some(Request::PowerOff(0)));
        s.write(0, 4, (3 << 16) | 0x3333).unwrap();
        assert_eq!(s.request, Some(Request::PowerOff(3)));
        s.write(0, 4, 0x7777).unwrap();
        assert_eq!(s.request, Some(Request::Reset));
    }
}
