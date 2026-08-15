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

// Memoizes parse results: parsing is a pure function of the string, and the
// resolver re-parses the same few atoms/candidates millions of times.
// Thread-local to stay lock-free; never invalidated.
thread_local! {
    static ATOM_CACHE: RefCell<HashMap<String, Option<Atom>>> = RefCell::new(HashMap::new());
    static CANDIDATE_CACHE: RefCell<HashMap<String, Option<Candidate>>> =
        RefCell::new(HashMap::new());
}

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
    let caps = atom_regex().captures(s)?;

    let blocker = match caps.name("blocker").map(|m| m.as_str()) {
        None => Blocker::None,
        Some("!") => Blocker::Weak,
        Some("!!") => Blocker::Strong,
        Some(_) => unreachable!(),
    };

    let (operator, category, package, version, revision) = if let Some(op) = caps.name("op") {
        let operator = match caps.name("glob") {
            // PMS 8.3.1: `*` is illegal with any operator but `=`.
            Some(_) if op.as_str() != "=" => return None,
            Some(_) => Operator::EqGlob,
            None => Operator::from_str(op.as_str()),
        };
        (
            operator,
            caps.name("vcat").unwrap().as_str().to_string(),
            caps.name("vpkg").unwrap().as_str().to_string(),
            Some(caps.name("ver").unwrap().as_str().to_string()),
            caps.name("rev").map(|m| m.as_str().to_string()),
        )
    } else {
        // Ambiguous per PMS (`foo-bar-2`).
        // Portage: lib/portage/dep/__init__.py, Atom.__init__
        if caps.name("ambiguous").is_some() {
            return None;
        }
        (
            Operator::None,
            caps.name("cat").unwrap().as_str().to_string(),
            caps.name("pkg").unwrap().as_str().to_string(),
            None,
            None,
        )
    };

    // A bare trailing ":" matches the regex but is invalid.
    let (slot, sub_slot, slot_operator) = (match caps.name("slotpart") {
        None => Some((None, None, None)),
        Some(m) if m.as_str().is_empty() => None,
        Some(_) => {
            let slot = caps.name("slot").map(|m| m.as_str().to_string());
            let sub_slot = caps.name("subslot").map(|m| m.as_str().to_string());
            let slot_operator = match caps.name("slotop").map(|m| m.as_str()) {
                None => None,
                // An explicit slot combined with "*" is invalid.
                Some("*") if slot.is_some() => return None,
                Some("*") => Some(SlotOperator::Star),
                Some("=") => Some(SlotOperator::Equals),
                Some(_) => unreachable!(),
            };
            Some((slot, sub_slot, slot_operator))
        }
    })?;

    let use_deps = match caps.name("usedeps") {
        None => None,
        Some(m) => Some(parse_use_deps(m.as_str())?),
    };

    let repo = caps.name("repo").map(|m| m.as_str().to_string());

    Some(Atom {
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
    })
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
            r"^(?P<cat>{CAT})/(?P<pkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?(?:::(?P<repo>{REPO}))?$"
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
        Operator::Eq => vercmp(&candidate.full_version(), &atom.full_version().unwrap()) == Some(0),
        // `=*` is a literal prefix match, not vercmp, but only at a component
        // boundary: "1*" must not match "10".
        // Portage: lib/portage/dep/__init__.py, match_from_list
        Operator::EqGlob => {
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
            Some(have) => have == want,
        },
    }
}

/// Returns the candidates (input order) matching `atom_str`; `None` if the
/// atom doesn't parse. Unparseable candidates are skipped.
/// Portage: lib/portage/dep/__init__.py, match_from_list
pub fn match_from_list<'a>(atom_str: &str, candidates: &[&'a str]) -> Option<Vec<&'a str>> {
    let atom = parse_atom(atom_str)?;
    Some(
        candidates
            .iter()
            .copied()
            .filter(|c| {
                let Some(candidate) = parse_candidate(c) else {
                    return false;
                };
                candidate.category == atom.category
                    && candidate.package == atom.package
                    && matches_version(&atom, &candidate)
                    && matches_slot(&atom, &candidate)
                    && matches_repo(&atom, &candidate)
            })
            .collect(),
    )
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
