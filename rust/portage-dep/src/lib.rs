// Rust port of a deliberately narrowed subset of `portage.dep.Atom` and
// `portage.dep.match_from_list` (lib/portage/dep/__init__.py) -- the
// "atom matching" pilot slice from PORTING/PROMPT.md's depgraph/config
// resolution follow-up work.
//
// KNOWN, DOCUMENTED SCOPE CUT vs. the real grammar (PMS chapter 8):
// `Atom`/`parse_atom`/`match_from_list` support no extended/wildcard
// atoms (`*/foo-1`), no build-ids (`foo-1.0@2`), and no EAPI
// parametrization (the real grammar changes shape per-EAPI --
// slot-operator support itself is EAPI 5+, but since nothing here is
// EAPI-parametrized in the first place, it's just always recognized, the
// same way every other EAPI-gated feature already ported is). The Python
// harness (PORTING/python/atom_harness.py) explicitly rejects atoms
// using any of the still-excluded features as INVALID, so both sides
// agree on the same input language rather than Rust silently accepting a
// narrower one. (A separate, bounded wildcard-atom API is further down
// in this file, for package.mask/.unmask/.accept_keywords matching only
// -- it doesn't change any of the above.)
//
// Slot operators (`:=`, `:*`, `:slot=` -- PMS 8.3.3) ARE supported: see
// `SlotOperator`, `atom_regex`'s doc comment on the two-stage parse this
// needed (mirroring real portage's own `_get_atom_re`/`_get_slot_dep_re`
// split), and `matches_slot`'s doc comment on why *matching* needed zero
// changes at all -- real `_match_slot` ignores `slot_operator` entirely,
// consulting only `Atom.slot`/`.sub_slot`, both of which this crate
// already modeled correctly before slot operators existed. This closed a
// real, previously-silent bug in `portage-repo`'s dependency recursion:
// any DEPEND/RDEPEND token using a slot operator (extremely common in
// real ebuilds, e.g. `dev-libs/foo:0=` for ABI-rebuild tracking) failed
// to parse under the old grammar and was silently dropped from the graph
// entirely -- no entry, no `NoVisibleCandidate`, no warning -- since
// `resolve_pretend_graph`'s BFS loop treats a parse failure as "not a
// dependency at all" (`let Some(atom) = parse_atom(..) else { continue };`),
// not as an unresolvable one.
//
// Candidates for match_from_list are plain strings shaped like
// `category/package-version[-rN][:slot[/subslot]]` -- not full Package
// objects (no USE/IUSE/repo metadata), since this pilot has no
// package-db/depgraph model yet. This mirrors how the real
// match_from_list already supports plain strings (via dep_getslot's
// ":slot" suffix convention) as a fallback when candidates aren't Package
// objects.
//
// USE deps (`foo[bar]`, `foo[bar?,!baz=,qux(+)]` -- PMS 8.3.4, all 7
// per-flag forms plus 4-style `(+)`/`(-)` defaults) ARE parsed -- see
// `UseDep`/`UseDepOp`/`UseDepDefault` and `parse_use_deps` -- and, as of
// `use_deps_satisfied` (see its own doc comment for the full algorithm,
// ported from real `match_from_list`'s own USE-dep post-pass), CAN now
// be enforced too, given real per-candidate IUSE/USE state -- but
// `matches_version`/`matches_slot`/`match_from_list` themselves still
// never consult `Atom::use_deps` at all, matching real `match_from_list`
// exactly: its own USE-dep filtering is skipped entirely for any
// candidate that isn't a real Package object with `.use`/`.iuse`
// attributes (see the `hasattr` check in
// `lib/portage/dep/__init__.py`'s `match_from_list`), which is exactly
// the plain-string-candidate case `match_from_list` here always sees --
// so leaving `match_from_list` itself unaware of use deps isn't a
// divergence, just where real portage's own architecture already draws
// this line. `portage-repo` calls `use_deps_satisfied` directly, as an
// extra filter after `match_from_list`'s own version/slot/repo
// filtering, once it already has each surviving candidate's own real
// IUSE/effective-USE in hand (computed for other reasons already -- see
// that crate's own doc comment). Before USE deps were parseable at all,
// an atom using one wasn't just "under-enforced" -- it was rejected as
// `INVALID` outright, which for a *dependency* atom extracted from
// DEPEND/RDEPEND meant `resolve_pretend_graph`'s BFS silently dropped it
// from the graph entirely (same class of bug the slot-operator follow-up
// found and fixed -- see that doc comment).
//
// The `=*` glob version operator (PMS 8.3.1) IS supported: see
// `Operator::EqGlob`, `atom_regex`'s doc comment on why the trailing "*"
// is captured generically rather than as a second grammar alternative,
// and `matches_version`'s own `EqGlob` arm (plus
// `normalize_leading_zeros`/`glob_compare_string`) for the boundary-aware
// prefix-match algorithm this needed -- real portage implements `=*` as
// a literal string-prefix match, not a `vercmp`-based one (its own
// comment: "Nasty special casing for leading zeros / Required as =* is a
// literal prefix match, so can't use vercmp"), with a component-boundary
// check fixing a real historical bug (560466: "1*" must not match "10").
// Grounded against the PMS's own historical note on this operator too:
// the component-wise "wildcard for any further components" semantic here
// is the *current* one -- a raw string-prefix match (e.g. "=foo-5.2*"
// matching "foo-5.22.0") was the original EAPI 0-5 behavior, retroactively
// dropped in October 2015, well before this repo's EAPI 5+ floor.
//
// `::reponame` (the repo constraint, PMS 3.1.5 "Repository names") IS
// supported now too: see `Atom::repo`/`Candidate::repo` and
// `matches_repo`'s own doc comment for the exact matching semantics
// (ported from real `match_from_list`'s own final post-pass filter --
// only ever rejects a candidate that carries a KNOWN, different repo; a
// repo-less candidate string always passes, matching real
// `dep_getrepo`'s own "unknown, not absent" semantics for a plain
// string). This pilot's own candidate strings never carried repo
// identity before this slice -- `portage-repo` now appends `::reponame`
// (using each repo's own `repos.conf` section name -- already tracked
// as `RepoConfig::name`, reused as-is rather than reading a second,
// separate `profiles/repo_name` file real portage also cross-checks
// against) to every candidate string it builds for `match_from_list`
// EXCEPT the two paths noted in that crate's own doc comment (blocker
// matching and slot-conflict re-verification), a deliberate, narrower
// scope cut than the rest of this feature's wiring.
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
use std::collections::HashSet;
use std::sync::OnceLock;

