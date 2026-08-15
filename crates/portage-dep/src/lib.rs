//! Rust port of Portage's `portage.dep` atom parsing and matching
//! (`lib/portage/dep/__init__.py`): `Atom`, `match_from_list`, USE-dep
//! evaluation and `extract_affecting_use`.
//!
//! Scope cuts a caller must know:
//! - `parse_atom` takes no extended/wildcard atoms (`*/foo-1`; see
//!   `WildcardAtom` for the bounded variant) and no build-ids (`foo-1.0@2`).
//! - Not EAPI-parametrized: slot operators, USE deps, `=*` and `::repo` are
//!   always recognized.
//! - Candidates are plain strings `cat/pkg-ver[-rN][:slot[/subslot]][::repo]`,
//!   not Package objects, so `match_from_list` never consults
//!   `Atom::use_deps`; callers apply `use_deps_satisfied` afterwards.
//! - A candidate without `::repo` always passes a repo constraint.
//! - A bare atom whose name ends in something version-like (`foo-bar-2`) is
//!   rejected as ambiguous, per PMS.

use portage_versions::vercmp;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;

const CAT: &str = r"[A-Za-z0-9_][A-Za-z0-9+_.-]*";
// Non-greedy, like `_pkg` in lib/portage/versions.py: the following
// `-<version>` decides where the package name ends.
const PKG: &str = r"[A-Za-z0-9_][A-Za-z0-9+_-]*?";
const VER: &str = r"\d+(?:\.\d+)*[a-z]?(?:_(?:pre|p|beta|alpha|rc)\d*)*";
const SLOT: &str = r"[A-Za-z0-9][A-Za-z0-9+_.-]*";
// Portage: lib/portage/dep/__init__.py, _repo_name
const REPO: &str = r"[A-Za-z0-9_][A-Za-z0-9_-]*";
// Portage: lib/portage/dep/__init__.py, _useflag_re (duplicated from
// portage-use-reduce to avoid a cross-crate dependency).
const USEFLAG: &str = r"[A-Za-z0-9][A-Za-z0-9+_@-]*";

/// A USE dependency's operator (PMS 8.3.4): one of the six prefix+suffix forms.
/// Only `Enabled`/`Disabled` constrain a candidate's USE state when matching;
/// the four parent-conditional forms are resolved by
/// `evaluate_use_dep_conditionals`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseDepOp {
    Enabled,          // "flag"
    Disabled,         // "-flag"
    IfParentEnabled,  // "flag?"
    IfParentDisabled, // "!flag?"
    EqualParent,      // "flag="
    OppositeParent,   // "!flag="
}

/// A 4-style `(+)`/`(-)` default (PMS 8.3.4): the value assumed when the
/// candidate's IUSE lacks the flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseDepDefault {
    Enabled,  // "(+)"
    Disabled, // "(-)"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseDep {
    pub flag: String,
    pub op: UseDepOp,
    pub default: Option<UseDepDefault>,
}

fn atom_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // "slotpart" and "usedeps" are captured permissively and validated in a
        // second stage, as Portage does (_get_atom_re, then _get_slot_dep_re): this
        // tells `foo` from `foo:` (invalid) and defers use-dep checks to
        // `parse_use_deps`.
        // "glob" is captured after any operator; `parse_atom` rejects it unless the
        // operator is `=`.
        // "repo" sits between the slot part and usedeps, as in Portage.
        Regex::new(&format!(
            r"^(?P<blocker>!!|!)?(?:(?P<op>=|>=|>|<=|<|~)(?P<vcat>{CAT})/(?P<vpkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?(?P<glob>\*)?|(?P<cat>{CAT})/(?P<pkg>{PKG})(?P<ambiguous>-{VER}(?:-r\d+)?)?)(?::(?P<slotpart>(?:(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?P<slotop>[*=])?))?(?:::(?P<repo>{REPO}))?(?P<usedeps>\[.*\])?$"
        ))
        .unwrap()
    })
}

