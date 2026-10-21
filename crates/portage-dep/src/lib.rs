//! Rust port of Portage's `portage.dep` atom parsing and matching
//! (`lib/portage/dep/__init__.py`): `Atom`, `match_from_list`, USE-dep
//! evaluation and `extract_affecting_use`.
//!
//! Scope cuts a caller must know:
//! - `parse_atom` (= `eapi=None`, no wildcard flag) still rejects extended
//!   atoms (`*/foo`, `dev-libs/fo*`, `=*/foo-*1*`); `parse_atom_with` accepts
//!   them only with `allow_wildcard` (S3 ports the wildcard branch).
//!   `CpBucketIndex::build` relies on the default rejection to route
//!   wildcards to `other`.
//! - `parse_atom` (= `eapi=None`) accepts build-ids (`=cat/pkg-1.0-2`) and
//!   `::repo`; `parse_atom_with` applies the caller's EAPI/flag options.
//! - Candidates are plain strings `cat/pkg-ver[-rN][:slot[/subslot]][::repo]`,
//!   not Package objects, so `match_from_list` never consults
//!   `Atom::use_deps`; callers apply `use_deps_satisfied` afterwards.
//! - A candidate without `::repo` always passes a repo constraint.
//! - A bare atom whose name ends in something version-like (`foo-bar-2`) is
//!   rejected as ambiguous, per PMS.
//! - An `=` atom with a build-id matches only a binary instance carrying that
//!   build-id (`match_from_list_with_build_id`); the plain-string path matches
//!   none (#341 S9).
//! - Installed packages' vdb dep strings are parsed with the plain grammar:
//!   Portage's C-scanner rejections of `::repo`/build-id atoms there are not
//!   replicated (deliberate cut, #341 S10).

use portage_versions::vercmp;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::eapi::EapiAttrs;
use eapi::{EapiValueError, SUPPORTED_EAPIS, eapi_attrs, eapi_is_supported};
use regex::Captures;

mod specificity;
pub use specificity::atom_specificity;
pub mod eapi;

#[cfg(test)]
mod tests_extended_match;
#[cfg(test)]
mod tests_grammar;

// Memoizes parse results: parsing is a pure function of the string, and the
// resolver re-parses the same few atoms/candidates millions of times.
// Thread-local to stay lock-free; never invalidated.
type AtomOptsCache = HashMap<AtomOptions, HashMap<String, Result<Atom, AtomError>>>;
/// Key of the per-pair match verdicts: atom string, candidate string, build-id.
type MatchPairKey = (String, String, Option<String>);
thread_local! {
    static ATOM_CACHE: RefCell<HashMap<String, Option<Atom>>> = RefCell::new(HashMap::new());
    static CANDIDATE_CACHE: RefCell<HashMap<String, Option<Candidate>>> =
        RefCell::new(HashMap::new());
    static ATOM_OPTS_CACHE: RefCell<AtomOptsCache> = RefCell::new(HashMap::new());
    // Per-pair match verdicts: matching is a pure function of the two
    // strings (match-mode parse) plus the binary build-id, and the resolver
    // re-asks the same pairs every pass. The flat owned key costs three
    // small allocations per lookup and skips an `Atom` clone, a `Candidate`
    // clone and a `vercmp` on every hit.
    static MATCH_CACHE: RefCell<HashMap<MatchPairKey, Option<bool>>> =
        RefCell::new(HashMap::new());
}

const CAT: &str = r"[A-Za-z0-9_][A-Za-z0-9+_.-]*";
// Non-greedy, like `_pkg` in lib/portage/versions.py: the following
// `-<version>` decides where the package name ends.
const PKG: &str = r"[A-Za-z0-9_][A-Za-z0-9+_-]*?";
// Portage `_v` with every `\d` as `[0-9]`: the regex crate's `\d` is
// Unicode-aware, but Portage compiles with `re.ASCII`.
const VER: &str = r"[0-9]+(?:\.[0-9]+)*[a-z]?(?:_(?:pre|p|beta|alpha|rc)[0-9]*)*";
// Portage `_slot` (`[\w][\w+.-]*`, ASCII): the leading `_` matters.
const SLOT: &str = r"[A-Za-z0-9_][A-Za-z0-9+_.-]*";
// Portage `_extended_cat` (dep:252): the wildcard category half, which admits
// `.`; and the wildcard `pkg_re` (dep:333), which does not. Both admit `*`.
const EXTCAT: &str = r"[A-Za-z0-9_+*][A-Za-z0-9_+.*-]*";
const EXTPKG: &str = r"[A-Za-z0-9_+*][A-Za-z0-9_+*-]*?";
// Portage `star` version (dep:332-372): `=…-\*\w+\*`, ASCII `\w`, so no `.`
// or `-` (`=dev-libs/fo*-*1.0*` is ERR, oracle probe2).
// Portage: lib/portage/dep/__init__.py, _repo_name
const REPO: &str = r"[A-Za-z0-9_][A-Za-z0-9_-]*";
// Portage: lib/portage/dep/__init__.py, _useflag_re (duplicated from
// portage-use-reduce to avoid a cross-crate dependency).
const USEFLAG: &str = r"[A-Za-z0-9][A-Za-z0-9+_@-]*";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    None,
    Weak,   // "!"
    Strong, // "!!"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operator {
    None,
    Eq,
    /// `=*` (PMS 8.3.1): compare only the given version components; see `matches_version`.
    EqGlob,
    Gt,
    Ge,
    Lt,
    Le,
    Tilde,
}

impl Operator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Operator::None => "",
            Operator::Eq => "=",
            Operator::EqGlob => "=*",
            Operator::Gt => ">",
            Operator::Ge => ">=",
            Operator::Lt => "<",
            Operator::Le => "<=",
            Operator::Tilde => "~",
        }
    }

    fn from_str(s: &str) -> Operator {
        match s {
            "=" => Operator::Eq,
            ">" => Operator::Gt,
            ">=" => Operator::Ge,
            "<" => Operator::Lt,
            "<=" => Operator::Le,
            "~" => Operator::Tilde,
            _ => unreachable!("regex only captures known operators, got {s:?}"),
        }
    }
}

/// A slot operator (PMS 8.3.3): `:*` (`Star`) or `:=`/`:slot=` (`Equals`).
/// A rebuild-trigger signal only; it never affects matching (see `matches_slot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotOperator {
    Star,
    Equals,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    pub blocker: Blocker,
    pub operator: Operator,
    pub category: String,
    pub package: String,
    pub version: Option<String>,
    pub revision: Option<String>,
    pub slot: Option<String>,
    pub sub_slot: Option<String>,
    pub slot_operator: Option<SlotOperator>,
    /// `None` if there is no `[...]`; `Some` is never empty (`foo[]` is invalid).
    /// `match_from_list` ignores it; see `use_deps_satisfied`.
    pub use_deps: Option<Vec<UseDep>>,
    /// `::reponame` (PMS 3.1.5), `None` if absent; see `matches_repo`.
    pub repo: Option<String>,
    /// `-<digits>` build-id tail of a versioned atom (`=cat/pkg-1.0-2`), ASCII
    /// digits with no leading zero; Portage's int is unbounded, so the digits
    /// are kept as a string. `None` if absent.
    /// Portage: lib/portage/dep/__init__.py, Atom.__init__ (op arm)
    pub build_id: Option<String>,
    /// True for `_get_atom_wildcard_re` matches (only with `allow_wildcard`).
    pub extended: bool,
}

/// Portage `eapi=` as a `Copy` value; the original string is only needed for
/// messages, and the caller has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AtomEapi {
    #[default]
    None,
    Known(u8),
    Unsupported,
}

impl AtomEapi {
    /// `None` stays `None`; a supported spelling → `Known`, an unsupported one
    /// → `Unsupported`; a supported spelling Portage cannot parse (`3_pre1`,
    /// padded) → `Err` (S1's `EapiValueError`).
    /// Portage: lib/portage/eapi.py:224-229 (`Eapi.__init__`).
    pub fn parse(eapi: Option<&str>) -> Result<AtomEapi, EapiValueError> {
        let Some(s) = eapi else {
            return Ok(AtomEapi::None);
        };
        if !eapi_is_supported(s) {
            return Ok(AtomEapi::Unsupported);
        }
        // Portage: `partition("-")[0]` checked against `ALL_EAPIS`.
        let key = s.split_once('-').map(|(head, _)| head).unwrap_or(s);
        if SUPPORTED_EAPIS.contains(&key) {
            Ok(AtomEapi::Known(
                key.parse().expect("SUPPORTED_EAPIS are digits"),
            ))
        } else {
            Err(EapiValueError {
                input: s.to_string(),
            })
        }
    }
}

/// Per-context parse flags: one parser with options instead of one fixed
/// superset. `parse_atom` is these defaults (= Portage `eapi=None`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AtomOptions {
    /// Portage `eapi=`.
    pub eapi: AtomEapi,
    /// Portage `allow_wildcard`: try `_get_atom_wildcard_re` when the strict
    /// grammar fails (dep:1972-1997).
    pub allow_wildcard: bool,
    /// `None` = Portage's default rule.
    pub allow_repo: Option<bool>,
    /// `None` = Portage's default rule.
    pub allow_build_id: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomErrorKind {
    Invalid,
    EapiIncompatible,
    IuseMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomError {
    pub kind: AtomErrorKind,
    pub message: String,
}

impl AtomError {
    fn invalid(s: &str) -> AtomError {
        // An `InvalidAtom` without a message renders as the atom string itself
        // (that is what `str(e)` prints, oracle Q1).
        AtomError {
            kind: AtomErrorKind::Invalid,
            message: s.to_string(),
        }
    }
}

impl Atom {
    /// The version including its revision (`1.2.3-r1`), as fed to `vercmp`.
    fn full_version(&self) -> Option<String> {
        self.version.as_ref().map(|v| match &self.revision {
            Some(r) => format!("{v}-r{r}"),
            None => v.clone(),
        })
    }
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
        // Blockers are stripped before matching (Portage dep:1959-1967), so
        // this regex has no blocker group.
        Regex::new(&format!(
            r"^(?:(?P<op>=|>=|>|<=|<|~)(?P<vcat>{CAT})/(?P<vpkg>{PKG})(?P<bidtail>-(?P<tailver>{VER})(?:-r(?P<tailrev>[0-9]+))?)?-(?P<ver>{VER})(?:-r(?P<rev>[0-9]+))?(?P<glob>\*)?|(?P<cat>{CAT})/(?P<pkg>{PKG})(?P<ambiguous>-{VER}(?:-r[0-9]+)?)?)(?::(?P<slotpart>(?:(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?P<slotop>[*=])?))?(?:::(?P<repo>{REPO}))?(?P<usedeps>\[.*\])?$"
        ))
        .unwrap()
    })
}

