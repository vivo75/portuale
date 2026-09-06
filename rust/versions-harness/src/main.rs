// Neutral CLI test-harness binary for portuale's version comparison (see
// docs/agent-context.md, "Test/benchmark harness architecture"). Exposes the
// same argv/output contract as python/versions_harness.py so a
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

fn main() -> ExitCode {
    harness_common::main_dispatch(
        "versions-harness <vercmp v1 v2 | ververify v | batch>",
        dispatch,
    )
}
