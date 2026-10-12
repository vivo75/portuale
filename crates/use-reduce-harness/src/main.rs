// Test-harness CLI for `use_reduce(flat=True)`: same argv/output contract
// as the Python harness.
//
// Usage:
//   use-reduce-harness reduce <mode> <uselist> <token...>
//       mode: "normal" | "matchall" | "matchnone"
//       uselist: comma-separated enabled flags, or "-" for none
//       token...: the dep string's whitespace-separated tokens
//     -> comma-joined flattened tokens (possibly empty), or "ERROR"
//   use-reduce-harness validate <eapi> <validflags> <token...>
//       eapi: EAPI text, or "None" for eapi=None
//       validflags: comma-separated valid IUSE flags, or "-" for none
//       token...: the dep string's whitespace-separated tokens
//     -> "OK", or "ERR:<category>:<message>" with Portage's
//        `_metadata_exception` selection (a categorized inner error prints
//        bare, anything else the outer `InvalidDependString` text)
//   use-reduce-harness batch
//     -> reads "reduce ..." / "validate ..." lines from stdin, one
//        result per line
//
// The `validate` op wires `parse_atom_with` + `check_conditional_flags` as
// the walk's atom hook, exactly like `portage-repo`'s `dep_validate`
// (which owns the production wiring; this is the differential mirror).

use portage_dep::{
    Atom, AtomEapi, AtomErrorKind, AtomOptions, check_conditional_flags, parse_atom_with,
};
use portage_use_reduce::{Error as ReduceError, use_reduce_flat_validated};
use portage_use_reduce::{MatchMode, TokenError, TokenErrorCategory, use_reduce_flat};
use std::collections::HashSet;
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

fn format_validate(
    eapi_arg: &str,
    validflags_arg: &str,
    tokens: &[String],
) -> Result<String, String> {
    let eapi_text = if eapi_arg == "None" { "" } else { eapi_arg };
    let atom_eapi = AtomEapi::parse(if eapi_arg == "None" {
        None
    } else {
        Some(eapi_arg)
    })
    .map_err(|_| {
        format!("ERR:dependency.syntax:Invalid atom (invalid EAPI '{eapi_arg}'), token 1")
    })?;
    let opts = AtomOptions {
        eapi: atom_eapi,
        ..Default::default()
    };
    let valid: HashSet<String> = if validflags_arg == "-" {
        HashSet::new()
    } else {
        validflags_arg.split(',').map(String::from).collect()
    };
    let valid_flag = |flag: &str| valid.contains(flag);
    let mut hook = |token: &str, _pos: usize| -> Result<(), TokenError> {
        let atom: Atom = parse_atom_with(token, &opts, eapi_text).map_err(|err| TokenError {
            category: match err.kind {
                AtomErrorKind::Invalid => None,
                AtomErrorKind::IuseMissing => Some(TokenErrorCategory::IuseMissing),
                AtomErrorKind::EapiIncompatible => Some(TokenErrorCategory::EapiIncompatible),
            },
            message: err.message,
        })?;
        check_conditional_flags(&atom, atom_eapi, &valid_flag).map_err(|err| TokenError {
            category: Some(TokenErrorCategory::IuseMissing),
            message: err.message,
        })?;
        Ok(())
    };
    match use_reduce_flat_validated(tokens, &valid_flag, &mut hook) {
        Ok(()) => Ok("OK".to_string()),
        Err(ReduceError::Categorized { message, category }) => {
            let cat = match category {
                TokenErrorCategory::IuseMissing => "IUSE.missing",
                TokenErrorCategory::EapiIncompatible => "EAPI.incompatible",
            };
            Ok(format!("ERR:{cat}:{message}"))
        }
        Err(other) => Ok(format!("ERR:dependency.syntax:{other}")),
    }
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
        "validate" => {
            let [eapi, validflags, tokens @ ..] = args else {
                return Err("validate expects at least 2 args (eapi, validflags)".to_string());
            };
            let tokens: Vec<String> = tokens.iter().map(|s| s.to_string()).collect();
            format_validate(eapi, validflags, &tokens)
        }
        other => Err(format!("unknown op {other:?}")),
    }
}

fn main() -> ExitCode {
    harness_common::main_dispatch(
        "use-reduce-harness <reduce mode uselist token... | validate eapi validflags token... | batch>",
        dispatch,
    )
}
