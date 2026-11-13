//! The host-target interface (HTIF) of the riscv-tests programs.
//!
//! A test program has two 64-bit words in RAM, `tohost` and `fromhost`. It
//! writes a command to `tohost`, and the host answers in `fromhost`. Spike
//! uses this interface, and the riscv-tests environments expect it.
//!
//! The command format: bits 63..56 name the device, bits 55..48 name the
//! command, and bits 47..0 hold the payload.
//!
//! - Device 0, command 0, payload with bit 0 set: the program exits with the
//!   code `payload >> 1`. A code of 0 means that each test passed. Any other
//!   code is the number of the test that failed.
//! - Device 0, command 0, payload with bit 0 clear: the payload is the
//!   address of a system call block. Only `write` to standard output is
//!   served.
//! - Device 1, command 1: write the low byte of the payload to the console.

/// The number of the `write` system call in the proxy kernel.
const SYS_WRITE: u64 = 64;

/// The state of the interface: where the two words are, and what the program
/// asked for.
#[derive(Debug, Default, Clone)]
pub struct Htif {
    /// The address of `tohost`, from the symbol table of the program.
    pub tohost: Option<u64>,
    /// The address of `fromhost`, from the symbol table of the program.
    pub fromhost: Option<u64>,
    /// The exit code, after the program asked to exit.
    pub exit_code: Option<u64>,
    /// The bytes that the program wrote to the console.
    pub output: Vec<u8>,
}

/// What the host must do after it reads a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtifAction {
    /// Nothing more.
    None,
    /// Read a system call block of eight words at this address, then call
    /// [`Htif::syscall`] with it.
    Syscall(u64),
}

impl Htif {
    /// Tells if a store of `width` bytes at `addr` touches `tohost`.
    pub fn touches_tohost(&self, addr: u64, width: usize) -> bool {
        match self.tohost {
            Some(t) => addr < t + 8 && addr + width as u64 > t,
            None => false,
        }
    }

    /// Reads one command. The caller then writes zero to `tohost`, which
    /// tells the program that the host took the command.
    pub fn command(&mut self, value: u64) -> HtifAction {
        let device = value >> 56;
        let cmd = (value >> 48) & 0xff;
        let payload = value & 0xffff_ffff_ffff;
        match (device, cmd) {
            (0, 0) if payload & 1 == 1 => {
                self.exit_code = Some(payload >> 1);
                HtifAction::None
            }
            (0, 0) => HtifAction::Syscall(payload),
            (1, 1) => {
                self.output.push(payload as u8);
                HtifAction::None
            }
            _ => HtifAction::None,
        }
    }

    /// Serves a system call. `block` holds the number and the arguments, and
    /// `read` gives the bytes of a buffer in memory. Returns the value for
    /// `fromhost`.
    pub fn syscall(&mut self, block: [u64; 8], read: impl Fn(u64, u64) -> Option<Vec<u8>>) -> u64 {
        if block[0] == SYS_WRITE
            && block[1] == 1
            && let Some(bytes) = read(block[2], block[3])
        {
            self.output.extend_from_slice(&bytes);
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_odd_payload_is_an_exit_code() {
        let mut htif = Htif::default();
        assert_eq!(htif.command((3 << 1) | 1), HtifAction::None);
        assert_eq!(htif.exit_code, Some(3));
    }

    #[test]
    fn one_is_a_pass() {
        let mut htif = Htif::default();
        htif.command(1);
        assert_eq!(htif.exit_code, Some(0));
    }

    #[test]
    fn an_even_payload_is_a_system_call() {
        let mut htif = Htif::default();
        assert_eq!(htif.command(0x8000_2000), HtifAction::Syscall(0x8000_2000));
        let back = htif.syscall([SYS_WRITE, 1, 0x100, 2, 0, 0, 0, 0], |addr, len| {
            assert_eq!((addr, len), (0x100, 2));
            Some(b"ok".to_vec())
        });
        assert_eq!(back, 1);
        assert_eq!(htif.output, b"ok");
        assert_eq!(htif.exit_code, None);
    }

    #[test]
    fn the_console_device_writes_one_byte() {
        let mut htif = Htif::default();
        htif.command(0x0101_0000_0000_0041);
        assert_eq!(htif.output, b"A");
    }

    #[test]
    fn only_a_store_that_overlaps_tohost_touches_it() {
        let htif = Htif {
            tohost: Some(0x1000),
            ..Htif::default()
        };
        assert!(htif.touches_tohost(0x1000, 4));
        assert!(htif.touches_tohost(0x1004, 4));
        assert!(htif.touches_tohost(0x0ffc, 8));
        assert!(!htif.touches_tohost(0x1008, 8));
        assert!(!htif.touches_tohost(0x0ff8, 8));
    }
}