fn use_dep_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?P<prefix>[!-]?)(?P<flag>{USEFLAG})(?P<default>\(\+\)|\(-\))?(?P<suffix>[?=]?)$"
        ))
        .unwrap()
    })
}

/// Validates the raw `[...]` text (brackets included) into `UseDep`s.
///
/// Returns `None` for what Portage's `Atom` rejects: a bad token,
/// `-flag=`/`-flag?`, or a flag whose `(+)`/`(-)` default differs between
/// tokens of the same atom.
fn parse_use_deps(raw: &str) -> Option<Vec<UseDep>> {
    let inner = raw.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return None;
    }
    let mut deps = Vec::new();
    // Default seen so far per flag; a later token with a different default state is invalid.
    let mut seen_defaults: std::collections::HashMap<String, Option<UseDepDefault>> =
        std::collections::HashMap::new();
    for token in inner.split(',') {
        let caps = use_dep_token_regex().captures(token)?;
        let flag = caps.name("flag").unwrap().as_str().to_string();
        let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
        let suffix = caps.name("suffix").map(|m| m.as_str()).unwrap_or("");
        let op = match (prefix, suffix) {
            ("", "") => UseDepOp::Enabled,
            ("-", "") => UseDepOp::Disabled,
            ("", "?") => UseDepOp::IfParentEnabled,
            ("!", "?") => UseDepOp::IfParentDisabled,
            ("", "=") => UseDepOp::EqualParent,
            ("!", "=") => UseDepOp::OppositeParent,
            _ => return None, // "-flag=" / "-flag?": syntactically matched, not a real operator
        };
        let default = match caps.name("default").map(|m| m.as_str()) {
            None => None,
            Some("(+)") => Some(UseDepDefault::Enabled),
            Some("(-)") => Some(UseDepDefault::Disabled),
            Some(_) => unreachable!(),
        };
        match seen_defaults.entry(flag.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if *e.get() != default {
                    return None;
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(default.clone());
            }
        }
        deps.push(UseDep { flag, op, default });
    }
    Some(deps)
}

fn candidate_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?P<cat>{CAT})/(?P<pkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?:::(?P<repo>{REPO}))?$"
        ))
        .unwrap()
    })
}

/// Collapses leading zeros of a plain version (never the `-rN` revision) as
/// Portage does before an `=*` comparison.
///
/// `=*` is a literal prefix match, so `"01"` must equal `"1"`: `"00"` ->
/// `"0"`, `"01"` -> `"1"`, `"0.5"` unchanged, `"00.5"` -> `"0.5"`.
/// Portage: lib/portage/dep/__init__.py, match_from_list (`=*` branch)
fn normalize_leading_zeros(version: &str) -> String {
    let stripped = version.trim_start_matches('0');
    let starts_with_digit = stripped.starts_with(|c: char| c.is_ascii_digit());
    if starts_with_digit {
        stripped.to_string()
    } else {
        format!("0{stripped}")
    }
}

/// The string `=*` prefix-compares for one side: normalized version plus `-rN` if present.
fn glob_compare_string(version: &str, revision: &Option<String>) -> String {
    let normalized = normalize_leading_zeros(version);
    match revision {
        Some(r) => format!("{normalized}-r{r}"),
        None => normalized,
    }
}

// --- Bounded wildcard atoms (package.mask/.unmask/.accept_keywords) ---
//
// Separate from `Atom`/`parse_atom`, which still reject wildcards. Only
// `*/*`, `cat/*` and `*/pkg`: a literal `*` for a whole category or package;
// no partial globs, operators or slots.

fn cat_full_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!("^{CAT}$")).unwrap())
}

fn pkg_full_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!("^{PKG}$")).unwrap())
}

fn affecting_useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Modern-EAPI useflag charset; EAPI is not parametrized.
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}
