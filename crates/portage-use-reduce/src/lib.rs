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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Conditionals are active iff the flag is in `uselist` (negated for `!flag?`).
    Normal,
    /// Every conditional is active, regardless of `uselist` (Portage's `matchall=True`).
    All,
    /// Every conditional is inactive, regardless of `uselist` (Portage's `matchnone=True`).
    None,
}

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

/// Whether a `flag?`/`!flag?` token is active under `uselist`/`mode`.
/// Shared with portage-repo's LICENSE-grammar parser.
pub fn is_active(
    conditional: &str,
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<bool, Error> {
    let (flag, negated) = match conditional.strip_prefix('!') {
        Some(rest) => (&rest[..rest.len() - 1], true),
        None => (&conditional[..conditional.len() - 1], false),
    };
    if !useflag_re().is_match(flag) {
        return Err(Error::InvalidUseFlag {
            flag: flag.to_string(),
            conditional: conditional.to_string(),
        });
    }
    Ok(match mode {
        MatchMode::All => true,
        MatchMode::None => false,
        MatchMode::Normal => {
            (uselist.contains(flag) && !negated) || (!uselist.contains(flag) && negated)
        }
    })
}

/// `tokens` is the dep string pre-split on whitespace; returns the
/// flattened token list, or an `InvalidDependString`-mirroring error.
pub fn use_reduce_flat(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<Vec<String>, Error> {
    let mut stack: Vec<Vec<String>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(Error::ExpectedCloseParen {
                        expected: "dependency string",
                        token: pos + 2,
                    });
                }
                // "(" satisfies any pending "||"/"flag?" requirement.
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(Error::ExpectedCloseParen {
                        expected: "'('",
                        token: pos + 1,
                    });
                }
                if stack.len() <= 1 {
                    return Err(Error::NoMatchingOpen { token: pos + 1 });
                }
                let l = stack.pop().unwrap();
                let top = stack.last_mut().unwrap();
                let conditional = match top.last() {
                    Some(last) if last.ends_with('?') => Some(top.pop().unwrap()),
                    _ => None,
                };
                match conditional {
                    Some(cond) => {
                        if is_active(&cond, uselist, mode)? {
                            stack.last_mut().unwrap().extend(l);
                        }
                    }
                    None => stack.last_mut().unwrap().extend(l),
                }
            }
            "||" => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotOr { token: pos + 1 });
                }
                need_bracket = true;
                stack.last_mut().unwrap().push("||".to_string());
            }
            "->" => {
                // `is_src_uri` is always false here, so arrows always fail.
                return Err(Error::SrcUriArrow { token: pos + 1 });
            }
            _ => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotToken {
                        token: token.to_string(),
                        at: pos + 1,
                    });
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(token.clone());
            }
        }
    }

    if stack.len() != 1 {
        return Err(Error::MissingCloseParen);
    }
    if need_bracket {
        return Err(Error::MissingOpenParen);
    }

    Ok(stack.pop().unwrap())
}

/// A node of the nested-list shape Portage's `paren_reduce` builds: a
/// flag/`"||"` marker stays a sibling string before its own group. This
/// builder skips the redundant-bracket-collapsing optimizations.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DepNode {
    Str(String),
    Group(Vec<DepNode>),
}

/// Builds the nested `DepNode` tree from `tokens`: same validation as
/// `use_reduce_flat` (same error messages), but building a tree instead
/// of eagerly flattening, so `select_subset` has actual group boundaries.
fn build_dep_tree(tokens: &[String]) -> Result<Vec<DepNode>, Error> {
    let mut stack: Vec<Vec<DepNode>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(Error::ExpectedCloseParen {
                        expected: "dependency string",
                        token: pos + 2,
                    });
                }
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(Error::ExpectedCloseParen {
                        expected: "'('",
                        token: pos + 1,
                    });
                }
                if stack.len() <= 1 {
                    return Err(Error::NoMatchingOpen { token: pos + 1 });
                }
                let l = stack.pop().unwrap();
                stack.last_mut().unwrap().push(DepNode::Group(l));
            }
            "||" => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotOr { token: pos + 1 });
                }
                need_bracket = true;
                stack
                    .last_mut()
                    .unwrap()
                    .push(DepNode::Str("||".to_string()));
            }
            "->" => {
                return Err(Error::SrcUriArrow { token: pos + 1 });
            }
            _ => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotToken {
                        token: token.to_string(),
                        at: pos + 1,
                    });
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(DepNode::Str(token.clone()));
            }
        }
    }

    if stack.len() != 1 {
        return Err(Error::MissingCloseParen);
    }
    if need_bracket {
        return Err(Error::MissingOpenParen);
    }

    Ok(stack.pop().unwrap())
}