/// Portage `_get_atom_wildcard_re` (dep:332-372): `simple` (extended category
/// / package, admitting a `-vr` tail only to reject it when it carries a
/// dotted component, dep:1988-1989) or `star` (`=cat/pkg-\*\w+\*`). Optional
/// `:slot` (`_slot_loose`) and `::repo`; no use deps. The slot groups reuse
/// the strict names so the second-stage slot validation is shared.
/// `wtaildot` is Portage's group `simple+3` (the `(\.\d+)` inside the tail):
/// `*/foo-1` and `*/foo-1-r1` parse as extended atoms, `*/foo-1.0` does not.
fn atom_wildcard_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?:(?P<simple>{EXTCAT}/{EXTPKG}(?P<wtail>-[0-9]+(?P<wtaildot>\.[0-9]+)*[a-z]?(?:_(?:pre|p|beta|alpha|rc)[0-9]*)*(?:-r[0-9]+)?)?)|=(?P<starcp>{EXTCAT}/{EXTPKG})-(?P<starver>\*[A-Za-z0-9_]+\*))(?::(?P<slotpart>(?:(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?P<slotop>[*=])?))?(?:::(?P<repo>{REPO}))?$"
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
/// `Err(token)` for what Portage's `Atom` rejects with
/// `Invalid use dep: '<token>'`: a bad token, `-flag=`/`-flag?`, or a flag
/// whose `(+)`/`(-)` default differs between tokens of the same atom.
/// Portage: lib/portage/dep/__init__.py, `_use_dep.__init__`
fn parse_use_deps(raw: &str) -> Result<Vec<UseDep>, String> {
    let Some(inner) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) else {
        return Err(raw.to_string());
    };
    let mut deps = Vec::new();
    // Default seen so far per flag; a later token with a different default state is invalid.
    let mut seen_defaults: std::collections::HashMap<String, Option<UseDepDefault>> =
        std::collections::HashMap::new();
    for token in inner.split(',') {
        let Some(caps) = use_dep_token_regex().captures(token) else {
            return Err(token.to_string());
        };
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
            // "-flag=" / "-flag?": syntactically matched, not a real operator
            _ => return Err(token.to_string()),
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
                    return Err(token.to_string());
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(default.clone());
            }
        }
        deps.push(UseDep { flag, op, default });
    }
    Ok(deps)
}

/// Strips the trailing `[use...]` block, leaving `cat/pkg[:slot[::repo]]`;
/// input without one is returned unchanged.
///
/// Mirrors Portage's `Atom.without_use`. Cuts at the first `[`, since the
/// USE block is always trailing (PMS 8.3.5).
pub fn without_use(atom_str: &str) -> &str {
    match atom_str.find('[') {
        Some(start) => &atom_str[..start],
        None => atom_str,
    }
}

pub fn parse_atom(s: &str) -> Option<Atom> {
    if let Some(hit) = ATOM_CACHE.with(|c| c.borrow().get(s).cloned()) {
        return hit;
    }
    let parsed = parse_atom_uncached(s);
    ATOM_CACHE.with(|c| {
        c.borrow_mut().insert(s.to_string(), parsed.clone());
    });
    parsed
}

fn parse_atom_uncached(s: &str) -> Option<Atom> {
    parse_atom_with_uncached(s, &AtomOptions::default(), "").ok()
}

/// Ports `Atom.__init__` with the caller's flags; `eapi_str` is the original
/// EAPI text, used only in messages ("… in EAPI {eapi_str}").
/// Portage: lib/portage/dep/__init__.py:1900-2153 (3.0.82.2).
///
/// Default rule (dep:1926-1937): `eapi` given ⇒ `allow_repo = repo_deps`
/// (False for every EAPI 0–9), `allow_build_id = false`; `eapi = None` ⇒ both
/// `true`. An explicit `Some(b)` always wins.
///
/// Post-parse checks run in Portage's order, `eapi` given only: `slot_deps`,
/// `use_deps`, `use_dep_defaults`, then `strong_blocks`. The conditional-flag
/// check sits between `use_dep_defaults` and `strong_blocks` in Portage; it
/// lives in `check_conditional_flags` (it needs the owner's IUSE, which would
/// not be hashable in the cache key). The order cannot change an observable
/// result: strong blocks are off only in EAPI 0/1, where use deps already fail
/// first (review I-1, proven live).
pub fn parse_atom_with(s: &str, opts: &AtomOptions, eapi_str: &str) -> Result<Atom, AtomError> {
    // Nested per-options map, looked up by `&str`: no allocation on a hit,
    // and the default-options `ATOM_CACHE` above stays exactly as it is.
    if let Some(hit) = ATOM_OPTS_CACHE.with(|c| {
        c.borrow()
            .get(opts)
            .and_then(|by_string| by_string.get(s))
            .cloned()
    }) {
        return hit;
    }
    let parsed = parse_atom_with_uncached(s, opts, eapi_str);
    ATOM_OPTS_CACHE.with(|c| {
        c.borrow_mut()
            .entry(*opts)
            .or_default()
            .insert(s.to_string(), parsed.clone());
    });
    parsed
}

fn slot_plain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Portage `_get_slot_dep_re` without `slot_operator`: plain `_slot`.
    RE.get_or_init(|| Regex::new(&format!(r"^{SLOT}$")).unwrap())
}

fn parse_atom_with_uncached(
    s: &str,
    opts: &AtomOptions,
    eapi_str: &str,
) -> Result<Atom, AtomError> {
    let attrs = match opts.eapi {
        AtomEapi::None | AtomEapi::Unsupported => {
            // Unsupported strings take the permissive set (eapi.py:247-287).
            eapi_attrs(None).expect("None never errs")
        }
        AtomEapi::Known(n) => {
            let key = SUPPORTED_EAPIS.get(n as usize).copied().unwrap_or("10");
            eapi_attrs(Some(key)).expect("a digit or an unsupported string never errs")
        }
    };
    let allow_repo = opts.allow_repo.unwrap_or(match opts.eapi {
        AtomEapi::None => true,
        AtomEapi::Known(_) | AtomEapi::Unsupported => attrs.repo_deps,
    });
    let allow_build_id = opts
        .allow_build_id
        .unwrap_or(matches!(opts.eapi, AtomEapi::None));

    // Blockers are stripped before either grammar (Portage dep:1959-1967);
    // `!!!x` strips two and leaves `!x`, which both grammars reject.
    let (blocker, rest) = if let Some(t) = s.strip_prefix("!!") {
        (Blocker::Strong, t)
    } else if let Some(t) = s.strip_prefix('!') {
        (Blocker::Weak, t)
    } else {
        (Blocker::None, s)
    };

    let strict = atom_regex().captures(rest);
    let wild;
    let (caps, extended): (&regex::Captures<'_>, bool) = match &strict {
        Some(m) => (m, false),
        None => {
            if !opts.allow_wildcard {
                return Err(AtomError::invalid(s));
            }
            wild = atom_wildcard_regex()
                .captures(rest)
                .ok_or_else(|| AtomError::invalid(s))?;
            (&wild, true)
        }
    };

    let AtomNamePartsOut {
        operator,
        category,
        package,
        version,
        revision,
        build_id,
    } = atom_name_parts(extended, caps, s, allow_build_id)?;

    // A bare trailing ":" matches the regex but is invalid.
    let (slot, sub_slot, slot_operator) = match caps.name("slotpart") {
        None => (None, None, None),
        Some(m) if m.as_str().is_empty() => return Err(AtomError::invalid(s)),
        Some(_) => {
            if attrs.slot_operator {
                let slot = caps.name("slot").map(|m| m.as_str().to_string());
                let sub_slot = caps.name("subslot").map(|m| m.as_str().to_string());
                let slot_operator = match caps.name("slotop").map(|m| m.as_str()) {
                    None => None,
                    // An explicit slot combined with "*" is invalid.
                    Some("*") if slot.is_some() => return Err(AtomError::invalid(s)),
                    Some("*") => Some(SlotOperator::Star),
                    Some("=") => Some(SlotOperator::Equals),
                    Some(_) => unreachable!(),
                };
                // Both parts are optional, so an empty match is possible.
                if slot.is_none() && slot_operator.is_none() {
                    return Err(AtomError::invalid(s));
                }
                (slot, sub_slot, slot_operator)
            } else {
                // Portage `_get_slot_dep_re` without `slot_operator`: the whole
                // text must be a plain slot, so `:0/1` and `:0=` are bare
                // `InvalidAtom`s here.
                let text = caps.name("slotpart").unwrap().as_str();
                if !slot_plain_re().is_match(text) {
                    return Err(AtomError::invalid(s));
                }
                (Some(text.to_string()), None, None)
            }
        }
    };

    let use_deps = match caps.name("usedeps") {
        None => None,
        Some(m) => Some(parse_use_deps(m.as_str()).map_err(|token| AtomError {
            kind: AtomErrorKind::Invalid,
            message: format!("Invalid use dep: '{token}'"),
        })?),
    };

    let repo = caps.name("repo").map(|m| m.as_str().to_string());
    if repo.is_some() && !allow_repo {
        return Err(AtomError::invalid(s));
    }

    check_atom_eapi_features(CheckAtomEapiFeaturesArgs {
        opts,
        slot: &slot,
        attrs,
        eapi_str,
        s,
        use_deps: &use_deps,
        blocker: &blocker,
    })?;

    Ok(Atom {
        blocker,
        operator,
        category,
        package,
        version,
        revision,
        slot,
        sub_slot,
        slot_operator,
        use_deps,
        repo,
        build_id,
        extended,
    })
}

/// What [`atom_name_parts`] hands back to its caller.
pub(crate) struct AtomNamePartsOut {
    pub(crate) operator: Operator,
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) build_id: Option<String>,
}

/// Split a parsed atom into operator, category, package, version, revision and build id.
pub(crate) fn atom_name_parts(
    extended: bool,
    caps: &Captures<'_>,
    s: &str,
    allow_build_id: bool,
) -> Result<AtomNamePartsOut, AtomError> {
    // The wildcard body (Portage dep:1972-1997): `simple` (extended
    // category/package, `*` allowed in either half) or `star`
    // (`=cat/pkg-\*\w+\*`, ASCII `\w`). A `simple` `-vr` tail rejects the
    // atom only when it carries a dotted component (dep:1988-1989 checks
    // group `simple+3`, the `(\.\d+)` inside the tail): `*/foo-1` and
    // `*/foo-1-r1` parse as extended atoms with the tail in the cp,
    // `*/foo-1.0` is invalid. `**` anywhere in the cpv is rejected
    // (dep:1990). Slot/repo/use/EAPI handling below is shared.
    let (operator, category, package, version, revision, build_id) = if extended {
        if caps.name("wtaildot").is_some() {
            return Err(AtomError::invalid(s));
        }
        if let Some(simple) = caps.name("simple") {
            if simple.as_str().contains("**") {
                return Err(AtomError::invalid(s));
            }
            let (cat, pkg) = simple
                .as_str()
                .split_once('/')
                .expect("wildcard simple always has cat/pkg");
            (
                Operator::None,
                cat.to_string(),
                pkg.to_string(),
                None,
                None,
                None,
            )
        } else {
            let starcp = caps
                .name("starcp")
                .expect("wildcard is simple or star")
                .as_str();
            let starver = caps
                .name("starver")
                .expect("star always has a version")
                .as_str();
            if format!("={starcp}-{starver}").contains("**") {
                return Err(AtomError::invalid(s));
            }
            let (cat, pkg) = starcp
                .split_once('/')
                .expect("wildcard star always has cat/pkg");
            (
                Operator::EqGlob,
                cat.to_string(),
                pkg.to_string(),
                Some(starver.to_string()),
                None,
                None,
            )
        }
    } else if let Some(op) = caps.name("op") {
        let tail = caps.name("bidtail");
        let glob = caps.name("glob");
        // PMS 8.3.1: `*` is illegal with any operator but `=`, and a
        // `=*` tail is always invalid (dep:2023-2033).
        if glob.is_some() && (op.as_str() != "=" || tail.is_some()) {
            return Err(AtomError::invalid(s));
        }
        let operator = if glob.is_some() {
            Operator::EqGlob
        } else {
            Operator::from_str(op.as_str())
        };
        // Portage's ambiguous `-vr` tail, tried before a longer package
        // name (dep:1998-2022): when present, the version comes from the
        // tail and the outer `-vr` is the build-id, valid only with
        // `allow_build_id` and only as plain digits (Portage parses it
        // with `int()`, dep:2014-2019). A leading zero is rejected.
        let (version, revision, build_id) = match tail {
            None => (
                Some(caps.name("ver").unwrap().as_str().to_string()),
                caps.name("rev").map(|m| m.as_str().to_string()),
                None,
            ),
            Some(_) => {
                if !allow_build_id {
                    return Err(AtomError::invalid(s));
                }
                let outer = caps.name("ver").unwrap().as_str();
                if caps.name("rev").is_some() || !outer.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(AtomError::invalid(s));
                }
                if outer.len() > 1 && outer.starts_with('0') {
                    return Err(AtomError::invalid(s));
                }
                (
                    Some(caps.name("tailver").unwrap().as_str().to_string()),
                    caps.name("tailrev").map(|m| m.as_str().to_string()),
                    Some(outer.to_string()),
                )
            }
        };
        (
            operator,
            caps.name("vcat").unwrap().as_str().to_string(),
            caps.name("vpkg").unwrap().as_str().to_string(),
            version,
            revision,
            build_id,
        )
    } else {
        // Ambiguous per PMS (`foo-bar-2`).
        // Portage: lib/portage/dep/__init__.py, Atom.__init__
        if caps.name("ambiguous").is_some() {
            return Err(AtomError::invalid(s));
        }
        (
            Operator::None,
            caps.name("cat").unwrap().as_str().to_string(),
            caps.name("pkg").unwrap().as_str().to_string(),
            None,
            None,
            None,
        )
    };
    Ok(AtomNamePartsOut {
        operator,
        category,
        package,
        version,
        revision,
        build_id,
    })
}