const CAT: &str = r"[A-Za-z0-9_][A-Za-z0-9+_.-]*";
// Non-greedy, like Python's `_pkg` in lib/portage/versions.py: lets the
// following `-<version>` anchor decide where the package name ends,
// e.g. "utf8-scanner-1.0" splits as pkg="utf8-scanner", version="1.0".
const PKG: &str = r"[A-Za-z0-9_][A-Za-z0-9+_-]*?";
const VER: &str = r"\d+(?:\.\d+)*[a-z]?(?:_(?:pre|p|beta|alpha|rc)\d*)*";
const SLOT: &str = r"[A-Za-z0-9][A-Za-z0-9+_.-]*";
// Identical to real portage's own `_repo_name` (lib/portage/dep/__init__.py):
// `[\w][\w-]*` -- `\w` is alnum-plus-underscore, so this is
// `[A-Za-z0-9_][A-Za-z0-9_-]*`. Matches PMS 3.1.5's "Repository names"
// prose ("may contain [A-Za-z0-9_-], must not begin with a hyphen").
const REPO: &str = r"[A-Za-z0-9_][A-Za-z0-9_-]*";
// Identical to real portage's own `_useflag_re` (lib/portage/dep/__init__.py)
// and already mirrored once in `portage-use-reduce`'s own `useflag_re` --
// duplicated here rather than added as a cross-crate dependency, since
// it's a single-line regex literal, not shared logic.
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
    /// `=*` (PMS 8.3.1): "if the version specified has an asterisk
    /// immediately following it, then only the given number of version
    /// components is used for comparison, i.e. the asterisk acts as a
    /// wildcard for any further components." A real, distinct operator
    /// value in real portage too (`Atom.operator == "=*"`), not `Eq` with
    /// a flag -- see `matches_version`'s own `EqGlob` arm for the
    /// matching algorithm this enables.
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

