//! The WebAssembly face of hartwood. The web frontend runs one `Emulator`
//! in a Web Worker, and calls it through these methods.
//!
//! A 64-bit value crosses to JavaScript as a `BigInt`. A list of them
//! crosses as a `BigUint64Array`.

use hartwood_core::boot::{BootImages, boot};
use hartwood_core::bus::Bus;
use hartwood_core::cpu::{Cpu, Stop};
use hartwood_core::decode::{decode, decode_compressed};
use hartwood_core::disasm::{csr_name, disassemble};
use wasm_bindgen::prelude::*;

/// The CSRs that the debugger shows, in this order.
const SHOWN_CSRS: [u16; 20] = [
    0x300, 0x305, 0x341, 0x342, 0x343, 0x304, 0x344, 0x302, 0x303, 0x100, 0x105, 0x141, 0x142,
    0x143, 0x180, 0x14d, 0x30a, 0x003, 0xc01, 0xc02,
];

/// Why a call to `run` returned, as a number for JavaScript.
#[wasm_bindgen]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    StepLimit = 0,
    Exit = 1,
    Reset = 2,
    Idle = 3,
    Breakpoint = 4,
}

#[wasm_bindgen]
pub struct Emulator {
    cpu: Cpu,
    breakpoints: Vec<u64>,
    exit_code: Option<u64>,
}

#[wasm_bindgen]
impl Emulator {
    /// Makes a machine with `ram_mib` MiB of RAM.
    #[wasm_bindgen(constructor)]
    pub fn new(ram_mib: u32) -> Emulator {
        Emulator {
            cpu: Cpu::new(Bus::new((ram_mib as usize) << 20)),
            breakpoints: Vec::new(),
            exit_code: None,
        }
    }

    /// Loads the images and gets the hart ready at the entry. `kernel` and
    /// `initrd` can be empty.
    pub fn boot(
        &mut self,
        firmware: Vec<u8>,
        kernel: Vec<u8>,
        initrd: Vec<u8>,
        bootargs: String,
    ) -> Result<(), JsError> {
        let images = BootImages {
            firmware,
            kernel: (!kernel.is_empty()).then_some(kernel),
            initrd: (!initrd.is_empty()).then_some(initrd),
            bootargs,
            dtb: None,
        };
        boot(&mut self.cpu, &images).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Attaches a disk image to the virtio block device.
    pub fn set_disk(&mut self, disk: Vec<u8>) {
        self.cpu.bus.virtio.disk = disk;
    }

    /// Runs at most `max_steps` steps, and stops early at a breakpoint.
    pub fn run(&mut self, max_steps: u32) -> StopReason {
        match self
            .cpu
            .run_with_breakpoints(u64::from(max_steps), &self.breakpoints)
        {
            Stop::StepLimit => StopReason::StepLimit,
            Stop::Exit(code) => {
                self.exit_code = Some(code);
                StopReason::Exit
            }
            Stop::Reset => StopReason::Reset,
            Stop::Idle => StopReason::Idle,
            Stop::Breakpoint => StopReason::Breakpoint,
        }
    }

    /// Runs one step.
    pub fn step(&mut self) {
        self.cpu.run(1);
    }

    /// The exit code, after a run that returned `Exit`.
    pub fn exit_code(&self) -> Option<u64> {
        self.exit_code
    }

    /// Gives the bytes that a person typed to the UART.
    pub fn push_input(&mut self, bytes: &[u8]) {
        self.cpu.bus.push_input(bytes);
    }

    /// Takes the bytes that the program wrote to the console since the last
    /// call.
    pub fn take_output(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.cpu.bus.uart.output);
        out.append(&mut self.cpu.bus.htif.output);
        out
    }

    pub fn set_breakpoints(&mut self, addresses: Vec<u64>) {
        self.breakpoints = addresses;
    }

    pub fn pc(&self) -> u64 {
        self.cpu.pc
    }

    /// The privilege mode: 0 for U, 1 for S, 3 for M.
    pub fn mode(&self) -> u8 {
        self.cpu.mode as u8
    }

    pub fn instret(&self) -> u64 {
        self.cpu.instret
    }

    pub fn cycle(&self) -> u64 {
        self.cpu.cycle
    }

    /// The 32 integer registers.
    pub fn x_registers(&self) -> Vec<u64> {
        self.cpu.x.to_vec()
    }

    /// The 32 floating point registers, as raw bits.
    pub fn f_registers(&self) -> Vec<u64> {
        self.cpu.f.to_vec()
    }

    /// The names of the CSRs that `csr_values` gives, one on each line.
    pub fn csr_names(&self) -> String {
        SHOWN_CSRS
            .iter()
            .map(|&a| csr_name(a).unwrap_or("?"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The values of the CSRs that `csr_names` names.
    pub fn csr_values(&self) -> Vec<u64> {
        let ctx = hartwood_core::csr::CsrContext {
            mode: hartwood_core::csr::Mode::Machine,
            cycle: self.cpu.cycle,
            instret: self.cpu.instret,
            time: self.cpu.bus.clint.mtime,
        };
        SHOWN_CSRS
            .iter()
            .map(|&a| self.cpu.csr.read(a, &ctx).unwrap_or(0))
            .collect()
    }

    /// Reads RAM for the memory view. A byte that is not in RAM, or not
    /// mapped, reads as 0, and `memory_valid` tells which ones those are.
    pub fn memory(&self, addr: u64, len: u32, virtual_address: bool) -> Vec<u8> {
        self.cpu
            .peek(addr, len as usize, virtual_address)
            .into_iter()
            .map(|b| b.unwrap_or(0))
            .collect()
    }

    /// One byte for each byte of `memory`: 1 if it is readable.
    pub fn memory_valid(&self, addr: u64, len: u32, virtual_address: bool) -> Vec<u8> {
        self.cpu
            .peek(addr, len as usize, virtual_address)
            .into_iter()
            .map(|b| u8::from(b.is_some()))
            .collect()
    }

    /// Disassembles `count` instructions from `addr`, a virtual address in
    /// the current mode. Each line is "address<TAB>bits<TAB>text".
    pub fn disassemble(&self, addr: u64, count: u32) -> String {
        let mut lines = Vec::new();
        let mut pc = addr;
        for _ in 0..count {
            let bytes = self.cpu.peek(pc, 4, true);
            let (Some(b0), Some(b1)) = (bytes[0], bytes[1]) else {
                lines.push(format!("{pc:016x}\t????\t(not mapped)"));
                pc += 2;
                continue;
            };
            let low = u16::from_le_bytes([b0, b1]);
            if low & 3 != 3 {
                let inst = decode_compressed(low);
                lines.push(format!("{pc:016x}\t{low:04x}\t{}", disassemble(&inst, pc)));
                pc += 2;
            } else {
                let high = u16::from_le_bytes([bytes[2].unwrap_or(0), bytes[3].unwrap_or(0)]);
                let raw = (u32::from(high) << 16) | u32::from(low);
                let inst = decode(raw);
                lines.push(format!("{pc:016x}\t{raw:08x}\t{}", disassemble(&inst, pc)));
                pc += 4;
            }
        }
        lines.join("\n")
    }
}
