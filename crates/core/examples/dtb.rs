//! Writes the device tree of the machine to standard output, so `dtc` can
//! check it:
//!
//!     cargo run -p hartwood-core --example dtb > virt.dtb
//!     dtc -I dtb -O dts virt.dtb

use std::io::Write;

use hartwood_core::fdt::{BootConfig, virt_dtb};

fn main() -> std::io::Result<()> {
    let dtb = virt_dtb(&BootConfig {
        ram_size: 128 << 20,
        bootargs: "console=ttyS0".into(),
        initrd: None,
    });
    std::io::stdout().write_all(&dtb)
}