/// The values [`check_atom_eapi_features`] reads from its caller.
pub(crate) struct CheckAtomEapiFeaturesArgs<'a> {
    pub(crate) opts: &'a AtomOptions,
    pub(crate) slot: &'a Option<String>,
    pub(crate) attrs: EapiAttrs,
    pub(crate) eapi_str: &'a str,
    pub(crate) s: &'a str,
    pub(crate) use_deps: &'a Option<Vec<UseDep>>,
    pub(crate) blocker: &'a Blocker,
}

/// Reject atom features the EAPI does not allow: slot dependencies, use dependencies and the like.
pub(crate) fn check_atom_eapi_features(
    args: CheckAtomEapiFeaturesArgs<'_>,
) -> Result<(), AtomError> {
    let CheckAtomEapiFeaturesArgs {
        opts,
        slot,
        attrs,
        eapi_str,
        s,
        use_deps,
        blocker,
    } = args;
    if !matches!(opts.eapi, AtomEapi::None) {
        if slot.is_some() && !attrs.slot_deps {
            return Err(AtomError {
                kind: AtomErrorKind::EapiIncompatible,
                message: format!("Slot deps are not allowed in EAPI {eapi_str}: '{s}'"),
            });
        }
        if let Some(deps) = &use_deps {
            if !attrs.use_deps {
                return Err(AtomError {
                    kind: AtomErrorKind::EapiIncompatible,
                    message: format!("Use deps are not allowed in EAPI {eapi_str}: '{s}'"),
                });
            } else if !attrs.use_dep_defaults && deps.iter().any(|d| d.default.is_some()) {
                return Err(AtomError {
                    kind: AtomErrorKind::EapiIncompatible,
                    message: format!("Use dep defaults are not allowed in EAPI {eapi_str}: '{s}'"),
                });
            }
        }
        if *blocker == Blocker::Strong && !attrs.strong_blocks {
            return Err(AtomError {
                kind: AtomErrorKind::EapiIncompatible,
                message: format!("Strong blocks are not allowed in EAPI {eapi_str}: '{s}'"),
            });
        }
    }
    Ok(())
}

