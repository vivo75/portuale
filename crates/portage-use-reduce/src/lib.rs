// Rust port of `portage.dep.use_reduce` (lib/portage/dep/__init__.py),
// restricted to `flat=True` mode: the tokenizer and bracket/use-conditional
// handling, ported as-is, error behavior included.
// Portage: lib/portage/dep/__init__.py, use_reduce
//
// Out of scope are only optional parameters the harness never exercises:
// `masklist`, `excludeall`, `is_src_uri`/the `->` arrow token,
// `opconvert`, and `token_class`/`is_valid_flag` (tokens stay opaque
// strings here).
//
// `subset` IS ported (`use_reduce_flat_subset`, against Portage's
// `select_subset`) via its own tree type (`DepNode`) feeding into the
// unmodified `use_reduce_flat`. `use_reduce_structured` ports the
// `flat=False`/`opconvert=False` mode (verified byte-for-byte against
// Portage over thousands of randomized dep strings).
// `use_reduce_flat_disjunctive` resolves `"||"` groups to the
// highest-`AltPreference` alternative a caller closure reports. This crate
// stays atom-agnostic throughout.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

/// Parse/reduce error from a USE dependency string. `Display` reproduces
/// Portage's messages byte-for-byte (the contract suite pins these).
#[derive(Debug)]
pub enum Error {
    /// `invalid use flag '{flag}' in conditional '{conditional}'`
    InvalidUseFlag { flag: String, conditional: String },
    /// `expected: {expected}, got: ')', token {token}` where `expected`
    /// is either `dependency string` or `'('` at the call site.
    ExpectedCloseParen {
        expected: &'static str,
        token: usize,
    },
    /// `no matching '(' for ')', token {token}`
    NoMatchingOpen { token: usize },
    /// `expected: '(', got: '||', token {token}`
    ExpectedOpenGotOr { token: usize },
    /// `expected: '(', got: '{token}', token {at}`
    ExpectedOpenGotToken { token: String, at: usize },
    /// `SRC_URI arrow are only allowed in SRC_URI: token {token}`
    SrcUriArrow { token: usize },
    /// `conditional '{s}' not followed by a group`
    ConditionalNotFollowedByGroup { s: String },
    /// `'||' not followed by a group`
    OrNotFollowedByGroup,
    /// `Missing ')' at end of string`
    MissingCloseParen,
    /// `Missing '(' at end of string`
    MissingOpenParen,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidUseFlag { flag, conditional } => {
                write!(
                    f,
                    "invalid use flag '{flag}' in conditional '{conditional}'"
                )
            }
            Error::ExpectedCloseParen { expected, token } => {
                write!(f, "expected: {expected}, got: ')', token {token}")
            }
            Error::NoMatchingOpen { token } => {
                write!(f, "no matching '(' for ')', token {token}")
            }
            Error::ExpectedOpenGotOr { token } => {
                write!(f, "expected: '(', got: '||', token {token}")
            }
            Error::ExpectedOpenGotToken { token, at } => {
                write!(f, "expected: '(', got: '{token}', token {at}")
            }
            Error::SrcUriArrow { token } => {
                write!(
                    f,
                    "SRC_URI arrow are only allowed in SRC_URI: token {token}"
                )
            }
            Error::ConditionalNotFollowedByGroup { s } => {
                write!(f, "conditional '{s}' not followed by a group")
            }
            Error::OrNotFollowedByGroup => write!(f, "'||' not followed by a group"),
            Error::MissingCloseParen => write!(f, "Missing ')' at end of string"),
            Error::MissingOpenParen => write!(f, "Missing '(' at end of string"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

fn useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}

/// A node of the nested-list shape Portage's `paren_reduce` builds: a
/// flag/`"||"` marker stays a sibling string before its own group. This
/// builder skips the redundant-bracket-collapsing optimizations.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DepNode {
    Str(String),
    Group(Vec<DepNode>),
}

/// Re-serializes a `DepNode` tree into a flat token stream for the final
/// `use_reduce_flat` flattening.
fn serialize_dep_tree(nodes: &[DepNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            DepNode::Str(s) => out.push(s.clone()),
            DepNode::Group(children) => {
                out.push("(".to_string());
                serialize_dep_tree(children, out);
                out.push(")".to_string());
            }
        }
    }
}

/// The placeholder atom Portage substitutes for a `|| ( )` group that
/// reduced to nothing (EAPI 7+, `empty_groups_always_true` false).
const EMPTY_ANY_OF: &str = "__const__/empty-any-of";

fn ends_in_any_of_dep(stack: &[Vec<DepNode>], k: isize) -> bool {
    if k < 0 {
        return false;
    }
    matches!(
        stack.get(k as usize).and_then(|l| l.last()),
        Some(DepNode::Str(s)) if s == "||"
    )
}

/// Portage's `last_any_of_operator_level`: the level of the last `||`
/// operator still in effect for the current level, or `-1`.
fn last_any_of_operator_level(stack: &[Vec<DepNode>], mut k: isize) -> isize {
    while k >= 0 {
        if let Some(DepNode::Str(s)) = stack[k as usize].last() {
            if s == "||" {
                return k;
            }
            if !s.ends_with('?') {
                return -1;
            }
        }
        k -= 1;
    }
    -1
}

/// Portage's `special_append` (non-`opconvert` path): "use extend instead
/// of append if possible -- this kills all redundant brackets."
fn special_append(stack: &mut [Vec<DepNode>], level: usize, l: Vec<DepNode>, is_single: bool) {
    if !is_single {
        stack[level].push(DepNode::Group(l));
        return;
    }
    let l0_is_or = matches!(l.first(), Some(DepNode::Str(s)) if s == "||");
    if l0_is_or && ends_in_any_of_dep(stack, level as isize - 1) {
        // `l == ["||", Group]` -> splice the group's own contents.
        if let Some(DepNode::Group(inner)) = l.into_iter().nth(1) {
            stack[level].extend(inner);
        }
    } else if l.len() == 1 && matches!(l[0], DepNode::Group(_)) {
        let last = last_any_of_operator_level(stack, level as isize - 1);
        let DepNode::Group(inner) = l.into_iter().next().unwrap() else {
            unreachable!()
        };
        if last == -1 {
            stack[level].extend(inner);
        } else {
            stack[level].push(DepNode::Group(inner));
        }
    } else {
        stack[level].extend(l);
    }
}

/// Portage's `_add_pkg_dep_string` `"||"` resolution, simplified: of every
/// alternative, picks the **first one with the highest [`AltPreference`]**
/// the probe reports. An alternative an inactive conditional emptied is
/// dropped before ranking. When every alternative empties, the group is
/// dropped if `empty_groups_always_true`, else it becomes the
/// [`EMPTY_ANY_OF`] placeholder. With *no* satisfiable alternative, the
/// whole group is kept as `use_reduce_flat` flattened it, so a dependency
/// portuale can't resolve is never silently dropped.
///
/// `tie_break` runs only on a tie at the best selectable rank: it gets the
/// tied alternatives' flattened atom lists, in order, and returns which
/// index wins (`&mut |_| 0` keeps first-listed-in-bin-wins).
// Portage: lib/portage/dep/dep_check.py, dep_zapdeps
pub type TieBreak<'a> = dyn FnMut(&[Vec<String>]) -> usize + 'a;
