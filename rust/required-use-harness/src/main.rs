// Neutral CLI test-harness binary for portuale's REQUIRED_USE checking (see
// docs/agent-context.md and portage-required-use/src/lib.rs's doc comment).
// Same argv/output contract as python/required_use_harness.py,
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
//   required-use-harness reduce <enabled> <iuse> <token...>
//     -> "-" when satisfied, else the human-readable minimal unsatisfied
//        sub-expression (real check_required_use(...).tounicode() run
//        through human_readable_required_use), or "ERROR"
//   required-use-harness batch
//     -> reads "<op> <enabled> <iuse> <token...>" lines from stdin, one
//        result per line

use portage_required_use::{check_required_use, human_readable, unsatisfied_reduced};
use std::collections::HashSet;
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

fn format_reduce(enabled_arg: &str, iuse_arg: &str, tokens: &[&str]) -> String {
    let enabled = parse_set(enabled_arg);
    let iuse = parse_set(iuse_arg);
    let required_use = tokens.join(" ");
    match unsatisfied_reduced(&required_use, &enabled, &iuse) {
        Ok(None) => "-".to_string(),
        Ok(Some(reduced)) => human_readable(&reduced),
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
        // "reduce": "-" when satisfied, else the human-readable minimal
        // unsatisfied sub-expression (real depgraph.py's "The following
        // REQUIRED_USE flag constraints are unsatisfied:" line).
        "reduce" => {
            let [enabled, iuse, tokens @ ..] = args else {
                return Err("reduce expects at least 2 args (enabled, iuse)".to_string());
            };
            Ok(format_reduce(enabled, iuse, tokens))
        }
        other => Err(format!("unknown op {other:?}")),
    }
}

fn main() -> ExitCode {
    harness_common::main_dispatch(
        "required-use-harness <check enabled iuse token... | batch>",
        dispatch,
    )
}