/// Portage `Atom.__init__`'s conditional-flag check (dep:2125-2144): every
/// conditional use-dep flag (`?`/`=` forms) must be valid in the owner's IUSE.
/// Runs only when `eapi` is given (with `eapi=None` Portage skips it); kept
/// separate from `parse_atom_with` so the cache key stays hashable.
///
/// Conditional types are visited in first-occurrence order (Portage iterates
/// `self.use.conditional`, a dict keyed by type); within one type the first
/// invalid flag in sorted order is reported (Portage iterates a set there, so
/// its pick among several is arbitrary — a deliberate micro-cut).
pub fn check_conditional_flags(
    atom: &Atom,
    eapi: AtomEapi,
    is_valid_flag: &dyn Fn(&str) -> bool,
) -> Result<(), AtomError> {
    if matches!(eapi, AtomEapi::None) {
        return Ok(());
    }
    let Some(deps) = &atom.use_deps else {
        return Ok(());
    };
    let mut order: Vec<&str> = Vec::new();
    let mut by_type: HashMap<&str, Vec<&str>> = HashMap::new();
    for ud in deps {
        let (cond_type, cond_str) = match ud.op {
            UseDepOp::IfParentEnabled => ("enabled", "?"),
            UseDepOp::IfParentDisabled => ("disabled", "!?"),
            UseDepOp::EqualParent => ("equal", "="),
            UseDepOp::OppositeParent => ("not_equal", "!="),
            UseDepOp::Enabled | UseDepOp::Disabled => continue,
        };
        let _ = cond_str;
        if !by_type.contains_key(cond_type) {
            order.push(cond_type);
        }
        by_type.entry(cond_type).or_default().push(ud.flag.as_str());
    }
    for cond_type in order {
        let mut flags = by_type[cond_type].clone();
        flags.sort_unstable();
        for flag in flags {
            if !is_valid_flag(flag) {
                let conditional = match cond_type {
                    "enabled" => format!("{flag}?"),
                    "disabled" => format!("!{flag}?"),
                    "equal" => format!("{flag}="),
                    _ => format!("!{flag}="),
                };
                return Err(AtomError {
                    kind: AtomErrorKind::IuseMissing,
                    message: format!(
                        "USE flag '{flag}' referenced in conditional '{conditional}' \
                         in atom '{}' is not in IUSE",
                        render_atom(atom),
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Rebuilds the atom text Portage prints for `str(atom)` (which is the
/// original string): blocker, operator, `cp-ver[-rN][-bid]`, slot, repo, use.
/// For a valid atom the rebuild equals the parsed token, so validators can
/// quote it (Portage's `unevaluated_atom` is that same original string).
pub fn atom_to_string(atom: &Atom) -> String {
    render_atom(atom)
}

/// Rebuilds the atom text Portage prints for `str(atom)` (which is the
/// original string): blocker, operator, `cp-ver[-rN][-bid]`, slot, repo, use.
fn render_atom(atom: &Atom) -> String {
    let mut out = String::new();
    match atom.blocker {
        Blocker::None => {}
        Blocker::Weak => out.push('!'),
        Blocker::Strong => out.push_str("!!"),
    }
    out.push_str(atom.operator.as_str());
    out.push_str(&atom.category);
    out.push('/');
    out.push_str(&atom.package);
    if let Some(v) = &atom.version {
        out.push('-');
        out.push_str(v);
        if let Some(r) = &atom.revision {
            out.push_str("-r");
            out.push_str(r);
        }
        if atom.operator == Operator::EqGlob && !atom.extended {
            out.push('*');
        } else if let Some(bid) = &atom.build_id {
            out.push('-');
            out.push_str(bid);
        }
    }
    if let Some(slot) = &atom.slot {
        out.push(':');
        out.push_str(slot);
        if let Some(sub) = &atom.sub_slot {
            out.push('/');
            out.push_str(sub);
        }
        match atom.slot_operator {
            None => {}
            Some(SlotOperator::Star) => out.push('*'),
            Some(SlotOperator::Equals) => out.push('='),
        }
    } else if let Some(sop) = &atom.slot_operator {
        out.push(':');
        match sop {
            SlotOperator::Star => out.push('*'),
            SlotOperator::Equals => out.push('='),
        }
    }
    if let Some(repo) = &atom.repo {
        out.push_str("::");
        out.push_str(repo);
    }
    if let Some(deps) = &atom.use_deps {
        out.push('[');
        out.push_str(
            &deps
                .iter()
                .map(|ud| {
                    let default = match ud.default {
                        None => "",
                        Some(UseDepDefault::Enabled) => "(+)",
                        Some(UseDepDefault::Disabled) => "(-)",
                    };
                    match ud.op {
                        UseDepOp::Enabled => format!("{}{default}", ud.flag),
                        UseDepOp::Disabled => format!("-{}{default}", ud.flag),
                        UseDepOp::IfParentEnabled => format!("{}{default}?", ud.flag),
                        UseDepOp::IfParentDisabled => format!("!{}{default}?", ud.flag),
                        UseDepOp::EqualParent => format!("{}{default}=", ud.flag),
                        UseDepOp::OppositeParent => format!("!{}{default}=", ud.flag),
                    }
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push(']');
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub category: String,
    pub package: String,
    pub version: String,
    pub revision: Option<String>,
    pub slot: Option<String>,
    pub sub_slot: Option<String>,
    /// `::reponame` suffix, `None` if absent ("repo unknown"); see `matches_repo`.
    pub repo: Option<String>,
    /// Binary build-id (Portage `_pkg_str.build_id`); always `None` from
    /// [`parse_candidate`] — a string never carries one, as `str(_pkg_str)`
    /// drops it (oracle `probe_match.py`: plain-string candidates match an
    /// `=`+build-id atom regardless). Callers holding a binary instance set
    /// it explicitly; see [`match_from_list_with_build_id`].
    /// Portage: lib/portage/dep/__init__.py:2994-3008.
    pub build_id: Option<String>,
}

impl Candidate {
    fn full_version(&self) -> String {
        match &self.revision {
            Some(r) => format!("{}-r{}", self.version, r),
            None => self.version.clone(),
        }
    }
}

fn candidate_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?P<cat>{CAT})/(?P<pkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>[0-9]+))?(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?:::(?P<repo>{REPO}))?$"
        ))
        .unwrap()
    })
}

pub fn parse_candidate(s: &str) -> Option<Candidate> {
    if let Some(hit) = CANDIDATE_CACHE.with(|c| c.borrow().get(s).cloned()) {
        return hit;
    }
    let parsed = parse_candidate_uncached(s);
    CANDIDATE_CACHE.with(|c| {
        c.borrow_mut().insert(s.to_string(), parsed.clone());
    });
    parsed
}

fn parse_candidate_uncached(s: &str) -> Option<Candidate> {
    let caps = candidate_regex().captures(s)?;
    Some(Candidate {
        category: caps.name("cat").unwrap().as_str().to_string(),
        package: caps.name("pkg").unwrap().as_str().to_string(),
        version: caps.name("ver").unwrap().as_str().to_string(),
        revision: caps.name("rev").map(|m| m.as_str().to_string()),
        slot: caps.name("slot").map(|m| m.as_str().to_string()),
        sub_slot: caps.name("subslot").map(|m| m.as_str().to_string()),
        repo: caps.name("repo").map(|m| m.as_str().to_string()),
        // A string never carries a build-id (`str(_pkg_str)` drops it);
        // binary callers set it after parsing (dep:2994-3008).
        build_id: None,
    })
}

/// Portage `extended_cp_match` (dep:2352-2365): each `*` becomes `[^/]*`,
/// anchored. Compiled regexes are memoised per pattern like Portage's
/// `_extended_cp_re_cache`.
pub fn extended_cp_match(pattern: &str, cp: &str) -> bool {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let re = c.entry(pattern.to_string()).or_insert_with(|| {
            Regex::new(&format!(
                "^{}$",
                regex::escape(pattern).replace(r"\*", "[^/]*")
            ))
            .expect("escaped pattern with ASCII class substitutions always compiles")
        });
        re.is_match(cp)
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

/// Version-operator check for a candidate that already matched category/package.
fn matches_version(atom: &Atom, candidate: &Candidate) -> bool {
    match atom.operator {
        Operator::None => true,
        Operator::Eq => {
            // The build-id is consulted only here, and only against a
            // candidate that has one: equal ids match, a candidate without
            // one never does (every portuale candidate models Portage's
            // `_pkg_str`/`Package`; plain strings match regardless in
            // Portage but never occur here). `>=`/`~`/`<`/`<=` ignore it.
            // Portage: lib/portage/dep/__init__.py:2994-3008; oracle
            // `probe_match.py` (`=dev-libs/foo-1.0-2` → only `#2`).
            if atom.build_id.is_some() && atom.build_id != candidate.build_id {
                return false;
            }
            vercmp(&candidate.full_version(), &atom.full_version().unwrap()) == Some(0)
        }
        // `=*` is a literal prefix match, not vercmp, but only at a component
        // boundary: "1*" must not match "10".
        // Portage: lib/portage/dep/__init__.py, match_from_list
        Operator::EqGlob => {
            if atom.extended {
                // Extended `=*` is a substring test on the candidate's
                // `_pkg_str` version (`ver[-rN]`, without `-r0`; dep:2954-2972).
                // Portuale candidates model `_pkg_str`, so a plain-string-only
                // `-r0` never appears here (review M-3): `=*/foo-*r0*` matches
                // no portuale candidate, while Portage would match the plain
                // string `"dev-libs/foo-1.0"`. The atom version keeps its
                // stars (`*1*`); Portage strips them (`mydep.version[1:-1]`).
                let ver = atom.version.as_ref().expect("star always has a version");
                let inner = ver
                    .strip_prefix('*')
                    .and_then(|v| v.strip_suffix('*'))
                    .expect("star version is `*…*`");
                return candidate.full_version().contains(inner);
            }
            let atom_cmp = glob_compare_string(atom.version.as_ref().unwrap(), &atom.revision);
            let cand_cmp = glob_compare_string(&candidate.version, &candidate.revision);
            let Some(rest) = cand_cmp.strip_prefix(&atom_cmp) else {
                return false;
            };
            match rest.chars().next() {
                None => true,
                Some('.' | '_' | '-') => true,
                Some(next) => {
                    let last_is_digit = atom_cmp
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_digit());
                    last_is_digit != next.is_ascii_digit()
                }
            }
        }
        Operator::Tilde => candidate.version == *atom.version.as_ref().unwrap(),
        Operator::Gt | Operator::Ge | Operator::Lt | Operator::Le => {
            match vercmp(&candidate.full_version(), &atom.full_version().unwrap()) {
                None => false,
                Some(cmp) => match atom.operator {
                    Operator::Gt => cmp > 0,
                    Operator::Ge => cmp >= 0,
                    Operator::Lt => cmp < 0,
                    Operator::Le => cmp <= 0,
                    _ => unreachable!(),
                },
            }
        }
    }
}

/// Exact slot match; sub-slot only if the atom has one. A candidate with no
/// slot always passes.
///
/// `slot_operator` is deliberately ignored, as in Portage's `_match_slot`: a
/// bare `:=`/`:*` has no slot so matches any, and `:slot=` filters like `:slot`.
fn matches_slot(atom: &Atom, candidate: &Candidate) -> bool {
    let Some(atom_slot) = &atom.slot else {
        return true;
    };
    let Some(candidate_slot) = &candidate.slot else {
        return true;
    };
    if candidate_slot != atom_slot {
        return false;
    }
    match &atom.sub_slot {
        None => true,
        Some(atom_sub) => candidate.sub_slot.as_deref() == Some(atom_sub.as_str()),
    }
}

/// Rejects only a candidate carrying a known, different repo; a repo-less
/// candidate means "unknown" and always passes. An atom without `::repo`
/// never filters.
/// Portage: lib/portage/dep/__init__.py, match_from_list (final repo filter)
fn matches_repo(atom: &Atom, candidate: &Candidate) -> bool {
    match &atom.repo {
        None => true,
        Some(want) => match &candidate.repo {
            None => true,
            // Portage's `_unknown_repo` also passes any `::repo` atom.
            Some(have) => have == want || have == "__unknown__",
        },
    }
}

/// Whether a candidate's `iuse` (declared flags, `+`/`-` markers stripped)
/// and `enabled` (effective USE) satisfy `use_deps`.
///
/// Not called by `match_from_list` (plain-string candidates carry no USE
/// state); callers with IUSE/USE in hand apply it afterwards.
///
/// - A flag without a `(+)`/`(-)` default must be in `iuse`, for every form.
/// - Only `Enabled`/`Disabled` constrain USE state; the four conditional
///   forms impose none here, as in Portage. A default applies only to a flag
///   missing from `iuse`.
///
/// Portage: lib/portage/dep/__init__.py, match_from_list (`unevaluated_atom.use` block)
/// `use_deps_satisfied` against a split validity domain: a flag is valid
/// when `declared` or any of `unions` names it. Lets callers skip building
/// the `declared ∪ iuse_effective (∪ …)` union per check — the union exists
/// only for these `contains` calls, and the effective set is large and
/// resolve-constant while the declared set is small and per-candidate.
pub fn use_deps_satisfied_union(
    use_deps: &[UseDep],
    declared: &HashSet<String>,
    unions: &[&HashSet<String>],
    enabled: &HashSet<String>,
) -> bool {
    let valid = |flag: &str| declared.contains(flag) || unions.iter().any(|u| u.contains(flag));
    if use_deps
        .iter()
        .any(|ud| ud.default.is_none() && !valid(&ud.flag))
    {
        return false;
    }

    let missing_enabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.default == Some(UseDepDefault::Enabled) && !valid(&ud.flag))
        .map(|ud| ud.flag.as_str())
        .collect();
    let missing_disabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.default == Some(UseDepDefault::Disabled) && !valid(&ud.flag))
        .map(|ud| ud.flag.as_str())
        .collect();

    let required_enabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.op == UseDepOp::Enabled)
        .map(|ud| ud.flag.as_str())
        .collect();
    let required_disabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.op == UseDepOp::Disabled)
        .map(|ud| ud.flag.as_str())
        .collect();

    if !required_enabled.is_empty() {
        if required_enabled
            .iter()
            .any(|f| missing_disabled.contains(f))
        {
            return false;
        }
        let need_enabled: Vec<&str> = required_enabled
            .iter()
            .filter(|f| !enabled.contains(**f))
            .copied()
            .collect();
        if !need_enabled.is_empty() && need_enabled.iter().any(|f| !missing_enabled.contains(f)) {
            return false;
        }
    }

    if !required_disabled.is_empty() {
        if required_disabled
            .iter()
            .any(|f| missing_enabled.contains(f))
        {
            return false;
        }
        let need_disabled: Vec<&str> = required_disabled
            .iter()
            .filter(|f| enabled.contains(**f))
            .copied()
            .collect();
        if !need_disabled.is_empty() && need_disabled.iter().any(|f| !missing_disabled.contains(f))
        {
            return false;
        }
    }

    true
}

/// Convenience for a prebuilt validity domain (see `valid_iuse`): exactly
/// `use_deps_satisfied_union` with no extra union sets.
pub fn use_deps_satisfied(
    use_deps: &[UseDep],
    iuse: &HashSet<String>,
    enabled: &HashSet<String>,
) -> bool {
    use_deps_satisfied_union(use_deps, iuse, &[], enabled)
}

/// The `("use", flag)` reason keys one parent atom's `use_deps` contribute
/// against a conflicting `other` instance: `(unconditional, violated)`.
///
/// - Unconditional: default-less flags absent from `other`'s IUSE; when
///   non-empty, the violated check is skipped.
/// - Violated: `[x]` with `x` off in `other` (while valid, or with a `(-)`
///   default); `[-x]` with `x` on (or invalid with a `(+)` default).
/// - Conditional forms never yield keys.
///
/// "Valid" flag means plain declared-IUSE membership (Portage also consults
/// implicit IUSE).
/// Portage: lib/portage/dep/slot_collision.py, _prepare_conflict_msg_and_check_for_specificity
pub fn use_mismatch_flags(
    use_deps: &[UseDep],
    other_use: &HashSet<String>,
    other_iuse: &HashSet<String>,
) -> (HashSet<String>, HashSet<String>) {
    let missing: HashSet<String> = use_deps
        .iter()
        .filter(|ud| ud.default.is_none() && !other_iuse.contains(&ud.flag))
        .map(|ud| ud.flag.clone())
        .collect();
    if !missing.is_empty() {
        return (missing, HashSet::new());
    }
    let mut violated = HashSet::new();
    for ud in use_deps {
        let hit = match ud.op {
            UseDepOp::Enabled => {
                !other_use.contains(&ud.flag)
                    && (other_iuse.contains(&ud.flag)
                        || ud.default == Some(UseDepDefault::Disabled))
            }
            UseDepOp::Disabled => {
                other_use.contains(&ud.flag)
                    || (!other_iuse.contains(&ud.flag)
                        && ud.default == Some(UseDepDefault::Enabled))
            }
            _ => false,
        };
        if hit {
            violated.insert(ud.flag.clone());
        }
    }
    (HashSet::new(), violated)
}

/// Whether any of `use_deps` is violated by `child_use`/`child_iuse` given the
/// recording package's `parent_use`: parent-conditionals are evaluated first,
/// then checked as in `use_deps_satisfied`.
/// Portage: lib/portage/dep/__init__.py, Atom.violated_conditionals
pub fn use_deps_violated(
    use_deps: &[UseDep],
    parent_use: &HashSet<String>,
    child_use: &HashSet<String>,
    child_iuse: &HashSet<String>,
) -> bool {
    let evaluated = evaluate_use_dep_conditionals(use_deps, parent_use);
    !use_deps_satisfied(&evaluated, child_iuse, child_use)
}

/// Renders one `UseDep` back to its atom-string token (inverse of `parse_use_deps`).
fn render_use_dep(ud: &UseDep) -> String {
    let default = match ud.default {
        None => "",
        Some(UseDepDefault::Enabled) => "(+)",
        Some(UseDepDefault::Disabled) => "(-)",
    };
    match ud.op {
        UseDepOp::Enabled => format!("{}{default}", ud.flag),
        UseDepOp::Disabled => format!("-{}{default}", ud.flag),
        UseDepOp::IfParentEnabled => format!("{}?{default}", ud.flag),
        UseDepOp::IfParentDisabled => format!("!{}?{default}", ud.flag),
        UseDepOp::EqualParent => format!("{}={default}", ud.flag),
        UseDepOp::OppositeParent => format!("!{}={default}", ud.flag),
    }
}

/// Resolves the four parent-conditional forms against `parent_use`, the
/// atom-owning package's effective USE set:
///
/// ```text
///     parent state   conditional   result
///      x              x?            x
///     -x              x?            (dropped -- no constraint at all)
///      x             !x?            (dropped -- no constraint at all)
///     -x             !x?           -x
///      x              x=            x
///     -x              x=           -x
///      x             !x=           -x
///     -x             !x=            x
/// ```
///
/// A dropped token imposes no constraint; an empty result means "no use-dep".
/// `Enabled`/`Disabled` and any `(+)`/`(-)` default pass through unchanged.
/// Portage: lib/portage/dep/__init__.py, Atom.evaluate_conditionals
pub fn evaluate_use_dep_conditionals(
    use_deps: &[UseDep],
    parent_use: &HashSet<String>,
) -> Vec<UseDep> {
    use_deps
        .iter()
        .filter_map(|ud| {
            let op = match ud.op {
                UseDepOp::Enabled | UseDepOp::Disabled => ud.op.clone(),
                UseDepOp::IfParentEnabled => {
                    if parent_use.contains(&ud.flag) {
                        UseDepOp::Enabled
                    } else {
                        return None;
                    }
                }
                UseDepOp::IfParentDisabled => {
                    if parent_use.contains(&ud.flag) {
                        return None;
                    } else {
                        UseDepOp::Disabled
                    }
                }
                UseDepOp::EqualParent => {
                    if parent_use.contains(&ud.flag) {
                        UseDepOp::Enabled
                    } else {
                        UseDepOp::Disabled
                    }
                }
                UseDepOp::OppositeParent => {
                    if parent_use.contains(&ud.flag) {
                        UseDepOp::Disabled
                    } else {
                        UseDepOp::Enabled
                    }
                }
            };
            Some(UseDep {
                flag: ud.flag.clone(),
                op,
                default: ud.default.clone(),
            })
        })
        .collect()
}

/// Applies `evaluate_use_dep_conditionals` to the atom string itself,
/// rewriting its `[...]` block.
///
/// Returns the input unchanged when it has no conditional use-deps, and `None`
/// if it doesn't parse. Lives here because `portage-use-reduce` stays
/// atom-agnostic; callers apply it per flattened token.
/// Portage: lib/portage/dep/__init__.py, use_reduce (per-token `evaluate_conditionals`)
pub fn evaluate_atom_conditionals(atom_str: &str, parent_use: &HashSet<String>) -> Option<String> {
    let atom = parse_atom(atom_str)?;
    let Some(use_deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) else {
        return Some(atom_str.to_string());
    };
    if !use_deps.iter().any(|ud| {
        matches!(
            ud.op,
            UseDepOp::IfParentEnabled
                | UseDepOp::IfParentDisabled
                | UseDepOp::EqualParent
                | UseDepOp::OppositeParent
        )
    }) {
        return Some(atom_str.to_string());
    }
    let evaluated = evaluate_use_dep_conditionals(use_deps, parent_use);

    let caps = atom_regex().captures(atom_str)?;
    let bracket_start = match caps.name("usedeps") {
        Some(m) => m.start(),
        None => atom_str.len(),
    };
    let bracket_end = atom_str.len();
    let new_bracket = if evaluated.is_empty() {
        String::new()
    } else {
        format!(
            "[{}]",
            evaluated
                .iter()
                .map(render_use_dep)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    Some(format!(
        "{}{new_bracket}",
        &atom_str[..bracket_start.min(bracket_end)]
    ))
}

/// One parsed candidate against one parsed atom: cp, version (plus the
/// `=`-only build-id rule), slot, repo.
fn candidate_hits(atom: &Atom, candidate: &Candidate) -> bool {
    // Portage dep:2934-2943: exact cp or `extended_cp_match`.
    let exact = atom.category == candidate.category && atom.package == candidate.package;
    if !exact
        && !(atom.extended
            && extended_cp_match(
                &format!("{}/{}", atom.category, atom.package),
                &format!("{}/{}", candidate.category, candidate.package),
            ))
    {
        return false;
    }
    matches_version(atom, candidate)
        && matches_slot(atom, candidate)
        && matches_repo(atom, candidate)
}

/// Parses `atom_str` the way [`match_from_list`] does: leading blockers
/// stripped, then re-parsed with `allow_wildcard=True, allow_repo=True`
/// like Portage (dep:2901-2906). `None` if the atom doesn't parse.
pub fn parse_match_atom(atom_str: &str) -> Option<Atom> {
    let opts = AtomOptions {
        allow_wildcard: true,
        allow_repo: Some(true),
        ..AtomOptions::default()
    };
    parse_atom_with(atom_str.trim_start_matches('!'), &opts, "").ok()
}

/// One candidate string plus an optional binary build-id against an
/// already-parsed match atom: the per-candidate form of
/// [`match_from_list_with_build_id`], for callers whose candidates share
/// one string (binpkg-multi-instance builds) and cannot be told apart
/// after matching. Unparseable candidates never match.
pub fn match_atom_matches_candidate(
    atom: &Atom,
    candidate_str: &str,
    build_id: Option<&str>,
) -> bool {
    let Some(mut candidate) = parse_candidate(candidate_str) else {
        return false;
    };
    if let Some(id) = build_id {
        candidate.build_id = Some(id.to_string());
    }
    candidate_hits(atom, &candidate)
}

/// One match-mode pair plus an optional binary build-id: `None` when the
/// atom doesn't parse (cached too, so a bad atom parses once). Every
/// caller parses match-mode (`parse_match_atom`), so the three strings
/// are the whole key.
// Portage: lib/portage/dep/__init__.py, match_from_list
fn match_pair(atom_str: &str, candidate: &str, build_id: Option<&str>) -> Option<bool> {
    let key = (
        atom_str.to_string(),
        candidate.to_string(),
        build_id.map(str::to_string),
    );
    if let Some(hit) = MATCH_CACHE.with(|m| m.borrow().get(&key).copied()) {
        return hit;
    }
    let verdict = parse_match_atom(atom_str)
        .map(|atom| match_atom_matches_candidate(&atom, candidate, build_id));
    MATCH_CACHE.with(|m| {
        m.borrow_mut().insert(key, verdict);
    });
    verdict
}

/// Returns the candidates (input order) matching `atom_str`; `None` if the
/// atom doesn't parse. Unparseable candidates are skipped.
/// Portage: lib/portage/dep/__init__.py, match_from_list
pub fn match_from_list<'a>(atom_str: &str, candidates: &[&'a str]) -> Option<Vec<&'a str>> {
    // The empty-list verdict still depends on the parse: an unparseable atom
    // is `None` even with no candidates (callers branch on `None` vs `Some`).
    if candidates.is_empty() {
        return parse_match_atom(atom_str).map(|_| Vec::new());
    }
    let mut out = Vec::with_capacity(candidates.len());
    for &c in candidates {
        if match_pair(atom_str, c, None)? {
            out.push(c);
        }
    }
    Some(out)
}

