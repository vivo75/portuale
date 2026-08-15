// Neutral CLI test-harness binary for the versions-comparison pilot (see
// PORTING/PROMPT.md, "Test/benchmark harness architecture"). Exposes the
// same argv/output contract as PORTING/python/versions_harness.py so a
// black-box test suite can drive both implementations identically.
//
// Usage:
//   versions-harness vercmp <ver1> <ver2>   -> prints an integer or "None"
//   versions-harness ververify <ver>        -> prints "True" or "False"
//   versions-harness batch                  -> reads "<op> <args...>" lines
//                                               from stdin, one result per
//                                               line on stdout (benchmark
//                                               mode: avoids per-op
//                                               fork/exec overhead)

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn dispatch(op: &str, args: &[&str]) -> Result<String, String> {
    match op {
        "vercmp" => {
            let [v1, v2] = args else {
                return Err(format!("vercmp expects 2 args, got {}", args.len()));
            };
            Ok(match portage_versions::vercmp(v1, v2) {
                Some(n) => n.to_string(),
                None => "None".to_string(),
            })
        }
        "ververify" => {
            let [v] = args else {
                return Err(format!("ververify expects 1 arg, got {}", args.len()));
            };
            Ok(if portage_versions::ververify(v) {
                "True"
            } else {
                "False"
            }
            .to_string())
        }
        other => Err(format!("unknown op {other:?}")),
    }
}

fn run_batch() -> ExitCode {
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();

    match args.as_slice() {
        [] => {
            eprintln!("usage: versions-harness <vercmp v1 v2 | ververify v | batch>");
            ExitCode::from(2)
        }
        ["batch"] => run_batch(),
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
