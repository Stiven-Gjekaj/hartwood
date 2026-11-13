//! `hartwood`: runs a RISC-V program or a boot image on the emulator.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hartwood_core::bus::Bus;
use hartwood_core::cpu::{Cpu, Stop};
use hartwood_core::elf::Elf;

#[derive(Parser)]
#[command(version, about = "An RV64GC emulator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs an ELF program until it writes an exit code to `tohost`.
    Run {
        /// The ELF file to run.
        image: PathBuf,
        /// Print each instruction before it runs.
        #[arg(long)]
        trace: bool,
        /// Stop after this many instructions.
        #[arg(long, default_value_t = u64::MAX)]
        max_steps: u64,
        /// The size of the RAM in MiB.
        #[arg(long, default_value_t = 128)]
        ram_mib: usize,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            image,
            trace,
            max_steps,
            ram_mib,
        } => run(&image, trace, max_steps, ram_mib),
    }
}

fn run(image: &PathBuf, trace: bool, max_steps: u64, ram_mib: usize) -> Result<ExitCode> {
    let bytes = std::fs::read(image).with_context(|| format!("cannot read {}", image.display()))?;
    let elf = Elf::parse(&bytes).with_context(|| format!("cannot load {}", image.display()))?;
    let mut cpu = Cpu::new(Bus::new(ram_mib << 20));
    cpu.load_elf(&elf)
        .map_err(|_| anyhow::anyhow!("a segment of {} is outside the RAM", image.display()))?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut left = max_steps;
    let stop = loop {
        // Trace one instruction at a time, and run in large batches when no
        // trace is asked for.
        let batch = if trace { 1 } else { left.min(100_000) };
        if trace {
            let raw = cpu.bus.read(cpu.pc, 4).unwrap_or(0);
            writeln!(out, "{:016x} {:08x}", cpu.pc, raw as u32)?;
        }
        let stop = cpu.run(batch);
        flush_console(&mut cpu, &mut out)?;
        left -= batch;
        if stop != Stop::StepLimit || left == 0 {
            break stop;
        }
    };
    out.flush()?;

    match stop {
        Stop::Exit(code) => Ok(ExitCode::from(code.min(255) as u8)),
        Stop::StepLimit => {
            eprintln!("hartwood: stopped after {} instructions", cpu.instret);
            Ok(ExitCode::from(124))
        }
    }
}

/// Writes what the program sent to the console since the last call.
fn flush_console(cpu: &mut Cpu, out: &mut impl Write) -> Result<()> {
    if !cpu.bus.htif.output.is_empty() {
        out.write_all(&cpu.bus.htif.output)?;
        cpu.bus.htif.output.clear();
    }
    Ok(())
}