/// [`match_from_list`] where each candidate may carry a binary build-id
/// (Portage `_pkg_str.build_id`, which `str()` drops, so a plain string
/// never has one): `None` keeps the string-path behaviour (an `=`+build-id
/// atom matches nothing), `Some(id)` matches an `=`+build-id atom only on
/// equal ids. Every other operator ignores the id, as in Portage.
/// Portage: lib/portage/dep/__init__.py:2994-3008; oracle `probe_match.py`.
pub fn match_from_list_with_build_id<'a>(
    atom_str: &str,
    candidates: &[(&'a str, Option<&str>)],
) -> Option<Vec<&'a str>> {
    if candidates.is_empty() {
        return parse_match_atom(atom_str).map(|_| Vec::new());
    }
    let mut out = Vec::with_capacity(candidates.len());
    for &(c, build_id) in candidates {
        if match_pair(atom_str, c, build_id)? {
            out.push(c);
        }
    }
    Some(out)
}

/// A deliberately narrow check, as in Portage's `Atom.intersects`: cp,
/// use-deps, operator and full version must all be equal, and slots must be
/// compatible (either `None`, or equal). `repo` is not checked; the caller
/// adds its own.
pub fn atom_intersects(a: &Atom, b: &Atom) -> bool {
    if a.category != b.category
        || a.package != b.package
        || a.use_deps != b.use_deps
        || a.operator != b.operator
        || a.full_version() != b.full_version()
    {
        return false;
    }
    a.slot.is_none() || b.slot.is_none() || a.slot == b.slot
}

// --- Extended (wildcard) atoms are parsed by `parse_atom_with` with
// `allow_wildcard` (Portage `_get_atom_wildcard_re`) and matched by
// `match_from_list` via `extended_cp_match`; the old bounded
// `WildcardAtom`/`parse_wildcard_atom`/`wildcard_atom_matches` API is gone
// (S3 moved its one caller, `matches_config_entry`, onto `match_from_list`).

#[cfg(test)]
mod parse_cache_tests {
    use super::*;

    // The memo cache must be transparent: cached calls equal the uncached body, hit or miss.
    const ATOM_CASES: &[&str] = &[
        ">=dev-libs/foo-1.2.3-r1:2/3=[bar,-baz]",
        "!!sys-apps/portage",
        "dev-libs/foo",
        "=cat/pkg-1*",
        "net-libs/rest:0/0::gentoo",
        "this is not an atom",
        "dev-libs/foo:",
        "",
    ];
    const CANDIDATE_CASES: &[&str] = &[
        "dev-libs/foo-1.2.3-r1:2/3::gentoo",
        "net-libs/rest-0.10.2",
        "sys-apps/portage-3.0.0:0",
        "not a candidate",
        "",
    ];

    #[test]
    fn cached_parse_atom_matches_uncached_and_is_stable_across_calls() {
        for &s in ATOM_CASES {
            let want = parse_atom_uncached(s);
            assert_eq!(parse_atom(s), want, "first cached call for {s:?}");
            assert_eq!(parse_atom(s), want, "second cached call for {s:?}");
        }
    }

    #[test]
    fn cached_parse_candidate_matches_uncached_and_is_stable_across_calls() {
        for &s in CANDIDATE_CASES {
            let want = parse_candidate_uncached(s);
            assert_eq!(parse_candidate(s), want, "first cached call for {s:?}");
            assert_eq!(parse_candidate(s), want, "second cached call for {s:?}");
        }
    }

