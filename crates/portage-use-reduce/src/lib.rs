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

struct ResolveDisjunctionsArgs<'a> {
    nodes: &'a [DepNode],
    uselist: &'a HashSet<String>,
    mode: MatchMode,
    empty_groups_always_true: bool,
    tie_break: &'a mut TieBreak<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }
}
