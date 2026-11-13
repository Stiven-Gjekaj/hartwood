//! The command line runs an ELF file and exits with the code of the program.

use std::path::PathBuf;
use std::process::Command;

use hartwood_core::asm::{addi, auipc, program, sd};
use hartwood_core::bus::RAM_BASE;
use hartwood_core::elf::builder;

/// Writes an ELF file that exits with `code` to a new temporary file.
fn exit_elf(code: i32) -> PathBuf {
    let text = [auipc(5, 0x1000), addi(6, 0, (code << 1) | 1), sd(6, 5, 0)];
    let bytes = builder::build(RAM_BASE, &program(&text), &[("tohost", RAM_BASE + 0x1000)]);
    let path =
        std::env::temp_dir().join(format!("hartwood-exit-{code}-{}.elf", std::process::id()));
    std::fs::write(&path, bytes).expect("the temporary directory is writable");
    path
}

#[test]
fn run_exits_with_the_code_of_the_program() {
    let path = exit_elf(7);
    let status = Command::new(env!("CARGO_BIN_EXE_hartwood"))
        .arg("run")
        .arg(&path)
        .status()
        .expect("hartwood starts");
    std::fs::remove_file(&path).ok();
    assert_eq!(status.code(), Some(7));
}

#[test]
fn trace_prints_each_instruction() {
    let path = exit_elf(0);
    let out = Command::new(env!("CARGO_BIN_EXE_hartwood"))
        .args(["run", "--trace"])
        .arg(&path)
        .output()
        .expect("hartwood starts");
    std::fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).expect("the trace is text");
    assert_eq!(text.lines().count(), 3);
    assert!(text.starts_with("0000000080000000 00001297"));
}