    #[test]
    fn cached_match_from_list_matches_fresh_parse_and_is_stable_across_calls() {
        let pairs: &[(&str, &str)] = &[
            (">=dev-libs/foo-1.2.3", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("=dev-libs/foo-1*", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("!dev-libs/foo", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("dev-libs/foo:2", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("dev-libs/foo:9", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("this is not an atom", "dev-libs/foo-1.2.3-r1:2/3::gentoo"),
            ("dev-libs/foo", "not a candidate"),
        ];
        for &(a, c) in pairs {
            let atom = parse_match_atom(a);
            let want = atom
                .as_ref()
                .map(|atom| match_atom_matches_candidate(atom, c, None));
            // `match_from_list` on one candidate is the memoised pair verdict.
            let single = match_from_list(a, &[c]);
            assert_eq!(
                single,
                want.map(|hit| if hit { vec![c] } else { Vec::new() }),
                "pair {a:?} vs {c:?}"
            );
            assert_eq!(match_from_list(a, &[c]), single, "repeat for {a:?}");
            assert_eq!(match_pair(a, c, None), want, "direct pair for {a:?}");
            assert_eq!(
                match_from_list_with_build_id(a, &[(c, None)]),
                single,
                "build-id path with no id for {a:?}"
            );
        }
        // The empty-list verdict still tracks the parse.
        assert_eq!(match_from_list("dev-libs/foo", &[]), Some(Vec::new()));
        assert_eq!(match_from_list("this is not an atom", &[]), None);
    }
}

#[cfg(test)]
mod without_use_tests {
    use super::*;

    // Cuts at the first `[` (the USE block is trailing, PMS 8.3.5); no block: unchanged.
    #[test]
    fn strips_trailing_use_block_leaving_the_rest_untouched() {
        assert_eq!(without_use("dev-libs/foo[bar]"), "dev-libs/foo");
        assert_eq!(
            without_use(">=dev-libs/foo-1.2.3-r1:2/3=[bar,-baz]"),
            ">=dev-libs/foo-1.2.3-r1:2/3="
        );
        assert_eq!(
            without_use("net-libs/rest:0/0::gentoo[x y]"),
            "net-libs/rest:0/0::gentoo"
        );
        assert_eq!(without_use("sys-apps/portage"), "sys-apps/portage");
        assert_eq!(without_use(""), "");
        // A malformed double-bracket string cuts at the first `[` too, like Portage.
        assert_eq!(without_use("cat/pkg[flag][other]"), "cat/pkg");
    }
}

/// An element of `extract_affecting_use`'s parse stack: a token or a nested group.
#[derive(Clone, Debug, PartialEq)]
enum Aff {
    Tok(String),
    Group(Vec<Aff>),
}

/// Whether the element ends in `?`, recursing into a group's last element.
fn aff_ends_q(e: &Aff) -> bool {
    match e {
        Aff::Tok(s) => s.ends_with('?'),
        Aff::Group(v) => v.last().is_some_and(aff_ends_q),
    }
}

/// Whether the element is the `||` token.
fn aff_is_barbar(e: &Aff) -> bool {
    matches!(e, Aff::Tok(s) if s == "||")
}

fn affecting_useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Modern-EAPI useflag charset; EAPI is not parametrized.
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}

/// Strips a leading `!` and the trailing `?`, then validates the flag name;
/// `None` where Portage raises `InvalidDependString`.
fn aff_cond_flag(tok: &str) -> Option<String> {
    let body = tok.strip_prefix('!').unwrap_or(tok);
    let mut chars = body.chars();
    chars.next_back();
    let flag = chars.as_str();
    if affecting_useflag_re().is_match(flag) {
        Some(flag.to_string())
    } else {
        None
    }
}

/// Folds `l` back into `stack[lvl]`, dropping redundant brackets (Portage's `special_append`).
fn aff_special_append(stack: &mut [Vec<Aff>], lvl: usize, is_single: bool, l: &[Aff]) {
    let keep_flat = is_single && !stack[lvl].last().is_some_and(aff_ends_q);
    if keep_flat {
        if l.len() == 1
            && let Aff::Group(inner) = &l[0]
        {
            stack[lvl].extend(inner.iter().cloned());
            return;
        }
        stack[lvl].extend(l.iter().cloned());
    } else {
        stack[lvl].push(Aff::Group(l.to_vec()));
    }
}

/// Port of `portage.dep.extract_affecting_use`: the USE flags whose `flag?`
/// conditionals decide whether `atom` (an exact whitespace token) is in
/// effect inside `dep`. `None` on malformed `dep` (Portage raises
/// `InvalidDependString`). Not EAPI-parametrized.
///
/// ```
/// # use std::collections::HashSet;
/// let got = portage_dep::extract_affecting_use(
///     "sasl? ( dev-libs/cyrus-sasl ) !minimal? ( cxx? ( dev-libs/cyrus-sasl ) )",
///     "dev-libs/cyrus-sasl",
/// ).unwrap();
/// assert_eq!(got, HashSet::from(["cxx".to_string(), "minimal".to_string(), "sasl".to_string()]));
/// ```
pub fn extract_affecting_use(dep: &str, atom: &str) -> Option<HashSet<String>> {
    let mut level: i32 = 0;
    let mut stack: Vec<Vec<Aff>> = vec![Vec::new()];
    let mut need_bracket = false;
    let mut affecting: HashSet<String> = HashSet::new();

    for token in dep.split_whitespace() {
        if token == "(" {
            need_bracket = false;
            stack.push(Vec::new());
            level += 1;
        } else if token == ")" {
            if need_bracket || level <= 0 {
                return None;
            }
            level -= 1;
            let lvl = level as usize;
            let l = stack.pop().unwrap();
            let is_single =
                l.len() == 1 || (l.len() == 2 && (aff_is_barbar(&l[0]) || aff_ends_q(&l[0])));

            // Predicates over the still-unmutated enclosing frames.
            let ends_in_any_of_dep = |k: i32| -> bool {
                k >= 0
                    && stack
                        .get(k as usize)
                        .and_then(|s| s.last())
                        .is_some_and(aff_is_barbar)
            };
            let ends_in_operator = |k: i32| -> bool {
                k >= 0
                    && stack
                        .get(k as usize)
                        .and_then(|s| s.last())
                        .is_some_and(|e| aff_is_barbar(e) || aff_ends_q(e))
            };
            let eiad_prev = ends_in_any_of_dep(level - 1);
            let eio_cur = ends_in_operator(level);
            let eiad_cur = ends_in_any_of_dep(level);

            if l.is_empty() {
                if stack[lvl]
                    .last()
                    .is_some_and(|e| aff_is_barbar(e) || aff_ends_q(e))
                {
                    stack[lvl].pop();
                }
                continue;
            }

            if !eiad_prev && !eio_cur {
                stack[lvl].extend(l);
            } else if stack[lvl].is_empty() {
                aff_special_append(&mut stack, lvl, is_single, &l);
            } else if l.len() == 1 && eiad_cur {
                stack[lvl].pop();
                aff_special_append(&mut stack, lvl, is_single, &l);
            } else if l.len() == 2
                && (aff_is_barbar(&l[0]) || aff_ends_q(&l[0]))
                && stack[lvl]
                    .last()
                    .is_some_and(|e| *e == l[0] || aff_is_barbar(e))
            {
                stack[lvl].pop();
                aff_special_append(&mut stack, lvl, is_single, &l);
                if aff_ends_q(&l[0]) {
                    let Aff::Tok(t) = &l[0] else { return None };
                    affecting.insert(aff_cond_flag(t)?);
                }
            } else {
                if let Some(last) = stack[lvl].last()
                    && aff_ends_q(last)
                {
                    let Aff::Tok(t) = last else { return None };
                    let f = aff_cond_flag(t)?;
                    affecting.insert(f);
                }
                aff_special_append(&mut stack, lvl, is_single, &l);
            }
        } else if token == "||" {
            if need_bracket {
                return None;
            }
            need_bracket = true;
            stack[level as usize].push(Aff::Tok(token.to_string()));
        } else {
            if need_bracket {
                return None;
            }
            if token.ends_with('?') {
                need_bracket = true;
                stack[level as usize].push(Aff::Tok(token.to_string()));
            } else if token == atom {
                stack[level as usize].push(Aff::Tok(token.to_string()));
            }
        }
    }

    if level != 0 || need_bracket {
        return None;
    }
    Some(affecting)
}

#[cfg(test)]
mod use_dep_satisfaction_tests {
    use super::*;

    fn use_deps(atom_str: &str) -> Vec<UseDep> {
        parse_atom(atom_str)
            .expect("atom must parse")
            .use_deps
            .expect("atom must carry use deps")
    }

    #[test]
    fn plain_flag_requires_it_declared_and_enabled() {
        let ud = use_deps("dev-libs/foo[bar]");
        let iuse = HashSet::from(["bar".to_string()]);
        assert!(use_deps_satisfied(
            &ud,
            &iuse,
            &HashSet::from(["bar".to_string()])
        ));
        assert!(!use_deps_satisfied(&ud, &iuse, &HashSet::new()));
    }

    #[test]
    fn negated_flag_requires_it_declared_and_disabled() {
        let ud = use_deps("dev-libs/foo[-bar]");
        let iuse = HashSet::from(["bar".to_string()]);
        assert!(use_deps_satisfied(&ud, &iuse, &HashSet::new()));
        assert!(!use_deps_satisfied(
            &ud,
            &iuse,
            &HashSet::from(["bar".to_string()])
        ));
    }

    #[test]
    fn flag_not_in_iuse_at_all_never_matches_without_a_default() {
        // A default-less flag must be declared in IUSE, whatever its USE state.
        let ud = use_deps("dev-libs/foo[bar]");
        assert!(!use_deps_satisfied(
            &ud,
            &HashSet::new(),
            &HashSet::from(["bar".to_string()])
        ));
    }

    #[test]
    fn plus_default_treats_a_missing_flag_as_enabled() {
        let ud = use_deps("dev-libs/foo[bar(+)]");
        // "bar" is undeclared: the (+) default stands in for "enabled".
        assert!(use_deps_satisfied(&ud, &HashSet::new(), &HashSet::new()));
    }

    #[test]
    fn minus_default_treats_a_missing_flag_as_disabled() {
        let ud = use_deps("dev-libs/foo[-bar(-)]");
        assert!(use_deps_satisfied(&ud, &HashSet::new(), &HashSet::new()));
    }

    #[test]
    fn plus_default_does_not_rescue_a_declared_but_disabled_flag() {
        // "bar" is declared, so the (+) default is moot; its real (disabled) state governs.
        let ud = use_deps("dev-libs/foo[bar(+)]");
        let iuse = HashSet::from(["bar".to_string()]);
        assert!(!use_deps_satisfied(&ud, &iuse, &HashSet::new()));
    }

    #[test]
    fn conditional_forms_only_require_the_flag_be_declared_no_state_constraint() {
        // Conditional forms impose no state constraint; only the declared-IUSE gate applies.
        for atom_str in [
            "dev-libs/foo[bar?]",
            "dev-libs/foo[!bar?]",
            "dev-libs/foo[bar=]",
            "dev-libs/foo[!bar=]",
        ] {
            let ud = use_deps(atom_str);
            let iuse = HashSet::from(["bar".to_string()]);
            assert!(
                use_deps_satisfied(&ud, &iuse, &HashSet::new()),
                "{atom_str} with bar disabled"
            );
            assert!(
                use_deps_satisfied(&ud, &iuse, &HashSet::from(["bar".to_string()])),
                "{atom_str} with bar enabled"
            );
            assert!(
                !use_deps_satisfied(&ud, &HashSet::new(), &HashSet::new()),
                "{atom_str} with bar undeclared"
            );
        }
    }

    /// Candidate state for a Portage test vector, built from its construction
    /// atom as Portage's `Package` mock does: `iuse` = default-less flags,
    /// `enabled` = bare flags. Vectors: lib/portage/tests/dep/test_match_from_list.py
    fn enabled_of(atom_str: &str) -> HashSet<String> {
        use_deps(atom_str)
            .into_iter()
            .filter(|ud| ud.op == UseDepOp::Enabled)
            .map(|ud| ud.flag)
            .collect()
    }

    fn iuse_of(atom_str: &str) -> HashSet<String> {
        use_deps(atom_str)
            .into_iter()
            .filter(|ud| ud.default.is_none())
            .map(|ud| ud.flag)
            .collect()
    }

    #[test]
    fn portage_test_suite_vector_foo_and_bar_both_required_neither_declares_bar() {
        let ud = use_deps("dev-libs/A[foo,bar]");
        // Package("=dev-libs/A-1[foo]") and Package("=dev-libs/A-2[-foo]")
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-1[foo]"),
            &enabled_of("=dev-libs/A-1[foo]")
        ));
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-2[-foo]"),
            &enabled_of("=dev-libs/A-2[-foo]")
        ));
    }

