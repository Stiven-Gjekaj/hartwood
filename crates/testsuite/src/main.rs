//! `hartwood-testsuite`: runs the riscv-tests programs and prints a table.
//!
//!     hartwood-testsuite [--dir <dir>] <prefix>...
//!
//! Each prefix selects a group of tests, such as `rv64ui-p-`. The program
//! exits with 1 if a selected test fails, and with 2 if no test matches.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use hartwood_testsuite::{Outcome, find_tests, run_file};

const DEFAULT_DIR: &str = "build/riscv-tests/isa";

fn main() -> ExitCode {
    let mut dir = PathBuf::from(DEFAULT_DIR);
    let mut prefixes = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            match args.next() {
                Some(d) => dir = PathBuf::from(d),
                None => {
                    eprintln!("--dir needs a directory");
                    return ExitCode::from(2);
                }
            }
        } else {
            prefixes.push(arg);
        }
    }
    if prefixes.is_empty() {
        eprintln!("usage: hartwood-testsuite [--dir <dir>] <prefix>...");
        return ExitCode::from(2);
    }

    let tests = match find_tests(&dir, &prefixes) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {}: {e}", dir.display());
            eprintln!("Run scripts/build-riscv-tests.sh first.");
            return ExitCode::from(2);
        }
    };
    if tests.is_empty() {
        eprintln!("no test in {} matches {prefixes:?}", dir.display());
        return ExitCode::from(2);
    }

    // The count of passes and the count of tests for each group, such as
    // rv64ui-p.
    let mut groups: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failed = 0;
    for path in &tests {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let group = name.splitn(3, '-').take(2).collect::<Vec<_>>().join("-");
        let outcome = run_file(path);
        let entry = groups.entry(group).or_default();
        entry.1 += 1;
        let text = match &outcome {
            Outcome::Pass => {
                entry.0 += 1;
                "pass".to_string()
            }
            Outcome::Fail(n) => format!("FAIL (test {n})"),
            Outcome::Timeout => "FAIL (timeout)".to_string(),
            Outcome::Error(e) => format!("FAIL ({e})"),
        };
        if !outcome.passed() {
            failed += 1;
        }
        println!("{name:<28} {text}");
    }

    println!();
    println!("{:<12} {:>6} {:>6}", "group", "pass", "total");
    for (group, (pass, total)) in &groups {
        println!("{group:<12} {pass:>6} {total:>6}");
    }
    println!(
        "{:<12} {:>6} {:>6}",
        "all",
        tests.len() - failed,
        tests.len()
    );

    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
