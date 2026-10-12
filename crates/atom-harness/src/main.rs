// Neutral CLI test-harness binary for portuale's atom matching (same
// argv/output contract as python/atom_harness.py).
//
// Usage:
//   atom-harness parse [--eapi E] [--allow-repo 0|1] [--allow-build-id 0|1]
//                      [--allow-wildcard] <atom>
//                                              -> tab-separated fields (with
//                                                 trailing build-id field),
//                                                 or "INVALID"
//   atom-harness match <atom> <cand...>    -> comma-joined matches (possibly
//                                              empty), or "INVALID"
//   atom-harness affecting <atom> <dep...> -> sorted comma-joined
//                                              extract_affecting_use flags,
//                                              or "INVALID"
//   atom-harness batch                     -> reads "parse <atom>",
//                                              "match <atom> <cand...>" or
//                                              "affecting <atom> <dep...>" lines
//                                              from stdin, one result per line
//                                              (`parse` lines take the same
//                                              flags, atom last)

use portage_dep::{AtomEapi, AtomOptions, Blocker, SlotOperator, UseDep, UseDepDefault, UseDepOp};
use std::process::ExitCode;

/// Reconstructs one use-dep token's canonical text, byte-identical to Portage's `a.use.tokens`.
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

fn format_parse(s: &str, opts: &AtomOptions, eapi_str: &str) -> String {
    let Ok(a) = portage_dep::parse_atom_with(s, opts, eapi_str) else {
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
        a.repo.as_deref().unwrap_or(""),
        &use_deps,
        a.build_id.as_deref().unwrap_or(""),
    ];
    fields.join("\t")
}

fn format_match(atom_str: &str, candidates: &[&str]) -> String {
    match portage_dep::match_from_list(atom_str, candidates) {
        None => "INVALID".to_string(),
        Some(matches) => matches.join(","),
    }
}

/// `extract_affecting_use` as sorted comma-joined flags, or "INVALID".
/// The dep string is the args after the atom joined on a single space.
fn format_affecting(atom_str: &str, dep_tokens: &[&str]) -> String {
    match portage_dep::extract_affecting_use(&dep_tokens.join(" "), atom_str) {
        None => "INVALID".to_string(),
        Some(flags) => {
            let mut v: Vec<&str> = flags.iter().map(String::as_str).collect();
            v.sort_unstable();
            v.join(",")
        }
    }
}

fn dispatch(op: &str, args: &[&str]) -> Result<String, String> {
    match op {
        "parse" => {
            // `parse [--eapi E] [--allow-repo 0|1] [--allow-build-id 0|1]
            // [--allow-wildcard] <atom>`; the atom is the last arg (atoms
            // never contain spaces, flags always start with `--`).
            let (opts, eapi_str, atom, eapi_invalid) = parse_opts(args)?;
            if eapi_invalid {
                return Ok("INVALID".to_string());
            }
            Ok(format_parse(atom, &opts, eapi_str))
        }
        "match" => {
            let [atom_str, candidates @ ..] = args else {
                return Err("match expects at least 1 arg (the atom)".to_string());
            };
            Ok(format_match(atom_str, candidates))
        }
        "affecting" => {
            let [atom_str, dep_tokens @ ..] = args else {
                return Err("affecting expects at least 1 arg (the atom)".to_string());
            };
            Ok(format_affecting(atom_str, dep_tokens))
        }
        other => Err(format!("unknown op {other:?}")),
    }
}

/// Parses the `parse` flags; returns the options, the borrowed EAPI text, the
/// atom (always the last arg), and whether the `--eapi` value is a
/// supported-but-unparseable spelling (Portage raises `ValueError` there, so
/// the harness reports the atom INVALID).
fn parse_opts<'a>(args: &'a [&'a str]) -> Result<(AtomOptions, &'a str, &'a str, bool), String> {
    let mut opts = AtomOptions::default();
    let mut eapi_str = "";
    let mut eapi_invalid = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--eapi" => {
                i += 1;
                eapi_str = args
                    .get(i)
                    .ok_or_else(|| "--eapi expects a value".to_string())?;
                match AtomEapi::parse(Some(eapi_str)) {
                    Ok(eapi) => opts.eapi = eapi,
                    Err(_) => eapi_invalid = true,
                }
            }
            "--allow-repo" => {
                i += 1;
                opts.allow_repo = Some(parse_bool_flag("--allow-repo", args.get(i))?);
            }
            "--allow-build-id" => {
                i += 1;
                opts.allow_build_id = Some(parse_bool_flag("--allow-build-id", args.get(i))?);
            }
            "--allow-wildcard" => opts.allow_wildcard = true,
            _ => break,
        }
        i += 1;
    }
    let [atom] = &args[i..] else {
        return Err(format!("parse expects 1 atom, got {:?}", &args[i..]));
    };
    Ok((opts, eapi_str, atom, eapi_invalid))
}

fn parse_bool_flag(name: &str, value: Option<&&str>) -> Result<bool, String> {
    match value.copied() {
        Some("1") | Some("y") | Some("yes") | Some("true") => Ok(true),
        Some("0") | Some("n") | Some("no") | Some("false") => Ok(false),
        other => Err(format!("{name} expects 0|1, got {other:?}")),
    }
}

fn main() -> ExitCode {
    harness_common::main_dispatch(
        "atom-harness <parse atom | match atom cand... | batch>",
        dispatch,
    )
}
