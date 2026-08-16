// Neutral CLI test-harness binary for the REQUIRED_USE pilot (see
// PORTING/PROMPT.md and portage-required-use/src/lib.rs's doc comment).
// Same argv/output contract as PORTING/python/required_use_harness.py,
// which wraps real portage.dep.check_required_use directly.
//
// Usage:
//   required-use-harness check <enabled> <iuse> <token...>
//       enabled: comma-separated effective USE flags, or "-" for none
//       iuse: comma-separated declared IUSE flags, or "-" for none
//       token...: the REQUIRED_USE string's whitespace-separated tokens
//                 (the CLI shell already splits argv on whitespace for
//                 us, same convention use-reduce-harness's own "reduce"
//                 op already uses)
//     -> "true" | "false" | "ERROR"
//   required-use-harness batch
//     -> reads "check <enabled> <iuse> <token...>" lines from stdin, one
//        result per line

use portage_required_use::check_required_use;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn parse_set(arg: &str) -> HashSet<String> {
    if arg == "-" {
        HashSet::new()
    } else {
        arg.split(',').map(String::from).collect()
    }
}

fn format_check(enabled_arg: &str, iuse_arg: &str, tokens: &[&str]) -> String {
    let enabled = parse_set(enabled_arg);
    let iuse = parse_set(iuse_arg);
    let required_use = tokens.join(" ");
    match check_required_use(&required_use, &enabled, &iuse) {
        Ok(true) => "true".to_string(),
        Ok(false) => "false".to_string(),
        Err(_) => "ERROR".to_string(),
    }
}

fn dispatch(op: &str, args: &[&str]) -> Result<String, String> {
    match op {
        "check" => {
            let [enabled, iuse, tokens @ ..] = args else {
                return Err("check expects at least 2 args (enabled, iuse)".to_string());
            };
            Ok(format_check(enabled, iuse, tokens))
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
            eprintln!("usage: required-use-harness <check enabled iuse token... | batch>");
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