    #[test]
    fn portage_test_suite_vector_foo_and_bar_both_required_one_satisfies() {
        let ud = use_deps("dev-libs/A[foo,bar]");
        // Package("=dev-libs/A-1[foo]") -> foo declared+enabled, but bar
        // never declared at all -> still rejected.
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-1[foo]"),
            &enabled_of("=dev-libs/A-1[foo]")
        ));
        // Package("=dev-libs/A-2[foo,bar]") -> both declared and enabled.
        assert!(use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-2[foo,bar]"),
            &enabled_of("=dev-libs/A-2[foo,bar]")
        ));
    }

    #[test]
    fn portage_test_suite_vector_plus_default_rescues_an_undeclared_flag_only() {
        let ud = use_deps("dev-libs/A[foo,bar(+)]");
        // Package("=dev-libs/A-1[-foo]"): bar undeclared -> (+) rescues
        // it, but foo is declared and disabled -> still rejected.
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-1[-foo]"),
            &enabled_of("=dev-libs/A-1[-foo]")
        ));
        // Package("=dev-libs/A-2[foo]"): foo declared+enabled, bar
        // undeclared but (+)-rescued -> accepted.
        assert!(use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-2[foo]"),
            &enabled_of("=dev-libs/A-2[foo]")
        ));
    }

    #[test]
    fn portage_test_suite_vector_minus_default_on_a_required_enabled_flag_is_a_contradiction() {
        // "bar(-)" on a required-enabled flag defaults undeclared "bar" to disabled: a contradiction.
        let ud = use_deps("dev-libs/A[foo,bar(-)]");
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-1[-foo]"),
            &enabled_of("=dev-libs/A-1[-foo]")
        ));
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-2[foo]"),
            &enabled_of("=dev-libs/A-2[foo]")
        ));
    }

    #[test]
    fn portage_test_suite_vector_minus_bar_default_combines_with_a_plain_required_flag() {
        let ud = use_deps("dev-libs/A[foo,-bar(-)]");
        // bar is declared here, so bar(-) is moot; foo is declared but disabled -> rejected.
        assert!(!use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-1[-foo,bar]"),
            &enabled_of("=dev-libs/A-1[-foo,bar]")
        ));
        // Package("=dev-libs/A-2[foo]"): foo declared+enabled; bar
        // undeclared, defaults disabled via (-) -> satisfies "-bar(-)".
        assert!(use_deps_satisfied(
            &ud,
            &iuse_of("=dev-libs/A-2[foo]"),
            &enabled_of("=dev-libs/A-2[foo]")
        ));
    }

    #[test]
    fn multiple_flags_all_must_be_satisfied() {
        let ud = use_deps("dev-libs/foo[bar,-baz]");
        let iuse = HashSet::from(["bar".to_string(), "baz".to_string()]);
        assert!(use_deps_satisfied(
            &ud,
            &iuse,
            &HashSet::from(["bar".to_string()])
        ));
        // baz still enabled -- violates the "-baz" requirement.
        assert!(!use_deps_satisfied(
            &ud,
            &iuse,
            &HashSet::from(["bar".to_string(), "baz".to_string()])
        ));
    }

    #[test]
    fn atom_intersects_matches_identical_atoms() {
        let a = parse_atom("dev-libs/foo").unwrap();
        let b = parse_atom("dev-libs/foo").unwrap();
        assert!(atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_rejects_a_different_package() {
        let a = parse_atom("dev-libs/foo").unwrap();
        let b = parse_atom("dev-libs/bar").unwrap();
        assert!(!atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_rejects_a_different_version_under_the_same_operator() {
        let a = parse_atom("=dev-libs/foo-1.0").unwrap();
        let b = parse_atom("=dev-libs/foo-2.0").unwrap();
        assert!(!atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_rejects_a_different_operator_even_when_the_version_would_satisfy_it() {
        // Portage's Atom.intersects is deliberately narrow: a differing operator never intersects, even when the ranges overlap.
        let a = parse_atom(">=dev-libs/foo-1.0").unwrap();
        let b = parse_atom("=dev-libs/foo-1.0").unwrap();
        assert!(!atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_allows_a_slot_on_only_one_side() {
        let a = parse_atom("dev-libs/foo").unwrap();
        let b = parse_atom("dev-libs/foo:1").unwrap();
        assert!(atom_intersects(&a, &b));
        assert!(atom_intersects(&b, &a));
    }

    #[test]
    fn atom_intersects_rejects_conflicting_slots() {
        let a = parse_atom("dev-libs/foo:1").unwrap();
        let b = parse_atom("dev-libs/foo:2").unwrap();
        assert!(!atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_matches_identical_slots() {
        let a = parse_atom("dev-libs/foo:1").unwrap();
        let b = parse_atom("dev-libs/foo:1").unwrap();
        assert!(atom_intersects(&a, &b));
    }

    #[test]
    fn atom_intersects_rejects_different_use_deps() {
        let a = parse_atom("dev-libs/foo[bar]").unwrap();
        let b = parse_atom("dev-libs/foo[-bar]").unwrap();
        assert!(!atom_intersects(&a, &b));
    }
}

#[cfg(test)]
mod use_mismatch_tests {
    use super::*;

    fn mismatch(atom_str: &str, use_: &[&str], iuse: &[&str]) -> (Vec<String>, Vec<String>) {
        let ud = parse_atom(atom_str)
            .expect("atom must parse")
            .use_deps
            .expect("atom must carry use deps");
        let other_use: HashSet<String> = use_.iter().map(|s| s.to_string()).collect();
        let other_iuse: HashSet<String> = iuse.iter().map(|s| s.to_string()).collect();
        let (mut missing, mut violated) = use_mismatch_flags(&ud, &other_use, &other_iuse);
        let mut missing: Vec<String> = missing.drain().collect();
        let mut violated: Vec<String> = violated.drain().collect();
        missing.sort();
        violated.sort();
        (missing, violated)
    }

    #[test]
    fn missing_iuse_is_unconditional_and_skips_violation_checks() {
        // Missing IUSE is unconditional and skips the violation checks.
        assert_eq!(
            mismatch("dev-libs/foo[x]", &[], &[]),
            (vec!["x".to_string()], vec![])
        );
        // ...and a would-be `[-z]` violation on the side is skipped too.
        assert_eq!(
            mismatch("dev-libs/foo[x,-z]", &["z"], &["z"]),
            (vec!["x".to_string()], vec![])
        );
    }

    #[test]
    fn enabled_flag_off_other_is_a_violation() {
        assert_eq!(
            mismatch("dev-libs/foo[x]", &[], &["x"]),
            (vec![], vec!["x".to_string()])
        );
        assert_eq!(
            mismatch("dev-libs/foo[x]", &["x"], &["x"]),
            (vec![], vec![])
        );
    }

    #[test]
    fn disabled_flag_on_other_is_a_violation() {
        assert_eq!(
            mismatch("dev-libs/foo[-x]", &["x"], &["x"]),
            (vec![], vec!["x".to_string()])
        );
        assert_eq!(mismatch("dev-libs/foo[-x]", &[], &["x"]), (vec![], vec![]));
        // `[-x]` with x not in IUSE: required, so missing (unconditional).
        assert_eq!(
            mismatch("dev-libs/foo[-x]", &[], &[]),
            (vec!["x".to_string()], vec![])
        );
    }

    #[test]
    fn conditional_forms_never_yield_keys() {
        // Conditional hits are dropped, as in Portage.
        for atom in [
            "dev-libs/foo[x?]",
            "dev-libs/foo[!x?]",
            "dev-libs/foo[x=]",
            "dev-libs/foo[!x=]",
        ] {
            assert_eq!(mismatch(atom, &[], &["x"]), (vec![], vec![]), "{atom}");
            assert_eq!(mismatch(atom, &["x"], &["x"]), (vec![], vec![]), "{atom}");
        }
    }

    #[test]
    fn defaults_rescue_undeclared_flags_for_matching_but_not_for_keys() {
        // The (-) default makes an undeclared, off `x` a violation.
        assert_eq!(
            mismatch("dev-libs/foo[x(-)]", &[], &[]),
            (vec![], vec!["x".to_string()])
        );
        // `[x(+)]` defaults it to enabled instead: nothing violated.
        assert_eq!(mismatch("dev-libs/foo[x(+)]", &[], &[]), (vec![], vec![]));
        // `[-x(+)]` with x undeclared: the (+) default stands in for
        // "as if enabled", contradicting the must-be-disabled demand.
        assert_eq!(
            mismatch("dev-libs/foo[-x(+)]", &[], &[]),
            (vec![], vec!["x".to_string()])
        );
    }
}

#[cfg(test)]
mod use_deps_violated_tests {
    use super::*;

    fn violated(atom_str: &str, parent: &[&str], child: &[&str], iuse: &[&str]) -> bool {
        let ud = parse_atom(atom_str)
            .expect("atom must parse")
            .use_deps
            .expect("atom must carry use deps");
        let parent_use: HashSet<String> = parent.iter().map(|s| s.to_string()).collect();
        let child_use: HashSet<String> = child.iter().map(|s| s.to_string()).collect();
        let child_iuse: HashSet<String> = iuse.iter().map(|s| s.to_string()).collect();
        use_deps_violated(&ud, &parent_use, &child_use, &child_iuse)
    }

    #[test]
    fn unconditional_forms_follow_the_child() {
        assert!(violated("dev-libs/foo[x]", &[], &[], &["x"]));
        assert!(!violated("dev-libs/foo[x]", &[], &["x"], &["x"]));
        assert!(violated("dev-libs/foo[-x]", &[], &["x"], &["x"]));
        assert!(!violated("dev-libs/foo[-x]", &[], &[], &["x"]));
    }

    #[test]
    fn parent_disabled_child_enabled_breaks_not_parent_disabled() {
        // Installed consumer built with -icu (`[-icu]` raw, `[!icu?]` live) vs the icu-flipped rebuild.
        assert!(violated("dev-libs/foo[!x?]", &[], &["x"], &["x"]));
        assert!(!violated("dev-libs/foo[!x?]", &[], &[], &["x"]));
        assert!(!violated("dev-libs/foo[!x?]", &["x"], &["x"], &["x"]));
        assert!(violated("dev-libs/foo[-x]", &[], &["x"], &["x"]));
    }

    #[test]
    fn parent_enabled_forms_compare_against_the_parent() {
        assert!(violated("dev-libs/foo[x?]", &["x"], &[], &["x"]));
        assert!(!violated("dev-libs/foo[x?]", &[], &[], &["x"]));
        assert!(!violated("dev-libs/foo[x?]", &["x"], &["x"], &["x"]));
        assert!(violated("dev-libs/foo[x=]", &["x"], &[], &["x"]));
        assert!(violated("dev-libs/foo[x=]", &[], &["x"], &["x"]));
        assert!(!violated("dev-libs/foo[x=]", &["x"], &["x"], &["x"]));
        assert!(violated("dev-libs/foo[!x=]", &["x"], &["x"], &["x"]));
        assert!(violated("dev-libs/foo[!x=]", &[], &[], &["x"]));
        assert!(!violated("dev-libs/foo[!x=]", &["x"], &[], &["x"]));
    }

    #[test]
    fn undeclared_flag_without_default_is_always_violated() {
        assert!(violated("dev-libs/foo[x]", &[], &[], &[]));
        assert!(violated("dev-libs/foo[!x?]", &[], &[], &[]));
    }
}

#[cfg(test)]
mod use_dep_conditional_evaluation_tests {
    use super::*;

    #[test]
    fn if_parent_enabled_becomes_enabled_when_parent_has_the_flag() {
        // "x?": parent x -> x.
        let ud = parse_atom("dev-libs/foo[bar?]").unwrap().use_deps.unwrap();
        let parent_use = HashSet::from(["bar".to_string()]);
        let evaluated = evaluate_use_dep_conditionals(&ud, &parent_use);
        assert_eq!(
            evaluated,
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Enabled,
                default: None,
            }]
        );
    }

    #[test]
    fn if_parent_enabled_is_dropped_when_parent_lacks_the_flag() {
        // "x?": parent -x -> dropped, not "-x".
        let ud = parse_atom("dev-libs/foo[bar?]").unwrap().use_deps.unwrap();
        let evaluated = evaluate_use_dep_conditionals(&ud, &HashSet::new());
        assert!(evaluated.is_empty());
    }

    #[test]
    fn if_parent_disabled_becomes_disabled_when_parent_lacks_the_flag() {
        // "!x?": parent state -x -> result -x.
        let ud = parse_atom("dev-libs/foo[!bar?]").unwrap().use_deps.unwrap();
        let evaluated = evaluate_use_dep_conditionals(&ud, &HashSet::new());
        assert_eq!(
            evaluated,
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Disabled,
                default: None,
            }]
        );
    }

    #[test]
    fn if_parent_disabled_is_dropped_when_parent_has_the_flag() {
        let ud = parse_atom("dev-libs/foo[!bar?]").unwrap().use_deps.unwrap();
        let parent_use = HashSet::from(["bar".to_string()]);
        let evaluated = evaluate_use_dep_conditionals(&ud, &parent_use);
        assert!(evaluated.is_empty());
    }

    #[test]
    fn equal_parent_mirrors_the_parents_own_state_exactly() {
        // "x=": mirrors the parent; always constrains.
        let ud = parse_atom("dev-libs/foo[bar=]").unwrap().use_deps.unwrap();
        assert_eq!(
            evaluate_use_dep_conditionals(&ud, &HashSet::from(["bar".to_string()])),
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Enabled,
                default: None,
            }]
        );
        assert_eq!(
            evaluate_use_dep_conditionals(&ud, &HashSet::new()),
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Disabled,
                default: None,
            }]
        );
    }

    #[test]
    fn opposite_parent_inverts_the_parents_own_state() {
        // "!x=": x -> -x, -x -> x.
        let ud = parse_atom("dev-libs/foo[!bar=]").unwrap().use_deps.unwrap();
        assert_eq!(
            evaluate_use_dep_conditionals(&ud, &HashSet::from(["bar".to_string()])),
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Disabled,
                default: None,
            }]
        );
        assert_eq!(
            evaluate_use_dep_conditionals(&ud, &HashSet::new()),
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Enabled,
                default: None,
            }]
        );
    }

    #[test]
    fn defaults_survive_evaluation_unchanged() {
        let ud = parse_atom("dev-libs/foo[bar(+)=]")
            .unwrap()
            .use_deps
            .unwrap();
        let evaluated = evaluate_use_dep_conditionals(&ud, &HashSet::new());
        assert_eq!(
            evaluated,
            vec![UseDep {
                flag: "bar".to_string(),
                op: UseDepOp::Disabled,
                default: Some(UseDepDefault::Enabled),
            }]
        );
    }

    #[test]
    fn unconditional_forms_pass_through_untouched() {
        let ud = parse_atom("dev-libs/foo[bar,-baz]")
            .unwrap()
            .use_deps
            .unwrap();
        assert_eq!(evaluate_use_dep_conditionals(&ud, &HashSet::new()), ud);
    }

    #[test]
    fn evaluate_atom_conditionals_rewrites_the_bracket_in_place() {
        let parent_use = HashSet::from(["bar".to_string()]);
        assert_eq!(
            evaluate_atom_conditionals("dev-libs/foo[bar=]", &parent_use).as_deref(),
            Some("dev-libs/foo[bar]")
        );
        assert_eq!(
            evaluate_atom_conditionals("dev-libs/foo[bar=]", &HashSet::new()).as_deref(),
            Some("dev-libs/foo[-bar]")
        );
    }

    #[test]
    fn evaluate_atom_conditionals_drops_the_whole_bracket_once_empty() {
        // A lone "x?" with a parent lacking the flag leaves no "[...]" at all, not "[]".
        assert_eq!(
            evaluate_atom_conditionals("dev-libs/foo[bar?]", &HashSet::new()).as_deref(),
            Some("dev-libs/foo")
        );
    }

    #[test]
    fn evaluate_atom_conditionals_preserves_slot_and_repo() {
        // This crate's grammar puts "::repo" before the use-deps bracket (Portage orders them the other way).
        let parent_use = HashSet::from(["bar".to_string()]);
        assert_eq!(
            evaluate_atom_conditionals("dev-libs/foo:0::testrepo[bar=]", &parent_use).as_deref(),
            Some("dev-libs/foo:0::testrepo[bar]")
        );
    }

    #[test]
    fn evaluate_atom_conditionals_leaves_a_plain_use_dep_atom_unchanged() {
        // No conditional ops: must be the literal same string.
        let atom_str = "dev-libs/foo[bar,-baz(+)]";
        assert_eq!(
            evaluate_atom_conditionals(atom_str, &HashSet::new()).as_deref(),
            Some(atom_str)
        );
    }

    #[test]
    fn evaluate_atom_conditionals_leaves_a_no_use_dep_atom_unchanged() {
        let atom_str = "dev-libs/foo";
        assert_eq!(
            evaluate_atom_conditionals(atom_str, &HashSet::new()).as_deref(),
            Some(atom_str)
        );
    }

    #[test]
    fn evaluate_atom_conditionals_returns_none_for_an_unparseable_atom() {
        assert_eq!(
            evaluate_atom_conditionals("not a valid atom", &HashSet::new()),
            None
        );
    }
}

