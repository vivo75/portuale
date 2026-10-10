// Shared CLI scaffolding for portuale's neutral test-harness binaries
// (versions-harness, atom-harness, use-reduce-harness, required-use-harness;
// see docs/agent-context.md, "Test/benchmark harness architecture"). Every
// harness is a thin per-crate `dispatch(op, args)` over the library surface
// plus the same two-mode driver: one operation per process invocation in
// correctness mode, or a stdin "batch" loop in benchmark mode. The argv/output
// contract this implements is identical to the Python-side harnesses.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

/// Result of a single harness operation, formatted for the output contract.
pub type DispatchResult = Result<String, String>;

/// Per-op library dispatch: `dispatch(op, args)` returns the op's output
/// string or an error message.
pub type Dispatch = fn(op: &str, args: &[&str]) -> DispatchResult;

/// Correctness/benchmark batch mode: read one operation per line from stdin
/// ("<op> <args...>"), write each result as its own line on stdout, and stop
/// with `ExitCode::FAILURE` at the first dispatch error. Empty lines are
/// skipped so trailing newlines never terminate the loop.
pub fn run_batch(dispatch: Dispatch) -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line.expect("failed to read stdin");
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match dispatch(parts[0], &parts[1..]) {
            Ok(result) => writeln!(out, "{result}").expect("failed to write stdout"),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

/// Standard harness `main`: no argv → print `usage: {usage}` and exit 2;
/// `batch` → `run_batch` stdin loop; otherwise dispatch the single op with
/// its remaining argv, printing the result on stdout and exiting 0, or the
/// error on stderr and exiting 2.
pub fn main_dispatch(usage: &str, dispatch: Dispatch) -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();

    match args.as_slice() {
        [] => {
            eprintln!("usage: {usage}");
            ExitCode::from(2)
        }
        ["batch"] => run_batch(dispatch),
        [op, rest @ ..] => match dispatch(op, rest) {
            Ok(result) => {
                println!("{result}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
    }
}