struct SelectSubsetArgs<'a> {
    nodes: &'a [DepNode],
    disjunction: bool,
    selected: bool,
    subset: &'a HashSet<String>,
    uselist: &'a HashSet<String>,
    mode: MatchMode,
}

/// Portage's `select_subset`: walks the parsed tree keeping only atoms
/// reachable through a conditional whose flag is in `subset`.
/// `disjunction` holds directly inside a `"||"` member list; `selected`
/// holds once inside a subset-matching conditional. Other conditionals
/// are still evaluated via `is_active`.
fn select_subset(args: SelectSubsetArgs<'_>) -> Result<Vec<DepNode>, Error> {
    let SelectSubsetArgs {
        nodes,
        disjunction,
        selected,
        subset,
        uselist,
        mode,
    } = args;
    let mut result: Vec<DepNode> = Vec::new();
    let mut iter = nodes.iter();
    while let Some(node) = iter.next() {
        match node {
            DepNode::Group(children) => {
                // A bare grouping: selection state passes through unchanged.
                let sub = select_subset(SelectSubsetArgs {
                    nodes: children,
                    disjunction: false,
                    selected,
                    subset,
                    uselist,
                    mode,
                })?;
                if disjunction {
                    if !sub.is_empty() {
                        result.push(DepNode::Group(sub));
                    }
                } else {
                    result.extend(sub);
                }
            }
            DepNode::Str(s) if s.ends_with('?') => {
                let Some(DepNode::Group(children)) = iter.next() else {
                    return Err(Error::ConditionalNotFollowedByGroup { s: s.to_string() });
                };
                if is_active(s, uselist, mode)? {
                    let flag = &s[..s.len() - 1];
                    let now_selected = selected || subset.contains(flag);
                    let sub = select_subset(SelectSubsetArgs {
                        nodes: children,
                        disjunction: false,
                        selected: now_selected,
                        subset,
                        uselist,
                        mode,
                    })?;
                    if disjunction {
                        if !sub.is_empty() {
                            result.push(DepNode::Group(sub));
                        }
                    } else {
                        result.extend(sub);
                    }
                }
            }
            DepNode::Str(s) if s == "||" => {
                let Some(DepNode::Group(children)) = iter.next() else {
                    return Err(Error::OrNotFollowedByGroup);
                };
                let sub = select_subset(SelectSubsetArgs {
                    nodes: children,
                    disjunction: true,
                    selected,
                    subset,
                    uselist,
                    mode,
                })?;
                if !sub.is_empty() {
                    if disjunction {
                        result.extend(sub);
                    } else {
                        result.push(DepNode::Str("||".to_string()));
                        result.push(DepNode::Group(sub));
                    }
                }
            }
            DepNode::Str(s) => {
                if selected {
                    result.push(DepNode::Str(s.clone()));
                }
            }
        }
    }
    Ok(result)
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

/// Portage's `use_reduce` in `flat=False`, `opconvert=False` mode: the
/// nested stack reducer with every redundant-bracket optimization ported
/// as-is. An inactive `flag? ( )` group is dropped, an emptied `|| ( )`
/// becomes [`EMPTY_ANY_OF`]. Stays token/atom-agnostic; returns the reduced
/// tree re-serialized to a flat token stream.
// Portage: lib/portage/dep/__init__.py, use_reduce
pub fn use_reduce_structured(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<Vec<String>, Error> {
    let mut stack: Vec<Vec<DepNode>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(Error::ExpectedCloseParen {
                        expected: "dependency string",
                        token: pos + 1,
                    });
                }
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(Error::ExpectedCloseParen {
                        expected: "'('",
                        token: pos + 1,
                    });
                }
                if stack.len() <= 1 {
                    return Err(Error::NoMatchingOpen { token: pos + 1 });
                }
                let mut l = stack.pop().unwrap();
                let level = stack.len() - 1;

                let is_single =
                    l.len() == 1 || (l.len() == 2 && matches!(&l[0], DepNode::Str(s) if s == "||"));
                let mut ignore = false;

                if let Some(DepNode::Str(last)) = stack[level].last() {
                    let last = last.clone();
                    if last == "||" && l.is_empty() {
                        l.push(DepNode::Str(EMPTY_ANY_OF.to_string()));
                        stack[level].pop();
                    } else if last.ends_with('?') {
                        if !is_active(&last, uselist, mode)? {
                            ignore = true;
                        }
                        stack[level].pop();
                    }
                }

                if !l.is_empty() && !ignore {
                    let ends_level = ends_in_any_of_dep(&stack, level as isize);
                    let ends_above = ends_in_any_of_dep(&stack, level as isize - 1);
                    if !ends_above && !ends_level {
                        stack[level].extend(l);
                    } else if stack[level].is_empty() {
                        special_append(&mut stack, level, l, is_single);
                    } else if is_single && ends_level {
                        stack[level].pop();
                        special_append(&mut stack, level, l, is_single);
                    } else if ends_level && ends_above {
                        stack[level].pop();
                        stack[level].extend(l);
                    } else {
                        special_append(&mut stack, level, l, is_single);
                    }
                }
            }
            "||" => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotOr { token: pos + 1 });
                }
                need_bracket = true;
                stack
                    .last_mut()
                    .unwrap()
                    .push(DepNode::Str("||".to_string()));
            }
            "->" => {
                return Err(Error::SrcUriArrow { token: pos + 1 });
            }
            _ => {
                if need_bracket {
                    return Err(Error::ExpectedOpenGotToken {
                        token: token.to_string(),
                        at: pos + 1,
                    });
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(DepNode::Str(token.clone()));
            }
        }
    }

    if stack.len() != 1 {
        return Err(Error::MissingCloseParen);
    }
    if need_bracket {
        return Err(Error::MissingOpenParen);
    }

    let mut out = Vec::new();
    serialize_dep_tree(&stack.pop().unwrap(), &mut out);
    Ok(out)
}

