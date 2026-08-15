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

fn candidate_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?P<cat>{CAT})/(?P<pkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?:::(?P<repo>{REPO}))?$"
        ))
        .unwrap()
    })
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
