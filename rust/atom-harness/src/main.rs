// Neutral CLI test-harness binary for the atom-matching pilot (see
// PORTING/PROMPT.md and PORTING/rust/portage-dep/src/lib.rs for the v1
// grammar this implements). Same argv/output contract as
// PORTING/python/atom_harness.py.
//
// Usage:
//   atom-harness parse <atom>              -> tab-separated fields, or "INVALID"
//   atom-harness match <atom> <cand...>    -> comma-joined matches (possibly
//                                              empty), or "INVALID"
//   atom-harness batch                     -> reads "parse <atom>" or
//                                              "match <atom> <cand...>" lines
//                                              from stdin, one result per line

use portage_dep::{parse_atom, Blocker, SlotOperator, UseDep, UseDepDefault, UseDepOp};
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

/// Reconstructs one use-dep token's canonical text ("{prefix}{flag}{default}{suffix}"),
/// matching real `a.use.tokens`' own original-text preservation byte for
/// byte, since this pilot's grammar doesn't normalize anything the token
/// regex didn't already require.
fn format_use_dep(u: &UseDep) -> String {
    let (prefix, suffix) = match u.op {
        UseDepOp::Enabled => ("", ""),
        UseDepOp::Disabled => ("-", ""),
        UseDepOp::IfParentEnabled => ("", "?"),
        UseDepOp::IfParentDisabled => ("!", "?"),
        UseDepOp::EqualParent => ("", "="),
        UseDepOp::OppositeParent => ("!", "="),
    };
    let default = match u.default {
        None => "",
        Some(UseDepDefault::Enabled) => "(+)",
        Some(UseDepDefault::Disabled) => "(-)",
    };
    format!("{prefix}{}{default}{suffix}", u.flag)
}

fn format_parse(s: &str) -> String {
    let Some(a) = parse_atom(s) else {
        return "INVALID".to_string();
    };
    let blocker = match a.blocker {
        Blocker::None => "",
        Blocker::Weak => "!",
        Blocker::Strong => "!!",
    };
    let slot_operator = match a.slot_operator {
        None => "",
        Some(SlotOperator::Star) => "*",
        Some(SlotOperator::Equals) => "=",
    };
    let use_deps = a
        .use_deps
        .as_ref()
        .map(|deps| {
            deps.iter()
                .map(format_use_dep)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let fields = [
        blocker,
        a.operator.as_str(),
        &a.category,
        &a.package,
        a.version.as_deref().unwrap_or(""),
        a.revision.as_deref().unwrap_or(""),
        a.slot.as_deref().unwrap_or(""),
        a.sub_slot.as_deref().unwrap_or(""),
        slot_operator,
        &use_deps,
    ];
    fields.join("\t")
}

fn format_match(atom_str: &str, candidates: &[&str]) -> String {
    match portage_dep::match_from_list(atom_str, candidates) {
        None => "INVALID".to_string(),
        Some(matches) => matches.join(","),
    }
}

fn dispatch(op: &str, args: &[&str]) -> Result<String, String> {
    match op {
        "parse" => {
            let [a] = args else {
                return Err(format!("parse expects 1 arg, got {}", args.len()));
            };
            Ok(format_parse(a))
        }
        "match" => {
            let [atom_str, candidates @ ..] = args else {
                return Err("match expects at least 1 arg (the atom)".to_string());
            };
            Ok(format_match(atom_str, candidates))
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
            eprintln!("usage: atom-harness <parse atom | match atom cand... | batch>");
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