/// Portage's `subset` parameter as a two-pass operation: `select_subset`
/// over the nested structure first, then the ordinary `use_reduce_flat`.
/// Portage's `--with-test-deps` call extracts exactly the *additional*
/// deps a `test?` conditional contributes once `"test"` is forced on.
// Portage: lib/portage/dep/__init__.py, use_reduce; depgraph.py
pub fn use_reduce_flat_subset(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
    subset: &HashSet<String>,
) -> Result<Vec<String>, Error> {
    let tree = build_dep_tree(tokens)?;
    let filtered = select_subset(SelectSubsetArgs {
        nodes: &tree,
        disjunction: false,
        selected: false,
        subset,
        uselist,
        mode,
    })?;
    let mut reserialized = Vec::new();
    serialize_dep_tree(&filtered, &mut reserialized);
    use_reduce_flat(&reserialized, uselist, mode)
}

/// One "alternative" inside a `"||"` group, consumed as one logical unit
/// even though a conditional spans two sibling `DepNode`s.
fn next_alternative<'a>(
    iter: &mut std::slice::Iter<'a, DepNode>,
) -> Option<Result<Vec<DepNode>, Error>> {
    let node = iter.next()?;
    Some(match node {
        DepNode::Str(s) if s.ends_with('?') => match iter.next() {
            Some(group @ DepNode::Group(_)) => Ok(vec![node.clone(), group.clone()]),
            _ => Err(Error::ConditionalNotFollowedByGroup { s: s.to_string() }),
        },
        other => Ok(vec![other.clone()]),
    })
}

