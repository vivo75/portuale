// Neutral CLI test-harness binary for the use_reduce(flat=True) pilot
// (see docs/agent-context.md and portage-use-reduce/src/lib.rs's doc comment). Same
// argv/output contract as python/use_reduce_harness.py.
//
// Usage:
//   use-reduce-harness reduce <mode> <uselist> <token...>
//       mode: "normal" | "matchall" | "matchnone"
//       uselist: comma-separated enabled flags, or "-" for none
//       token...: the dep string's whitespace-separated tokens (the CLI
//                 shell already splits argv on whitespace for us, which is
//                 exactly what use_reduce's own depstr.split() first step
//                 does internally)
//     -> comma-joined flattened tokens (possibly empty), or "ERROR"
//   use-reduce-harness batch
//     -> reads "reduce <mode> <uselist> <token...>" lines from stdin, one
//        result per line

use portage_use_reduce::{use_reduce_flat, MatchMode};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn format_reduce(mode: &str, uselist_arg: &str, tokens: &[String]) -> Result<String, String> {
    let mode = match mode {
        "normal" => MatchMode::Normal,
        "matchall" => MatchMode::All,
        "matchnone" => MatchMode::None,
        other => return Err(format!("unknown mode {other:?}")),
    };
    let uselist: HashSet<String> = if uselist_arg == "-" {
        HashSet::new()
    } else {
        uselist_arg.split(',').map(String::from).collect()
    };
    Ok(match use_reduce_flat(tokens, &uselist, mode) {
        Ok(result) => result.join(","),
        Err(_) => "ERROR".to_string(),
    })
}

fn dispatch(op: &str, args: &[&str]) -> Result<String, String> {
    match op {
        "reduce" => {
            let [mode, uselist, tokens @ ..] = args else {
                return Err("reduce expects at least 2 args (mode, uselist)".to_string());
            };
            let tokens: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
            format_reduce(mode, uselist, &tokens)
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
            eprintln!("usage: use-reduce-harness <reduce mode uselist token... | batch>");
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
