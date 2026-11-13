//! The core of hartwood: an RV64GC hart, its MMU, the bus and the devices of
//! the QEMU `virt` machine.
//!
//! The core does no I/O. It reads no file, no clock and no terminal. The
//! host gives it the bytes of each image, and takes the bytes that the
//! devices write. The same core builds for a native host and for
//! `wasm32-unknown-unknown`.

pub mod asm;
pub mod bus;
pub mod compressed;
pub mod cpu;
pub mod csr;
pub mod decode;
pub mod devices;
pub mod elf;
pub mod fdt;
pub mod htif;
pub mod pmp;
pub mod ram;
pub mod softfloat;
pub mod trap;
