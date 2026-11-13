//! The terminal side of the UART.
//!
//! In a terminal, the console puts it in raw mode, so each key goes to the
//! guest at once, Ctrl-C included. A thread reads the keyboard and sends
//! the bytes on a channel, so the emulator never blocks on a read.
//! Ctrl-A then X ends the run, as in QEMU.

use std::io::{IsTerminal, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use anyhow::Result;

const CTRL_A: u8 = 0x01;

pub struct Console {
    input: Option<Receiver<u8>>,
    raw: bool,
    /// The last key was Ctrl-A.
    escape: bool,
    quit: bool,
    pending: Vec<u8>,
}

impl Console {
    /// Opens the console. `keyboard` says whether to read the keyboard.
    pub fn open(keyboard: bool) -> Result<Self> {
        let stdin = std::io::stdin();
        let raw = keyboard && stdin.is_terminal();
        if raw {
            crossterm::terminal::enable_raw_mode()?;
        }
        let input = keyboard.then(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut stdin = std::io::stdin().lock();
                let mut byte = [0u8; 1];
                while let Ok(1) = stdin.read(&mut byte) {
                    if tx.send(byte[0]).is_err() {
                        break;
                    }
                }
            });
            rx
        });
        Ok(Self {
            input,
            raw,
            escape: false,
            quit: false,
            pending: Vec::new(),
        })
    }

    /// Tells if a person can type.
    pub fn interactive(&self) -> bool {
        self.input.is_some()
    }

    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Writes bytes from the guest. In raw mode a newline does not return
    /// the cursor, so each `\n` gets a `\r`, as a terminal driver adds.
    pub fn write(&mut self, out: &mut impl Write, bytes: &[u8]) -> Result<()> {
        if self.raw {
            for &b in bytes {
                if b == b'\n' {
                    out.write_all(b"\r\n")?;
                } else {
                    out.write_all(&[b])?;
                }
            }
        } else {
            out.write_all(bytes)?;
        }
        Ok(())
    }

    fn accept(&mut self, byte: u8) {
        if self.escape {
            self.escape = false;
            match byte {
                b'x' | b'X' => self.quit = true,
                // Ctrl-A twice sends one Ctrl-A.
                CTRL_A => self.pending.push(CTRL_A),
                _ => {}
            }
        } else if byte == CTRL_A {
            self.escape = true;
        } else {
            self.pending.push(byte);
        }
    }

    /// Takes the keys that came since the last call.
    pub fn take_input(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(rx) = &self.input {
            bytes.extend(rx.try_iter());
        }
        for b in bytes {
            self.accept(b);
        }
        std::mem::take(&mut self.pending)
    }

    /// Waits up to `timeout` for a key, and keeps it for `take_input`.
    pub fn wait_for_input(&mut self, timeout: Duration) {
        let byte = match &self.input {
            Some(rx) => match rx.recv_timeout(timeout) {
                Ok(b) => Some(b),
                Err(RecvTimeoutError::Timeout) => None,
                // The input ended. Wait so the loop does not spin.
                Err(RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(timeout);
                    None
                }
            },
            None => None,
        };
        if let Some(b) = byte {
            self.accept(b);
        }
    }

    /// Gives the terminal back as it was.
    pub fn close(&mut self) {
        if self.raw {
            let _ = crossterm::terminal::disable_raw_mode();
            self.raw = false;
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.close();
    }
}
