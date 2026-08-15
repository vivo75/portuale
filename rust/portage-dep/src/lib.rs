// Rust port of a deliberately narrowed subset of `portage.dep.Atom` and
// `portage.dep.match_from_list` (lib/portage/dep/__init__.py) -- the
// "atom matching" pilot slice from PORTING/PROMPT.md's depgraph/config
// resolution follow-up work.
//
// KNOWN, DOCUMENTED SCOPE CUT vs. the real grammar (see
// PORTING/rust/atom-harness/README.md for the rationale): `Atom`/
// `parse_atom`/`match_from_list` support no USE deps (`foo[bar]`), no
// extended/wildcard atoms (`*/foo-1`), no build-ids (`foo-1.0@2`), no repo
// constraint (`::gentoo`), no slot operators (`:=`, `:*`), no `=*` glob
// version operator, and no EAPI parametrization (the real grammar changes
// shape per-EAPI). The Python harness (PORTING/python/atom_harness.py)
// explicitly rejects atoms using any of these features as INVALID, so
// both sides agree on the same input language rather than Rust silently
// accepting a narrower one. (A separate, bounded wildcard-atom API is
// further down in this file, for package.mask/.unmask/.accept_keywords
// matching only -- it doesn't change any of the above.)
//
// Candidates for match_from_list are plain strings shaped like
// `category/package-version[-rN][:slot[/subslot]]` -- not full Package
// objects (no USE/IUSE/repo metadata), since this pilot has no
// package-db/depgraph model yet. This mirrors how the real
// match_from_list already supports plain strings (via dep_getslot's
// ":slot" suffix convention) as a fallback when candidates aren't Package
// objects.
//
// One easy-to-miss PMS rule that IS ported: a bare (no-operator) atom
// whose package name is followed by something that looks like a version
// (e.g. "foo-bar-2", which could be read as package "foo-bar" version "2")
// is rejected as ambiguous, not silently accepted with the longest
// possible package name. See the `ambiguous` capture group below and
// Atom.__init__'s corresponding check on the "simple" branch in
// lib/portage/dep/__init__.py.

use portage_versions::vercmp;
use regex::Regex;
use std::sync::OnceLock;

const CAT: &str = r"[A-Za-z0-9_][A-Za-z0-9+_.-]*";
// Non-greedy, like Python's `_pkg` in lib/portage/versions.py: lets the
// following `-<version>` anchor decide where the package name ends,
// e.g. "utf8-scanner-1.0" splits as pkg="utf8-scanner", version="1.0".
const PKG: &str = r"[A-Za-z0-9_][A-Za-z0-9+_-]*?";
const VER: &str = r"\d+(?:\.\d+)*[a-z]?(?:_(?:pre|p|beta|alpha|rc)\d*)*";
const SLOT: &str = r"[A-Za-z0-9][A-Za-z0-9+_.-]*";

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
}

impl Atom {
    /// The version including its revision, e.g. "1.2.3-r1", for feeding to
    /// `vercmp` -- mirrors Atom.cpv's version part in the Python original.
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
        Regex::new(&format!(
            r"^(?P<blocker>!!|!)?(?:(?P<op>=|>=|>|<=|<|~)(?P<vcat>{CAT})/(?P<vpkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?|(?P<cat>{CAT})/(?P<pkg>{PKG})(?P<ambiguous>-{VER}(?:-r\d+)?)?)(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?$"
        ))
        .unwrap()
    })
}