/// A slot-operator dependency (PMS 8.3.3) -- `:*` (`Star`, any slot
/// acceptable, no explicit slot) or `:=`/`:slot=` (`Equals`, any slot
/// acceptable if no explicit slot is given, otherwise restricted to that
/// slot -- see `Atom::slot`). Purely a rebuild-trigger signal in real
/// portage (whether a dependency needs rebuilding when the matched
/// package's sub-slot changes); irrelevant to whether an atom *matches* a
/// candidate, which is exactly why `matches_slot` needs zero changes to
/// support this -- see its doc comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotOperator {
    Star,
    Equals,
}

/// A 2-style or 4-style USE dependency's operator (PMS 8.3.4) -- which of
/// the 6 real `prefix`+`suffix` combinations (real portage's own
/// `_usedep_re` groups) a single flag spec uses. `EqualParent`/
/// `OppositeParent` and `IfParentEnabled`/`IfParentDisabled` are both
/// conditional on the *atom-owning* package's own USE state, not just the
/// candidate's -- `use_deps_satisfied` (see its own doc comment) still
/// requires their own flag to be a real, declared IUSE flag on the
/// candidate (same as any other use-dep flag), but, matching real
/// `match_from_list` exactly, imposes no enabled/disabled constraint
/// from them at all; only `Enabled`/`Disabled` (the two unconditional
/// forms) actually constrain a candidate's own USE state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseDepOp {
    Enabled,          // "flag"
    Disabled,         // "-flag"
    IfParentEnabled,  // "flag?"
    IfParentDisabled, // "!flag?"
    EqualParent,      // "flag="
    OppositeParent,   // "!flag="
}

/// A 4-style USE dependency's default (PMS 8.3.4): what to assume when
/// the ebuild being matched against doesn't have `flag` in
/// IUSE_REFERENCEABLE at all. Parsed for fidelity/round-tripping; never
/// consulted by matching, same as the rest of `UseDep`.
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
    /// `None` for no `[...]` at all; `Some(v)` (`v` always non-empty --
    /// `foo[]` is invalid, same as real portage) for a present one, in
    /// original left-to-right order. Never consulted by matching -- see
    /// the module doc comment.
    pub use_deps: Option<Vec<UseDep>>,
    /// `::reponame` (PMS 3.1.5 "Repository names"), `None` if absent --
    /// see `matches_repo`'s own doc comment for the matching semantics.
    pub repo: Option<String>,
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
        // The whole post-":" slot expression is wrapped in its own
        // "slotpart" group (mirroring real portage's two-stage approach --
        // _get_atom_re captures the raw text after ":", then
        // _get_slot_dep_re re-parses it) so parse_atom can tell "no ':' at
        // all" (group absent) apart from "':' present but empty" (group
        // matched an empty string), which PMS says is invalid (see the
        // "if self.slot is None and self.slot_operator is None: raise"
        // check in Atom.__init__).
        // "usedeps" mirrors real portage's own permissive `\[.*\]` outer
        // capture (`_use` in lib/portage/dep/__init__.py): validated in a
        // second stage by `parse_use_deps`, same two-stage split as the
        // slot part above.
        // "glob" (a trailing "*" right after the version/revision, PMS
        // 8.3.1's "=*" operator) is captured for ANY of the 6 operators
        // here, not just "=" -- real portage's own grammar only ever
        // allows it after "=" (a separate, dedicated "star" alternative
        // in _get_atom_re, distinct from its general "op" alternative),
        // but `parse_atom` below rejects a captured "glob" paired with
        // any operator other than "=" explicitly, which is simpler than
        // duplicating the whole op+cpv sequence into a second regex
        // alternative just to exclude 5 of 6 operators from one optional
        // trailing character.
        // "repo" ("::reponame", PMS 3.1.5) sits between the slot part and
        // usedeps, matching real _get_atom_re's own ordering exactly --
        // shared by both the "op" and bare "simple" branches above it,
        // same as slotpart/usedeps already are.
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

