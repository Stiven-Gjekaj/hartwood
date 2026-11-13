//! Runs the programs of the riscv-tests suite on hartwood.

use std::path::{Path, PathBuf};

use hartwood_core::bus::Bus;
use hartwood_core::cpu::{Cpu, Stop};
use hartwood_core::elf::Elf;

/// The RAM of each test. The tests use less than one MiB.
const RAM_SIZE: usize = 4 << 20;

/// A test that runs longer than this is stuck. The longest test of the suite
/// runs fewer than one million instructions.
const STEP_LIMIT: u64 = 50_000_000;

/// The result of one test program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    /// The program reported that the test with this number failed.
    Fail(u64),
    /// The program did not finish in [`STEP_LIMIT`] instructions.
    Timeout,
    /// The emulator could not continue.
    Error(String),
}

impl Outcome {
    pub fn passed(&self) -> bool {
        *self == Outcome::Pass
    }
}

/// Runs one test program.
pub fn run_file(path: &Path) -> Outcome {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    run_bytes(&bytes)
}

/// Runs one test program from its bytes.
pub fn run_bytes(bytes: &[u8]) -> Outcome {
    let elf = match Elf::parse(bytes) {
        Ok(e) => e,
        Err(e) => return Outcome::Error(e.to_string()),
    };
    let mut cpu = Cpu::new(Bus::new(RAM_SIZE));
    if cpu.load_elf(&elf).is_err() {
        return Outcome::Error("a segment is outside the RAM".into());
    }
    match cpu.run(STEP_LIMIT) {
        Stop::Exit(0) => Outcome::Pass,
        Stop::Exit(n) => Outcome::Fail(n),
        Stop::StepLimit | Stop::Idle | Stop::Reset => Outcome::Timeout,
    }
}

/// Lists the test programs in `dir` whose names start with one of
/// `prefixes`, in name order. A `.dump` file is a listing, not a program.
pub fn find_tests(dir: &Path, prefixes: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("rv64") || name.ends_with(".dump") || !path.is_file() {
            continue;
        }
        if prefixes.iter().any(|p| name.starts_with(p.as_str())) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}