/// How much a caller prefers one `"||"` alternative -- port of Portage
/// `dep_zapdeps`'s `choice_bins`: nine bins, first atom of the first
/// *selectable* choice in bin order.
// Portage: lib/portage/dep/dep_check.py, dep_zapdeps
///
/// ```text
/// for allow_masked in (False, True):
///     for choices in choice_bins:
///         for choice in choices:
///             if choice.all_available or allow_masked:
///                 return choice.atoms
/// ```
///
/// ```text
/// 0  preferred_in_graph          ( = preferred_installed = preferred_any_slot )
/// 1  preferred_non_installed
/// 2  unsat_use_in_graph
/// 3  unsat_use_installed
/// 4  unsat_use_non_installed
/// 5  other_installed
/// 6  other_installed_some
/// 7  other_installed_any_slot
/// 8  other
/// ```
///
/// Selection is two passes: only `is_selectable` ranks first, then the
/// `other_*` bins. The guard is portuale's own and stays `Unsatisfiable`.
///
/// The enum is ordered by bin ordinal (higher discriminant = more
/// preferred), with one extra lowest sentinel [`AltPreference::Unsatisfiable`].
/// Deriving `Ord` gives selection exactly the bin-order in either pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum AltPreference {
    /// No atom in this alternative resolves at all (portuale's guard only).
    /// Never selected, in either pass.
    #[default]
    Unsatisfiable,
    /// Bin 8: every atom resolves to *something*, but at least one only
    /// via a masked/forced USE change. Second-pass-only.
    Other,
    /// Bin 7: no full match exists, but some bare `cat/pkg` is installed.
    /// Second-pass-only.
    OtherInstalledAnySlot,
    /// Bin 6: like above, but at least one full vdb match succeeds.
    /// Second-pass-only.
    OtherInstalledSome,
    /// Bin 5: every full vdb match succeeds, yet not `all_available`.
    /// Second-pass-only.
    OtherInstalled,
    /// Bin 4: resolvable with unmasked USE-deps, but no single candidate
    /// satisfies them all. Selectable.
    UnsatUseNonInstalled,
    /// Bin 3: like `UnsatUseNonInstalled` but every atom's `cat/pkg` is
    /// already installed. Selectable.
    UnsatUseInstalled,
    /// Bin 2: like `UnsatUseNonInstalled` but every atom is already in
    /// the merge graph. Selectable.
    UnsatUseInGraph,
    /// Bin 1: resolvable, but at least one `cat/pkg` is not installed.
    /// Selectable.
    Available,
    /// Bin 0: every `cat/pkg` installed or in the graph. The top bin.
    /// Selectable.
    Installed,
}

impl AltPreference {
    /// Whether Portage's loop would pick this rank on its first pass.
    /// The `other_*` bins are only considered when nothing is selectable.
    pub fn is_selectable(self) -> bool {
        matches!(
            self,
            AltPreference::Installed
                | AltPreference::Available
                | AltPreference::UnsatUseInGraph
                | AltPreference::UnsatUseInstalled
                | AltPreference::UnsatUseNonInstalled
        )
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

pub struct UseReduceFlatDisjunctiveArgs<'a> {
    pub tokens: &'a [String],
    pub uselist: &'a HashSet<String>,
    pub mode: MatchMode,
    pub empty_groups_always_true: bool,
    pub tie_break: &'a mut TieBreak<'a>,
}

pub fn use_reduce_flat_disjunctive(
    args: UseReduceFlatDisjunctiveArgs<'_>,
    alternative_satisfiable: &mut impl FnMut(&[String]) -> AltPreference,
) -> Result<Vec<String>, Error> {
    let UseReduceFlatDisjunctiveArgs {
        tokens,
        uselist,
        mode,
        empty_groups_always_true,
        tie_break,
    } = args;
    let tree = build_dep_tree(tokens)?;
    let resolved = resolve_disjunctions(
        ResolveDisjunctionsArgs {
            nodes: &tree,
            uselist,
            mode,
            empty_groups_always_true,
            tie_break,
        },
        alternative_satisfiable,
    )?;
    let mut reserialized = Vec::new();
    serialize_dep_tree(&resolved, &mut reserialized);
    use_reduce_flat(&reserialized, uselist, mode)
}

struct ResolveDisjunctionsArgs<'a> {
    nodes: &'a [DepNode],
    uselist: &'a HashSet<String>,
    mode: MatchMode,
    empty_groups_always_true: bool,
    tie_break: &'a mut TieBreak<'a>,
}