/// Parses the raw `[...]` text (brackets included) captured by
/// `atom_regex`'s permissive `usedeps` group into a validated
/// `Vec<UseDep>`, mirroring real portage's own two-stage approach
/// (`Atom.__init__`'s use-dep loop, not a separate regex function this
/// time -- there's no dedicated `_get_usedep_re`-equivalent split out on
/// the Rust side, but the algorithm is the same): split on `,`, validate
/// each token against `use_dep_token_regex`, and validate that only the
/// 6 real `prefix`+`suffix` combinations appear (`-flag=`/`-flag?` are
/// syntactically matched by the per-token regex but not real operators --
/// verified empirically against real portage, which rejects them too).
/// Also validates that a flag's `(+)`/`(-)` default, if any, is
/// consistent across every token mentioning that flag within this same
/// atom (`foo[bar(+),bar(-)]` and `foo[bar(+),-bar]` are both invalid --
/// same empirically-verified real behavior), so the accept/reject
/// boundary matches real `Atom` exactly, even though the *values* are
/// never consulted by matching (see the module doc comment).
fn parse_use_deps(raw: &str) -> Option<Vec<UseDep>> {
    let inner = raw.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return None;
    }
    let mut deps = Vec::new();
    // "has a default at all" per flag, so a later token for the same
    // flag with a *different* has-default state (regardless of which
    // specific default) is caught too -- mirrors real Atom.__init__'s
    // three-way missing_enabled/missing_disabled/no_default bookkeeping.
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
    let caps = atom_regex().captures(s)?;

    let blocker = match caps.name("blocker").map(|m| m.as_str()) {
        None => Blocker::None,
        Some("!") => Blocker::Weak,
        Some("!!") => Blocker::Strong,
        Some(_) => unreachable!(),
    };

    let (operator, category, package, version, revision) = if let Some(op) = caps.name("op") {
        let operator = match caps.name("glob") {
            // "an asterisk used with any other operator is illegal" (PMS
            // 8.3.1) -- e.g. ">=cat/pkg-1.2*" must be rejected outright,
            // not silently truncated to ">=cat/pkg-1.2" or accepted as a
            // glob under the wrong operator.
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

    // "slotpart" present but empty means a bare trailing ":" with nothing
    // after it -- syntactically matched by the regex (both the slot and
    // the operator sub-groups are individually optional) but explicitly
    // invalid per PMS/Atom.__init__ (see atom_regex's doc comment).
    let (slot, sub_slot, slot_operator) = (match caps.name("slotpart") {
        None => Some((None, None, None)),
        Some(m) if m.as_str().is_empty() => None,
        Some(_) => {
            let slot = caps.name("slot").map(|m| m.as_str().to_string());
            let sub_slot = caps.name("subslot").map(|m| m.as_str().to_string());
            let slot_operator = match caps.name("slotop").map(|m| m.as_str()) {
                None => None,
                // An explicit slot combined with "*" is invalid -- "*"
                // means "any slot", which is meaningless alongside a
                // specific one (see Atom.__init__'s corresponding check).
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
    /// `::reponame` suffix, `None` if the string didn't have one --
    /// mirrors real `dep_getrepo`'s own convention for plain-string
    /// candidates. See `matches_repo`'s own doc comment.
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

/// Collapses a leading run of `'0'` characters in `version` the way real
/// `match_from_list`'s own `=*` branch does before ever comparing
/// anything (its own comment: "XXX: Nasty special casing for leading
/// zeros / Required as =* is a literal prefix match, so can't use
/// vercmp"). `=*` matches by literal string prefix, not `vercmp`, so
/// without this a version's own incidental leading zeros (e.g. "01" vs
/// "1", numerically identical) would make two numerically-equal versions
/// compare unequal as prefixes. Only ever applied to the plain version
/// (never the `-rN` revision suffix, matching real portage's own
/// `mycpv_cps[2]`/`xs[2]` -- the `catpkgsplit`-style "version, no
/// revision" component).
///
/// Empirically verified against real `portage.dep.match_from_list`
/// (`python3 -c` probing several leading-zero cases) before relying on
/// this port: `"0" -> "0"`, `"00" -> "0"`, `"01" -> "1"`, `"0.5" -> "0.5"`
/// (unchanged: the single leading zero is a real, meaningful digit, not
/// redundant), `"00.5" -> "0.5"` (the redundant *second* zero is
/// dropped).
fn normalize_leading_zeros(version: &str) -> String {
    let stripped = version.trim_start_matches('0');
    let starts_with_digit = stripped.starts_with(|c: char| c.is_ascii_digit());
    if starts_with_digit {
        stripped.to_string()
    } else {
        format!("0{stripped}")
    }
}

/// The exact string `=*` prefix-compares against for one side (atom or
/// candidate) of an `EqGlob` match: `version` with its own leading zeros
/// collapsed (see `normalize_leading_zeros`), plus `-r{revision}` if
/// present, unchanged -- mirrors real portage's own targeted
/// `mycpv.replace(cp + "-" + orig_version, cp + "-" + normalized, 1)`,
/// which only ever rewrites the plain-version substring, never the
/// revision.
fn glob_compare_string(version: &str, revision: &Option<String>) -> String {
    let normalized = normalize_leading_zeros(version);
    match revision {
        Some(r) => format!("{normalized}-r{r}"),
        None => normalized,
    }
}

/// Mirrors match_from_list's per-candidate filtering, for a single
/// candidate that has already matched category/package.
fn matches_version(atom: &Atom, candidate: &Candidate) -> bool {
    match atom.operator {
        Operator::None => true,
        Operator::Eq => vercmp(&candidate.full_version(), &atom.full_version().unwrap()) == Some(0),
        // PMS 8.3.1: "only the given number of version components is
        // used for comparison, i.e. the asterisk acts as a wildcard for
        // any further components." Real portage implements this as a
        // literal string-prefix match (not vercmp-based -- see
        // normalize_leading_zeros's doc comment) on
        // category/package-version[-rN], but only at a genuine
        // component boundary: real portage's own bug 560466 fix means
        // "1*" must NOT match "10" (both digits, no real boundary there)
        // even though "10" literally starts with "1" -- captured below
        // by the digit-adjacency check. category/package equality is
        // already guaranteed by match_from_list's own caller-side filter
        // before matches_version ever runs, so comparing just the
        // version[-rN] suffix (rather than the full
        // category/package-version[-rN] string real portage's own
        // implementation slices) is equivalent and simpler.
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

/// Mirrors _match_slot: exact slot match required; sub_slot only checked
/// if the atom specifies one. A candidate with no slot info at all always
/// passes (matches match_from_list's behavior for plain-string candidates
/// it can't determine a slot for -- see the module doc comment).
///
/// `atom.slot_operator` is never consulted here, deliberately: real
/// `_match_slot` doesn't look at it either -- only real `match_from_list`'s
/// own `if mydep.slot is not None:` guard (mirrored by the `atom.slot`
/// check below) decides whether slot-filtering happens at all. A bare
/// `:=`/`:*` atom has `slot == None` (no explicit slot was given), so it
/// already falls through this same early-return and matches any slot;
/// `:slot=` has `slot == Some(..)`, so it's filtered exactly like a plain
/// `:slot` atom would be. This is why adding slot-operator *parsing*
/// needed no changes at all to slot-operator *matching*.
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

/// Mirrors real `match_from_list`'s own final post-pass filter (only run
/// `if mydep.repo:` -- an atom with no `::repo` constraint never filters
/// on repo at all): a candidate is rejected only if it carries a KNOWN
/// repo that differs from the atom's. A candidate with no repo info at
/// all (`candidate.repo == None`, this pilot's default for any
/// plain-string candidate that never had `::repo` appended -- see
/// `dep_getrepo`'s own real semantics, which return `None` for a
/// repo-less string) always passes, regardless of what the atom asks
/// for -- real portage's own justification: a plain string generally
/// means "repo unknown," not "no repo," so it can't positively fail a
/// repo check.
fn matches_repo(atom: &Atom, candidate: &Candidate) -> bool {
    match &atom.repo {
        None => true,
        Some(want) => match &candidate.repo {
            None => true,
            Some(have) => have == want,
        },
    }
}

/// Ports real `match_from_list`'s own USE-dep post-pass (its
/// `if mydep.unevaluated_atom.use:` block, `lib/portage/dep/__init__.py`
/// lines 3143-3188) -- NOT called from `match_from_list` itself, since
/// that function only ever sees plain candidate strings, which carry no
/// IUSE/USE state at all (real `match_from_list` skips this same block
/// entirely for a plain-string candidate too -- its own `hasattr(x,
/// "use")` guard -- so this pilot's `match_from_list` staying unaware of
/// use deps isn't a divergence). Callers with real per-candidate
/// IUSE/USE state (`portage-repo`, which already computes both via
/// `read_md5_cache`/`effective_use_flags` for other reasons) call this
/// directly, after `match_from_list`'s own version/slot/repo filtering.
///
/// `iuse` is the candidate's own declared IUSE (flag names, `+`/`-`
/// default markers already stripped -- same shape `effective_use_flags`'s
/// callers already extract from md5-cache elsewhere); `enabled` is its
/// own effective (computed) USE set.
///
/// Real behavior, faithfully ported, not simplified: a use-dep flag with
/// no `(+)`/`(-)` default marker -- of ANY form, including the four
/// conditional ones below -- must be a real, declared IUSE flag on the
/// candidate, or the atom doesn't match this candidate at all (real
/// `_use_dep.required`, checked via `x.iuse.is_valid_flag(...)` before
/// anything else). Only the two *unconditional* forms, `flag` and
/// `-flag` (`UseDepOp::Enabled`/`Disabled`), actually constrain the
/// candidate's own enabled/disabled state; a `(+)`/`(-)` default is
/// consulted only for a flag that's missing from this candidate's own
/// IUSE, standing in for "as if the flag were enabled/disabled".
/// `flag?`/`!flag?`/`flag=`/`!flag=` (`UseDepOp::IfParentEnabled`/
/// `IfParentDisabled`/`EqualParent`/`OppositeParent`) impose NO
/// enabled/disabled constraint here at all -- this is real
/// `match_from_list`'s own genuine behavior (it only ever consults
/// `mydep.use.enabled`/`.disabled`, which real `_use_dep.__init__` populates
/// solely from the two unconditional forms; the four conditional ones
/// land in a separate `.conditional` structure that `match_from_list`
/// never reads), not a pilot simplification: evaluating a conditional
/// use-dep needs the *atom-owning* package's own USE state, a completely
/// different mechanism (dependency-string conditional evaluation) this
/// pilot doesn't have and `match_from_list` itself doesn't either.
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
                    && matches_repo(&atom, &candidate)
            })
            .collect(),
    )
}

/// Real `Atom.intersects()` (`lib/portage/dep/__init__.py`): despite the
/// name, a real, deliberately NARROW check -- real portage's own
/// docstring says so directly ("atoms with different cpv, operator or
/// use attributes cause this method to return False even though there
/// may actually be some intersection... TODO: Detect more forms of
/// intersection"). Ported field-for-field, skipping real portage's own
/// `self == other` fast-path shortcut (redundant, not a simplification
/// -- two textually-identical atoms already satisfy every check below
/// and fall through to `true` the same way): `cp` (category+package),
/// `use` (use-deps), `operator`, and `cpv` (category+package+version --
/// `operator` plus the full version/revision together, compared here as
/// `full_version()`) must ALL match exactly, not overlap and not
/// satisfy a range, before slot compatibility (`None` on either side,
/// or an identical value) decides the result. `repo` is deliberately
/// NOT checked here, matching real `intersects()` itself -- real
/// `action_deselect`'s own caller adds its own separate repo check
/// afterward (`and not (arg_atom.repo and not atom.repo)`, ported at
/// `run_deselect`'s own call site in `pretend.rs`, not folded in here).
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
        // Real _use_dep.required: any use-dep flag with no (+)/(-)
        // default must be a real, declared IUSE flag on the candidate,
        // or the atom simply doesn't match -- regardless of enabled/
        // disabled state.
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
        // "bar" isn't declared in IUSE at all -- the (+) default stands
        // in for "as if enabled", so this still matches.
        assert!(use_deps_satisfied(&ud, &HashSet::new(), &HashSet::new()));
    }

    #[test]
    fn minus_default_treats_a_missing_flag_as_disabled() {
        let ud = use_deps("dev-libs/foo[-bar(-)]");
        assert!(use_deps_satisfied(&ud, &HashSet::new(), &HashSet::new()));
    }

    #[test]
    fn plus_default_does_not_rescue_a_declared_but_disabled_flag() {
        // "bar" IS declared in IUSE here, so the (+) default (which only
        // ever applies to a MISSING flag) doesn't apply -- the candidate's
        // own actual (disabled) state governs instead.
        let ud = use_deps("dev-libs/foo[bar(+)]");
        let iuse = HashSet::from(["bar".to_string()]);
        assert!(!use_deps_satisfied(&ud, &iuse, &HashSet::new()));
    }

    #[test]
    fn conditional_forms_only_require_the_flag_be_declared_no_state_constraint() {
        // flag? / !flag? / flag= / !flag= never constrain enabled/disabled
        // state in match_from_list itself (see use_deps_satisfied's own
        // doc comment) -- only the "must be declared IUSE" gate applies.
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

    /// Real portage's own authoritative test vectors for this exact
    /// USE-dep-vs-Package-mock matching behavior --
    /// lib/portage/tests/dep/test_match_from_list.py's own
    /// testMatch_from_list, the `dev-libs/A[...]` cases (lines 151-195).
    /// Its own `Package` mock derives a candidate's `iuse` from
    /// `atom.use.required` (the flags with NO `(+)`/`(-)` default in the
    /// atom string used to construct that particular candidate) and
    /// `enabled` from `atom.use.enabled` (bare, non-`-`-prefixed tokens)
    /// -- reproduced by hand below via `use_deps`/`enabled_of` on the
    /// same construction atom strings, rather than re-deriving the
    /// mock's own logic.
    fn enabled_of(atom_str: &str) -> HashSet<String> {
        use_deps(atom_str)
            .into_iter()
            .filter(|ud| ud.op == UseDepOp::Enabled)
            .map(|ud| ud.flag)
            .collect()
    }

    fn iuse_of(atom_str: &str) -> HashSet<String> {
        // "required": every use-dep flag with no (+)/(-) default marker.
        use_deps(atom_str)
            .into_iter()
            .filter(|ud| ud.default.is_none())
            .map(|ud| ud.flag)
            .collect()
    }

    #[test]
    fn real_test_suite_vector_foo_and_bar_both_required_neither_declares_bar() {
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
    fn real_test_suite_vector_foo_and_bar_both_required_one_satisfies() {
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
    fn real_test_suite_vector_plus_default_rescues_an_undeclared_flag_only() {
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
    fn real_test_suite_vector_minus_default_on_a_required_enabled_flag_is_a_contradiction() {
        // "bar(-)" (no "-" prefix, so op=Enabled) defaults an UNDECLARED
        // "bar" to disabled -- directly contradicting "bar" being
        // required enabled, so a candidate missing "bar" entirely is
        // rejected outright, regardless of "foo".
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
    fn real_test_suite_vector_minus_bar_default_combines_with_a_plain_required_flag() {
        let ud = use_deps("dev-libs/A[foo,-bar(-)]");
        // Package("=dev-libs/A-1[-foo,bar]"): bar IS declared here (no
        // default in ITS OWN construction atom), so bar(-)'s default
        // never applies -- foo is declared but disabled, violating the
        // plain "foo" (must-be-enabled) requirement.
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
        // Real Atom.intersects()'s own docstring: deliberately narrow,
        // "atoms with different cpv, operator or use attributes cause
        // this method to return False even though there may actually be
        // some intersection". `>=dev-libs/foo-1.0` would genuinely be
        // satisfied by version 1.0, but the operator itself must match
        // exactly here, not just range-satisfaction.
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