pub fn parse_atom(s: &str) -> Option<Atom> {
    let caps = atom_regex().captures(s)?;

    let blocker = match caps.name("blocker").map(|m| m.as_str()) {
        None => Blocker::None,
        Some("!") => Blocker::Weak,
        Some("!!") => Blocker::Strong,
        Some(_) => unreachable!(),
    };

    let (operator, category, package, version, revision) = if let Some(op) = caps.name("op") {
        (
            Operator::from_str(op.as_str()),
            caps.name("vcat").unwrap().as_str().to_string(),
            caps.name("vpkg").unwrap().as_str().to_string(),
            Some(caps.name("ver").unwrap().as_str().to_string()),
            caps.name("rev").map(|m| m.as_str().to_string()),
        )
    } else {
        // A bare (no-operator) atom whose package name is followed by
        // something that looks like a version (e.g. "foo-bar-2") is
        // ambiguous under PMS and must be rejected, not silently absorbed
        // into a longer package name -- mirrors Atom.__init__'s check on
        // the "simple" branch's trailing optional "-<version>" group in
        // lib/portage/dep/__init__.py.
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

    let slot = caps.name("slot").map(|m| m.as_str().to_string());
    let sub_slot = caps.name("subslot").map(|m| m.as_str().to_string());

    Some(Atom {
        blocker,
        operator,
        category,
        package,
        version,
        revision,
        slot,
        sub_slot,
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
            r"^(?P<cat>{CAT})/(?P<pkg>{PKG})-(?P<ver>{VER})(?:-r(?P<rev>\d+))?(?::(?P<slot>{SLOT})(?:/(?P<subslot>{SLOT}))?)?$"
        ))
        .unwrap()
    })
}

pub fn parse_candidate(s: &str) -> Option<Candidate> {
    let caps = candidate_regex().captures(s)?;
    Some(Candidate {
        category: caps.name("cat").unwrap().as_str().to_string(),
        package: caps.name("pkg").unwrap().as_str().to_string(),
        version: caps.name("ver").unwrap().as_str().to_string(),
        revision: caps.name("rev").map(|m| m.as_str().to_string()),
        slot: caps.name("slot").map(|m| m.as_str().to_string()),
        sub_slot: caps.name("subslot").map(|m| m.as_str().to_string()),
    })
}

/// Mirrors match_from_list's per-candidate filtering, for a single
/// candidate that has already matched category/package.
fn matches_version(atom: &Atom, candidate: &Candidate) -> bool {
    match atom.operator {
        Operator::None => true,
        Operator::Eq => vercmp(&candidate.full_version(), &atom.full_version().unwrap()) == Some(0),
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

/// Mirrors _match_slot: exact slot match required; sub_slot only checked
/// if the atom specifies one. A candidate with no slot info at all always
/// passes (matches match_from_list's behavior for plain-string candidates
/// it can't determine a slot for -- see the module doc comment).
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

/// Mirrors `match_from_list`: given an atom string and a list of candidate
/// strings, returns the subset (in input order) that match. `None` means
/// the atom itself failed to parse under the v1 grammar. Unparseable
/// candidate strings are silently skipped (a documented simplification --
/// see the module doc comment).
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
            })
            .collect(),
    )
}

// --- Bounded wildcard atoms (package.mask/.unmask/.accept_keywords) ---
//
// A separate, additional API from everything above: `Atom`/`parse_atom`/
// `match_from_list` are unchanged, so atom-harness's existing v1 grammar
// contract (which explicitly rejects wildcard atoms as INVALID) is not
// affected by any of this. This exists for package.mask/.unmask/
// .accept_keywords matching (see portage-repo), where real files lean
// heavily on wildcard atoms like "*/*" and "dev-qt/*" in practice.
//
// Deliberately bounded, not the full PMS extended-atom-syntax grab-bag:
// only "*/*", "category/*", and "*/package" -- a literal "*" standing in
// for an entire category or package name, not a partial-string glob like
// "cat/pkg-*". No version operators, no slots on a wildcard atom (real
// PMS extended atoms don't carry them either).

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

/// Parses `s` as a wildcard atom. Returns `None` if `s` doesn't have
/// exactly one `/`, either side fails to validate as a category/package
/// token (or `*`), or neither side is actually a `*` (a plain atom with
/// no wildcard at all isn't this grammar's job -- try `parse_atom` +
/// `match_from_list` first, which covers versioned/slotted atoms this
/// can't).
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