fn resolve_disjunctions(
    args: ResolveDisjunctionsArgs<'_>,
    alternative_satisfiable: &mut impl FnMut(&[String]) -> AltPreference,
) -> Result<Vec<DepNode>, Error> {
    let ResolveDisjunctionsArgs {
        nodes,
        uselist,
        mode,
        empty_groups_always_true,
        tie_break,
    } = args;
    let mut result: Vec<DepNode> = Vec::new();
    // Portage drains the plain `dep_stack` before the disjunctive stack,
    // so plain results are emitted first, then the `||`-chosen ones, each
    // in original order. An unresolved `||` group stays in place.
    // Portage: lib/portage/_emerge/depgraph.py, _create_graph
    let mut deferred: Vec<DepNode> = Vec::new();
    let mut iter = nodes.iter();
    while let Some(node) = iter.next() {
        match node {
            DepNode::Group(children) => {
                let resolved = resolve_disjunctions(
                    ResolveDisjunctionsArgs {
                        nodes: children,
                        uselist,
                        mode,
                        empty_groups_always_true,
                        tie_break,
                    },
                    alternative_satisfiable,
                )?;
                result.push(DepNode::Group(resolved));
            }
            DepNode::Str(s) if s.ends_with('?') => {
                let Some(DepNode::Group(children)) = iter.next() else {
                    return Err(Error::ConditionalNotFollowedByGroup { s: s.to_string() });
                };
                let resolved = resolve_disjunctions(
                    ResolveDisjunctionsArgs {
                        nodes: children,
                        uselist,
                        mode,
                        empty_groups_always_true,
                        tie_break,
                    },
                    alternative_satisfiable,
                )?;
                result.push(DepNode::Str(s.clone()));
                result.push(DepNode::Group(resolved));
            }
            DepNode::Str(s) if s == "||" => {
                let Some(DepNode::Group(alternatives)) = iter.next() else {
                    return Err(Error::OrNotFollowedByGroup);
                };
                // Two passes like Portage: rank every alternative, hand a tie
                // at the best bin to `tie_break`, and only consider the
                // non-selectable `other_*` bins when the first pass finds
                // nothing at all.
                let mut ranked: Vec<(AltPreference, Vec<DepNode>, Vec<String>)> = Vec::new();
                let mut alt_iter = alternatives.iter();
                while let Some(alt) = next_alternative(&mut alt_iter) {
                    let alt_nodes = alt?;
                    let mut flat = Vec::new();
                    serialize_dep_tree(&alt_nodes, &mut flat);
                    let Ok(flat_atoms) = use_reduce_flat(&flat, uselist, mode) else {
                        continue;
                    };
                    // An alternative an inactive conditional emptied is not
                    // a rankable choice: skip it before probing.
                    if flat_atoms.is_empty() {
                        continue;
                    }
                    let rank = alternative_satisfiable(&flat_atoms);
                    ranked.push((rank, alt_nodes, flat_atoms));
                }
                if ranked.is_empty() {
                    // Every alternative was emptied by inactive
                    // conditionals: drop the group, or emit the
                    // unsatisfiable placeholder as a `||`-chosen atom.
                    if !empty_groups_always_true {
                        deferred.push(DepNode::Str(EMPTY_ANY_OF.to_string()));
                    }
                    continue;
                }
                let mut selectable: Vec<&(AltPreference, Vec<DepNode>, Vec<String>)> = ranked
                    .iter()
                    .filter(|(r, _, _)| r.is_selectable())
                    .collect();
                if selectable.is_empty() {
                    // Second pass: never the circular-self-dep guard (no
                    // Portage equivalent), only a genuine `other_*` bin.
                    selectable = ranked
                        .iter()
                        .filter(|(r, _, _)| *r != AltPreference::Unsatisfiable)
                        .collect();
                }
                let best_rank = selectable.iter().map(|(r, _, _)| *r).max();
                let chosen_nodes = match best_rank {
                    None => None,
                    Some(best_rank) => {
                        let mut tied: Vec<(Vec<DepNode>, Vec<String>)> = selectable
                            .into_iter()
                            .filter(|(r, _, _)| *r == best_rank)
                            .map(|(_, nodes, flat)| (nodes.clone(), flat.clone()))
                            .collect();
                        let winner = if tied.len() == 1 {
                            0
                        } else {
                            let flats: Vec<Vec<String>> =
                                tied.iter().map(|(_, flat)| flat.clone()).collect();
                            tie_break(&flats).min(tied.len() - 1)
                        };
                        Some(tied.swap_remove(winner).0)
                    }
                };
                let chosen = match chosen_nodes {
                    Some(alt_nodes) => Some(resolve_disjunctions(
                        ResolveDisjunctionsArgs {
                            nodes: &alt_nodes,
                            uselist,
                            mode,
                            empty_groups_always_true,
                            tie_break,
                        },
                        alternative_satisfiable,
                    )?),
                    None => None,
                };
                match chosen {
                    Some(alt_nodes) => deferred.extend(alt_nodes),
                    None => {
                        result.push(DepNode::Str("||".to_string()));
                        result.push(DepNode::Group(alternatives.clone()));
                    }
                }
            }
            DepNode::Str(s) => {
                result.push(DepNode::Str(s.clone()));
            }
        }
    }
    result.extend(deferred);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Tests map the old `bool` probe onto `Available` / `Unsatisfiable`;
    /// `Installed` is covered by portage-repo and the contract suite.
    fn pref(satisfiable: bool) -> AltPreference {
        if satisfiable {
            AltPreference::Available
        } else {
            AltPreference::Unsatisfiable
        }
    }

    #[test]
    fn subset_extracts_only_the_gated_atoms() {
        let result = use_reduce_flat_subset(
            &toks("foobar test? ( dev-libs/a dev-libs/b )"),
            &set(&["test"]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a", "dev-libs/b"]);
    }

    #[test]
    fn subset_excludes_deps_gated_by_a_different_conditional() {
        let result = use_reduce_flat_subset(
            &toks("foo? ( dev-libs/a ) test? ( dev-libs/b )"),
            &set(&["foo", "test"]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
    }

    #[test]
    fn subset_still_honors_a_nested_non_subset_conditional_normally() {
        // "bar" isn't in the subset, so it is evaluated normally.
        let result_bar_on = use_reduce_flat_subset(
            &toks("test? ( bar? ( dev-libs/a ) )"),
            &set(&["test", "bar"]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert_eq!(result_bar_on, vec!["dev-libs/a"]);

        let result_bar_off = use_reduce_flat_subset(
            &toks("test? ( bar? ( dev-libs/a ) )"),
            &set(&["test"]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert!(result_bar_off.is_empty());
    }

    #[test]
    fn subset_negated_conditional_never_contributes() {
        // "!test?" never matches "test" in the subset.
        let result = use_reduce_flat_subset(
            &toks("!test? ( dev-libs/a )"),
            &set(&[]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn subset_preserves_an_any_of_group_that_survives_filtering() {
        // Only the test-gated alternative survives; the "||" marker stays.
        let result = use_reduce_flat_subset(
            &toks("|| ( foo? ( dev-libs/a ) test? ( dev-libs/b ) )"),
            &set(&["test"]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert_eq!(result, vec!["||", "dev-libs/b"]);
    }

    #[test]
    fn subset_of_an_unconditional_dep_string_yields_nothing() {
        let result = use_reduce_flat_subset(
            &toks("dev-libs/a dev-libs/b"),
            &set(&[]),
            MatchMode::Normal,
            &set(&["test"]),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn disjunctive_picks_the_first_satisfiable_alternative() {
        // Only "b" is satisfiable; the rest is dropped entirely.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b dev-libs/c )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |atoms| pref(atoms == ["dev-libs/b"]),
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
    }

    #[test]
    fn disjunctive_falls_back_to_every_alternative_when_none_satisfiable() {
        // Nothing satisfiable: falls back to flattening everything.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |_| AltPreference::Unsatisfiable,
        )
        .unwrap();
        assert_eq!(result, vec!["||", "dev-libs/a", "dev-libs/b"]);
    }

    #[test]
    fn disjunctive_treats_a_bracketed_multi_atom_alternative_as_one_unit() {
        // The first alternative is the pair (a AND b); it needs both.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( ( dev-libs/a dev-libs/b ) dev-libs/c )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |atoms| pref(atoms == ["dev-libs/a", "dev-libs/b"]),
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a", "dev-libs/b"]);
    }

    #[test]
    fn disjunctive_skips_the_bracketed_pair_when_only_one_half_is_satisfiable() {
        // Only "a" alone is satisfiable, so "c" wins instead.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( ( dev-libs/a dev-libs/b ) dev-libs/c )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |atoms| pref(atoms == ["dev-libs/c"]),
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/c"]);
    }

    #[test]
    fn disjunctive_skips_an_emptied_alternative_and_selects_the_survivor() {
        // With "foo" off the emptied alternative is skipped; the probe
        // panics on an empty slice, so it must never see one.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( foo? ( dev-libs/a ) dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |atoms: &[String]| {
                assert!(
                    !atoms.is_empty(),
                    "an emptied alternative must never reach the probe"
                );
                pref(atoms == ["dev-libs/b"])
            },
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
    }

    #[test]
    fn disjunctive_emptied_group_becomes_empty_any_of_placeholder() {
        // Fully-emptied group under EAPI 7+ rules: placeholder atom.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( foo? ( dev-libs/a ) )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: false,
                tie_break: &mut |_| 0,
            },
            &mut |_: &[String]| -> AltPreference {
                panic!("no surviving alternative may reach the probe")
            },
        )
        .unwrap();
        assert_eq!(result, vec!["__const__/empty-any-of"]);
    }

    #[test]
    fn disjunctive_emptied_group_is_dropped_when_empty_groups_always_true() {
        // Fully-emptied group under EAPI <= 6 rules: dropped.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( foo? ( dev-libs/a ) )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |_: &[String]| -> AltPreference {
                panic!("no surviving alternative may reach the probe")
            },
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn disjunctive_prefers_an_installed_alternative_over_an_earlier_available_one() {
        // Bin 0 beats bin 1: `b` wins even though `a` is listed first.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |atoms: &[String]| {
                if atoms == ["dev-libs/b"] {
                    AltPreference::Installed
                } else {
                    AltPreference::Available
                }
            },
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
    }

    #[test]
    fn disjunctive_defers_a_tie_to_tie_break_and_keeps_first_when_it_says_zero() {
        // A tie: `tie_break` decides; `&mut |_| 0` keeps the first.
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |_| AltPreference::Available,
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a"]);
    }

    #[test]
    fn disjunctive_ties_call_tie_break_with_every_tied_alternatives_own_flat_atoms() {
        // `tie_break` gets the tied alternatives' flat atoms, in order.
        let mut seen: Vec<Vec<String>> = Vec::new();
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |alts: &[Vec<String>]| {
                    seen = alts.to_vec();
                    1
                },
            },
            &mut |_| AltPreference::Available,
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
        assert_eq!(
            seen,
            vec![
                vec!["dev-libs/a".to_string()],
                vec!["dev-libs/b".to_string()]
            ]
        );
    }

    #[test]
    fn disjunctive_never_calls_tie_break_for_a_single_alternative_at_the_best_rank() {
        // No tie, so `tie_break` must not run at all.
        let mut called = false;
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("|| ( dev-libs/a dev-libs/b )"),
                uselist: &HashSet::new(),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| {
                    called = true;
                    0
                },
            },
            &mut |atoms: &[String]| {
                if atoms == ["dev-libs/b"] {
                    AltPreference::Installed
                } else {
                    AltPreference::Available
                }
            },
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
        assert!(
            !called,
            "a single best-rank alternative must skip tie_break"
        );
    }

    #[test]
    fn disjunctive_leaves_non_disjunctive_deps_untouched() {
        let result = use_reduce_flat_disjunctive(
            UseReduceFlatDisjunctiveArgs {
                tokens: &toks("dev-libs/a foo? ( dev-libs/b )"),
                uselist: &set(&["foo"]),
                mode: MatchMode::Normal,
                empty_groups_always_true: true,
                tie_break: &mut |_| 0,
            },
            &mut |_| AltPreference::Unsatisfiable,
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a", "dev-libs/b"]);
    }

    /// Expected values from Portage's `use_reduce` (`flat=False` default).
    #[test]
    fn structured_matches_portage_use_reduce_normalization() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            ("c/a c/b", &[], &["c/a", "c/b"]),
            ("( c/a c/b )", &[], &["c/a", "c/b"]),
            ("c/a ( ( c/b ) )", &[], &["c/a", "c/b"]),
            ("|| ( c/a c/b )", &[], &["||", "(", "c/a", "c/b", ")"]),
            ("|| ( c/b c/a )", &[], &["||", "(", "c/b", "c/a", ")"]),
            ("|| ( c/a )", &[], &["c/a"]),
            (
                "|| ( ( c/a c/b ) c/c )",
                &[],
                &["||", "(", "(", "c/a", "c/b", ")", "c/c", ")"],
            ),
            (
                "|| ( c/a ( c/b c/c ) )",
                &[],
                &["||", "(", "c/a", "(", "c/b", "c/c", ")", ")"],
            ),
            (
                "|| ( || ( c/a c/b ) c/c )",
                &[],
                &["||", "(", "c/a", "c/b", "c/c", ")"],
            ),
            ("c/a foo? ( c/b )", &["foo"], &["c/a", "c/b"]),
            ("c/a foo? ( c/b )", &[], &["c/a"]),
            ("|| ( c/a foo? ( c/b ) )", &[], &["c/a"]),
            (
                "|| ( c/a foo? ( c/b ) )",
                &["foo"],
                &["||", "(", "c/a", "c/b", ")"],
            ),
            ("|| ( foo? ( c/a c/b ) )", &["foo"], &["c/a", "c/b"]),
            (
                "|| ( foo? ( c/a ) bar? ( c/b ) )",
                &[],
                &["__const__/empty-any-of"],
            ),
            ("foo? ( bar? ( c/a ) )", &["foo"], &[]),
            (
                "|| ( c/a c/b ) c/c",
                &[],
                &["||", "(", "c/a", "c/b", ")", "c/c"],
            ),
            (
                "c/c || ( c/a c/b )",
                &[],
                &["c/c", "||", "(", "c/a", "c/b", ")"],
            ),
            (
                "|| ( c/a c/b ) || ( c/c c/d )",
                &[],
                &["||", "(", "c/a", "c/b", ")", "||", "(", "c/c", "c/d", ")"],
            ),
            ("foo? ( c/a bar? ( c/b ) )", &["foo"], &["c/a"]),
            (
                "foo? ( c/a bar? ( c/b ) )",
                &["foo", "bar"],
                &["c/a", "c/b"],
            ),
            ("|| ( foo? ( c/a ) c/b )", &[], &["c/b"]),
            (
                "|| ( foo? ( c/a ) c/b )",
                &["foo"],
                &["||", "(", "c/a", "c/b", ")"],
            ),
            (
                "|| ( ( foo? ( c/a ) c/x ) c/b )",
                &[],
                &["||", "(", "c/x", "c/b", ")"],
            ),
            (
                "|| ( c/a || ( c/b c/c ) )",
                &[],
                &["||", "(", "c/a", "c/b", "c/c", ")"],
            ),
            (
                "|| ( || ( c/a c/b ) )",
                &[],
                &["||", "(", "c/a", "c/b", ")"],
            ),
            ("( || ( c/a c/b ) )", &[], &["||", "(", "c/a", "c/b", ")"]),
            ("|| ( c/a ( c/b ) )", &[], &["||", "(", "c/a", "c/b", ")"]),
            (
                "|| ( ( c/a ) ( c/b ) )",
                &[],
                &["||", "(", "c/a", "c/b", ")"],
            ),
            ("|| ( ( c/a ) c/b )", &[], &["||", "(", "c/a", "c/b", ")"]),
            (
                "foo? ( || ( c/a c/b ) )",
                &["foo"],
                &["||", "(", "c/a", "c/b", ")"],
            ),
            ("foo? ( || ( c/a c/b ) )", &[], &[]),
            (
                "|| ( foo? ( c/a c/b ) c/c )",
                &["foo"],
                &["||", "(", "(", "c/a", "c/b", ")", "c/c", ")"],
            ),
            ("|| ( foo? ( c/a c/b ) c/c )", &[], &["c/c"]),
        ];
        for (s, u, expected) in cases {
            let got =
                use_reduce_structured(&toks(s), &set(u), MatchMode::Normal).unwrap_or_else(|e| {
                    panic!("use_reduce_structured({s:?}) errored: {e}");
                });
            assert_eq!(
                got,
                expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "input {s:?} uselist {u:?}"
            );
        }
    }
}
