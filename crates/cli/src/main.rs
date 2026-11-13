//! `hartwood`: runs a RISC-V program or a boot image on the emulator.

mod console;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hartwood_core::boot::{BootImages, boot};
use hartwood_core::bus::Bus;
use hartwood_core::cpu::{Cpu, Stop};

use crate::console::Console;

#[derive(Parser)]
#[command(version, about = "An RV64GC emulator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs an ELF program or a kernel, with the UART on this terminal.
    Run(RunArgs),
}

#[derive(clap::Args)]
struct RunArgs {
    /// The ELF file to run, or a raw firmware image such as OpenSBI.
    image: PathBuf,
    /// A raw kernel image that the firmware starts, at 0x80200000.
    #[arg(long)]
    kernel: Option<PathBuf>,
    /// An initramfs. The device tree tells the kernel where it is.
    #[arg(long)]
    initrd: Option<PathBuf>,
    /// The kernel command line.
    #[arg(long, default_value = "console=ttyS0")]
    append: String,
    /// A device tree file to use instead of the one that hartwood writes.
    #[arg(long)]
    dtb: Option<PathBuf>,
    /// A raw disk image for the virtio block device. The emulator changes a
    /// copy in memory, and not the file.
    #[arg(long)]
    drive: Option<PathBuf>,
    /// Print each instruction before it runs.
    #[arg(long)]
    trace: bool,
    /// Start the trace after this many steps.
    #[arg(long, default_value_t = 0)]
    trace_from: u64,
    /// Stop after this many instructions.
    #[arg(long, default_value_t = u64::MAX)]
    max_steps: u64,
    /// The size of the RAM in MiB.
    #[arg(long, default_value_t = 128)]
    ram_mib: usize,
    /// Type these bytes on the console when the guest turns on the receive
    /// interrupt of the UART, which a driver does when it is ready to read.
    /// `\n` is a newline.
    #[arg(long)]
    input: Option<String>,
    /// Stop with code 0 when the console prints this text.
    #[arg(long)]
    until: Option<String>,
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(&args),
    }
}

/// Reads a file, and names it in the error.
fn read_file(path: &PathBuf) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

fn run(args: &RunArgs) -> Result<ExitCode> {
    let images = BootImages {
        firmware: read_file(&args.image)?,
        kernel: args.kernel.as_ref().map(read_file).transpose()?,
        initrd: args.initrd.as_ref().map(read_file).transpose()?,
        bootargs: args.append.clone(),
        dtb: args.dtb.as_ref().map(read_file).transpose()?,
    };
    let mut cpu = Cpu::new(Bus::new(args.ram_mib << 20));
    boot(&mut cpu, &images).with_context(|| format!("cannot boot {}", args.image.display()))?;
    if let Some(drive) = &args.drive {
        cpu.bus.virtio.disk = read_file(drive)?;
    }
    let mut script: Vec<u8> = args
        .input
        .as_ref()
        .map(|s| s.replace("\\n", "\n").into_bytes())
        .unwrap_or_default();

    // The console reads the keyboard only when a person can type. With
    // --input, the run is a script.
    let mut console = Console::open(args.input.is_none())?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut printed = Vec::new();
    let mut left = args.max_steps;

    let stop = loop {
        let tracing = args.trace && cpu.cycle >= args.trace_from;
        if tracing {
            let raw = cpu.bus.read(cpu.pc, 4).unwrap_or(0);
            console.write(
                &mut out,
                format!("{:016x} {:08x}\n", cpu.pc, raw as u32).as_bytes(),
            )?;
        }
        let batch = if tracing {
            1
        } else if args.trace {
            (args.trace_from - cpu.cycle).clamp(1, 100_000).min(left)
        } else {
            left.min(100_000)
        };
        let stop = cpu.run(batch);
        left -= batch.min(left);

        // The console prints what the UART and the HTIF got.
        for output in [&mut cpu.bus.uart.output, &mut cpu.bus.htif.output] {
            if !output.is_empty() {
                console.write(&mut out, output)?;
                if args.until.is_some() {
                    printed.extend_from_slice(output);
                }
                output.clear();
            }
        }
        out.flush()?;
        if let Some(until) = &args.until
            && contains(&printed, until.as_bytes())
        {
            break Stop::Exit(0);
        }
        if console.quit_requested() {
            break Stop::Exit(0);
        }

        if !script.is_empty() && cpu.bus.uart.receive_enabled() {
            cpu.bus.push_input(&std::mem::take(&mut script));
        }
        let typed = console.take_input();
        if !typed.is_empty() {
            cpu.bus.push_input(&typed);
        }
        match stop {
            Stop::StepLimit if left == 0 => break stop,
            Stop::StepLimit => {}
            // The hart sleeps until input comes. Wait for the keyboard.
            Stop::Idle if console.interactive() => {
                console.wait_for_input(Duration::from_millis(20))
            }
            other => break other,
        }
    };
    console.close();
    out.flush()?;

    match stop {
        Stop::Exit(code) => Ok(ExitCode::from(code.min(255) as u8)),
        Stop::Reset | Stop::Breakpoint => Ok(ExitCode::SUCCESS),
        Stop::StepLimit => {
            eprintln!("hartwood: stopped after {} instructions", cpu.instret);
            Ok(ExitCode::from(124))
        }
        Stop::Idle => {
            eprintln!("hartwood: the hart waits for an interrupt that cannot come");
            Ok(ExitCode::from(124))
        }
    }
}

/// Tells if `needle` is in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}
