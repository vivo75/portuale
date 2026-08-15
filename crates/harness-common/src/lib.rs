// Shared CLI scaffolding for portuale's neutral test-harness binaries
// (versions-harness, atom-harness, use-reduce-harness,
// required-use-harness): one operation per invocation, or a stdin
// "batch" loop in benchmark mode.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

/// Result of a single harness operation, formatted for the output contract.
pub type DispatchResult = Result<String, String>;

/// Per-op library dispatch: `dispatch(op, args)` returns the op's output or error message.
pub type Dispatch = fn(op: &str, args: &[&str]) -> DispatchResult;

/// Batch mode: one `<op> <args...>` per stdin line, one result per stdout
/// line; `ExitCode::FAILURE` at the first dispatch error. Empty lines are skipped.
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

/// Standard harness `main`: no argv prints `usage: {usage}` (exit 2);
/// `batch` runs the stdin loop; otherwise dispatches the single op.
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
