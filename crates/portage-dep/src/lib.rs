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
pub fn use_deps_satisfied(
    use_deps: &[UseDep],
    iuse: &HashSet<String>,
    enabled: &HashSet<String>,
) -> bool {
    if use_deps
        .iter()
        .any(|ud| ud.default.is_none() && !iuse.contains(&ud.flag))
    {
        return false;
    }

    let missing_enabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.default == Some(UseDepDefault::Enabled) && !iuse.contains(&ud.flag))
        .map(|ud| ud.flag.as_str())
        .collect();
    let missing_disabled: HashSet<&str> = use_deps
        .iter()
        .filter(|ud| ud.default == Some(UseDepDefault::Disabled) && !iuse.contains(&ud.flag))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardAtom {
    pub category: Option<String>,
    pub package: Option<String>,
}

/// Parses `*/*`, `cat/*` or `*/pkg`; `None` for anything else, including a
/// plain atom with no wildcard (use `parse_atom`).
pub fn parse_wildcard_atom(s: &str) -> Option<WildcardAtom> {
    let (cat, pkg) = s.split_once('/')?;
    if pkg.contains('/') {
        return None;
    }

    let category = if cat == "*" {
        None
    } else if cat_full_re().is_match(cat) {
        Some(cat.to_string())
    } else {
        return None;
    };
    let package = if pkg == "*" {
        None
    } else if pkg_full_re().is_match(pkg) {
        Some(pkg.to_string())
    } else {
        return None;
    };

    if category.is_some() && package.is_some() {
        return None;
    }
    Some(WildcardAtom { category, package })
}

pub fn wildcard_atom_matches(atom: &WildcardAtom, category: &str, package: &str) -> bool {
    atom.category.as_deref().is_none_or(|c| c == category)
        && atom.package.as_deref().is_none_or(|p| p == package)
}

#[cfg(test)]
mod wildcard_tests {
    use super::*;

    #[test]
    fn any_any_matches_everything() {
        let w = parse_wildcard_atom("*/*").unwrap();
        assert!(wildcard_atom_matches(&w, "dev-libs", "foo"));
        assert!(wildcard_atom_matches(&w, "app-misc", "bar"));
    }

    #[test]
    fn category_wildcard_matches_only_that_category() {
        let w = parse_wildcard_atom("dev-qt/*").unwrap();
        assert!(wildcard_atom_matches(&w, "dev-qt", "qtcore"));
        assert!(!wildcard_atom_matches(&w, "dev-libs", "qtcore"));
    }

    #[test]
    fn package_wildcard_matches_only_that_package_name() {
        let w = parse_wildcard_atom("*/foo").unwrap();
        assert!(wildcard_atom_matches(&w, "dev-libs", "foo"));
        assert!(wildcard_atom_matches(&w, "app-misc", "foo"));
        assert!(!wildcard_atom_matches(&w, "dev-libs", "bar"));
    }

    #[test]
    fn plain_atom_with_no_wildcard_is_rejected() {
        // Not this grammar's job -- callers should try parse_atom first.
        assert_eq!(parse_wildcard_atom("dev-libs/foo"), None);
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert_eq!(parse_wildcard_atom("no-slash-at-all"), None);
        assert_eq!(parse_wildcard_atom("dev-libs/"), None);
        assert_eq!(parse_wildcard_atom("/foo"), None);
        assert_eq!(parse_wildcard_atom("dev-libs/foo/bar"), None);
    }
}

fn affecting_useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Modern-EAPI useflag charset; EAPI is not parametrized.
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
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
}