#[cfg(test)]
mod extract_affecting_use_tests {
    use super::*;

    /// The 23 passing cases from Portage's
    /// `lib/portage/tests/dep/test_extract_affecting_use.py`, verbatim.
    #[test]
    fn matches_portages_test_corpus() {
        let cases: &[(&str, &str, &[&str])] = &[
            ("a? ( A ) !b? ( B ) !c? ( C ) d? ( D )", "A", &["a"]),
            ("a? ( A ) !b? ( B ) !c? ( C ) d? ( D )", "B", &["b"]),
            ("a? ( A ) !b? ( B ) !c? ( C ) d? ( D )", "C", &["c"]),
            ("a? ( A ) !b? ( B ) !c? ( C ) d? ( D )", "D", &["d"]),
            ("a? ( b? ( AB ) )", "AB", &["a", "b"]),
            ("a? ( b? ( c? ( ABC ) ) )", "ABC", &["a", "b", "c"]),
            ("a? ( A b? ( c? ( ABC ) AB ) )", "A", &["a"]),
            ("a? ( A b? ( c? ( ABC ) AB ) )", "AB", &["a", "b"]),
            ("a? ( A b? ( c? ( ABC ) AB ) )", "ABC", &["a", "b", "c"]),
            ("a? ( A b? ( c? ( ABC ) AB ) ) X", "X", &[]),
            ("X a? ( A b? ( c? ( ABC ) AB ) )", "X", &[]),
            ("ab? ( || ( A B ) )", "A", &["ab"]),
            ("!ab? ( || ( A B ) )", "B", &["ab"]),
            ("ab? ( || ( A || ( b? ( || ( B C ) ) ) ) )", "A", &["ab"]),
            (
                "ab? ( || ( A || ( b? ( || ( B C ) ) ) ) )",
                "B",
                &["ab", "b"],
            ),
            (
                "ab? ( || ( A || ( b? ( || ( B C ) ) ) ) )",
                "C",
                &["ab", "b"],
            ),
            (
                "( ab? ( || ( ( A ) || ( b? ( ( ( || ( B ( C ) ) ) ) ) ) ) ) )",
                "A",
                &["ab"],
            ),
            (
                "( ab? ( || ( ( A ) || ( b? ( ( ( || ( B ( C ) ) ) ) ) ) ) ) )",
                "B",
                &["ab", "b"],
            ),
            (
                "( ab? ( || ( ( A ) || ( b? ( ( ( || ( B ( C ) ) ) ) ) ) ) ) )",
                "C",
                &["ab", "b"],
            ),
            ("a? ( A )", "B", &[]),
            ("a? ( || ( A B ) )", "B", &["a"]),
            (
                "a? ( >=dev-lang/php-5.2[pcre(+)] )",
                ">=dev-lang/php-5.2[pcre(+)]",
                &["a"],
            ),
        ];
        for (dep, atom, want) in cases {
            let got = extract_affecting_use(dep, atom)
                .unwrap_or_else(|| panic!("({dep:?}, {atom:?}) returned None, want {want:?}"));
            let want: HashSet<String> = want.iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want, "extract_affecting_use({dep:?}, {atom:?})");
        }
    }

    /// The 15 malformed cases from the same file's `test_cases_xfail`
    /// (Portage raises `InvalidDependString`; portuale returns `None`).
    #[test]
    fn malformed_syntax_returns_none() {
        let cases: &[(&str, &str)] = &[
            ("? ( A )", "A"),
            ("!? ( A )", "A"),
            ("( A", "A"),
            ("A )", "A"),
            ("||( A B )", "A"),
            ("|| (A B )", "A"),
            ("|| ( A B)", "A"),
            ("|| ( A B", "A"),
            ("|| A B )", "A"),
            ("|| A B", "A"),
            ("|| ( A B ) )", "A"),
            ("|| || B C", "A"),
            ("|| ( A B || )", "A"),
            ("a? A", "A"),
            ("( || ( || || ( A ) foo? ( B ) ) )", "A"),
        ];
        for (dep, atom) in cases {
            assert_eq!(
                extract_affecting_use(dep, atom),
                None,
                "extract_affecting_use({dep:?}, {atom:?}) should be None"
            );
        }
    }

    #[test]
    fn docstring_example() {
        let got = extract_affecting_use(
            "sasl? ( dev-libs/cyrus-sasl ) !minimal? ( cxx? ( dev-libs/cyrus-sasl ) )",
            "dev-libs/cyrus-sasl",
        )
        .unwrap();
        assert_eq!(
            got,
            HashSet::from(["cxx".to_string(), "minimal".to_string(), "sasl".to_string()])
        );
    }
}
