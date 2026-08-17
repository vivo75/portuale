// Rust port of `portage.dep.use_reduce` (lib/portage/dep/__init__.py),
// restricted to `flat=True` mode -- the depgraph/config-resolution slice
// after atom matching (see PORTING/PROMPT.md).
//
// Unlike atom.rs, this is NOT a narrowed grammar: flat mode's tokenizer
// and bracket/use-conditional handling (the part of use_reduce this ports)
// is fully self-contained in the real implementation and is ported as-is,
// error behavior included. What's out of scope is a set of *optional
// parameters* the harness simply never exercises: `masklist`, `excludeall`
// (rare flag-override sets), `is_src_uri`/the "->" SRC_URI arrow token
// (fetch-restriction syntax), `opconvert` and non-flat structured output
// (`flat=False`'s nested-list bracket-optimization logic is considerably
// more involved and not needed for a flat token list), and
// `token_class`/`is_valid_flag` (tokens stay opaque strings here, matching
// how config.py itself calls use_reduce for RESTRICT/PROPERTIES/IUSE-like
// values -- see the grep in PORTING/README.md).
//
// `subset` (the `--with-test-deps` follow-up) IS now ported too --
// `use_reduce_flat_subset`, grounded against real `select_subset`, which
// needs the *full* nested `paren_reduce` structure (not `flat=True`'s
// own eagerly-flattened one) to know where a `"||"` group's own
// boundaries are before subset-filtering happens, so it gets its own
// tree type (`DepNode`) and its own build/filter/reserialize pipeline
// feeding into the *unmodified* `use_reduce_flat` below for the actual
// final flattening -- see `use_reduce_flat_subset`'s own doc comment.
//
// `flat=True` is a real, heavily-used invocation mode (not a convenience
// fiction): lib/portage/package/ebuild/config.py, _emerge/resolver/, and
// _emirrordist all call use_reduce(..., flat=True) for exactly this "give
// me a flat token list" need.
//
// `use_reduce_flat_disjunctive` (the real "||"-group resolution follow-up)
// reuses the exact same DepNode/build_dep_tree/serialize_dep_tree
// machinery `use_reduce_flat_subset` already needed, extended with a new
// `resolve_disjunctions` walk: picks the first alternative of every "||"
// group a caller-supplied satisfiability closure accepts, instead of
// flattening every alternative the way `use_reduce_flat` alone always
// has -- see its own doc comment for the full grounding (real
// `_add_pkg_dep_string`'s own considerably richer preference order isn't
// ported; this pilot has no backtracking architecture at all). This
// crate stays atom-agnostic throughout, matching its own established
// "tokens stay opaque strings" architecture -- portage-repo supplies the
// actual visibility-checking closure.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Conditionals are active iff the flag is in `uselist` (negated for `!flag?`).
    Normal,
    /// Every conditional is active, regardless of `uselist` (real use_reduce's `matchall=True`).
    All,
    /// Every conditional is inactive, regardless of `uselist` (real use_reduce's `matchnone=True`).
    None,
}

fn useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}

/// Whether a single `flag?`/`!flag?` USE-conditional token is active
/// under `uselist`/`mode`. Exported (not just used internally by
/// `use_reduce_flat`'s own bracket handling) so `portage-repo`'s own
/// bespoke LICENSE-grammar parser can reuse the exact same conditional-
/// resolution logic -- LICENSE (PMS 7.3.2) needs the real `||`-group
/// structure `use_reduce_flat`'s own flattening deliberately discards
/// (see that crate's own doc comment for why it needed a separate
/// parser, not a reused one), but its USE-conditional semantics are
/// identical to every other dependency-string-shaped value, so this one
/// piece is shared rather than reimplemented a second time.
pub fn is_active(
    conditional: &str,
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<bool, String> {
    let (flag, negated) = match conditional.strip_prefix('!') {
        Some(rest) => (&rest[..rest.len() - 1], true),
        None => (&conditional[..conditional.len() - 1], false),
    };
    if !useflag_re().is_match(flag) {
        return Err(format!(
            "invalid use flag '{flag}' in conditional '{conditional}'"
        ));
    }
    Ok(match mode {
        MatchMode::All => true,
        MatchMode::None => false,
        MatchMode::Normal => {
            (uselist.contains(flag) && !negated) || (!uselist.contains(flag) && negated)
        }
    })
}

/// `tokens` is the dep string pre-split on whitespace (equivalent to
/// Python's `depstr.split()`, which is the real function's own first
/// step). Returns the flattened token list, or an error message mirroring
/// `InvalidDependString`.
pub fn use_reduce_flat(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<Vec<String>, String> {
    let mut stack: Vec<Vec<String>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(format!(
                        "expected: dependency string, got: ')', token {}",
                        pos + 2
                    ));
                }
                // "(" always satisfies a pending "||"/"flag?" requirement,
                // regardless of need_bracket's prior state.
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: ')', token {}", pos + 1));
                }
                if stack.len() <= 1 {
                    return Err(format!("no matching '(' for ')', token {}", pos + 1));
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
                    return Err(format!("expected: '(', got: '||', token {}", pos + 1));
                }
                need_bracket = true;
                stack.last_mut().unwrap().push("||".to_string());
            }
            "->" => {
                // is_src_uri is always false for this harness (SRC_URI
                // arrows are out of v1 scope), matching real use_reduce's
                // behavior when is_src_uri=False.
                return Err(format!(
                    "SRC_URI arrow are only allowed in SRC_URI: token {}",
                    pos + 1
                ));
            }
            _ => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '{token}', token {}", pos + 1));
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(token.clone());
            }
        }
    }

    if stack.len() != 1 {
        return Err("Missing ')' at end of string".to_string());
    }
    if need_bracket {
        return Err("Missing '(' at end of string".to_string());
    }

    Ok(stack.pop().unwrap())
}

/// A single node of the nested-list shape real `paren_reduce` builds
/// (`['foobar', 'foo?', ['bar', 'baz']]` -- a flag/`"||"` marker stays a
/// sibling *string* immediately before its own nested group, never
/// merged into it at parse time). Unlike real `paren_reduce`, this
/// builder does none of its redundant-bracket-collapsing optimizations
/// (`is_single`/`special_append`/etc.) -- those only ever change how
/// *minimally* the tree is represented, never its semantics, and
/// `select_subset` below (the only consumer) doesn't care either way.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DepNode {
    Str(String),
    Group(Vec<DepNode>),
}

/// Builds the nested `DepNode` tree from `tokens` -- same bracket/`"||"`/
/// SRC_URI-arrow validation as `use_reduce_flat`'s own tokenizer (same
/// error messages too), just building a tree instead of eagerly
/// flattening, so a `subset` filter (`select_subset` below) has real
/// group boundaries to walk before flattening happens at all.
fn build_dep_tree(tokens: &[String]) -> Result<Vec<DepNode>, String> {
    let mut stack: Vec<Vec<DepNode>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(format!(
                        "expected: dependency string, got: ')', token {}",
                        pos + 2
                    ));
                }
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: ')', token {}", pos + 1));
                }
                if stack.len() <= 1 {
                    return Err(format!("no matching '(' for ')', token {}", pos + 1));
                }
                let l = stack.pop().unwrap();
                stack.last_mut().unwrap().push(DepNode::Group(l));
            }
            "||" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '||', token {}", pos + 1));
                }
                need_bracket = true;
                stack
                    .last_mut()
                    .unwrap()
                    .push(DepNode::Str("||".to_string()));
            }
            "->" => {
                return Err(format!(
                    "SRC_URI arrow are only allowed in SRC_URI: token {}",
                    pos + 1
                ));
            }
            _ => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '{token}', token {}", pos + 1));
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(DepNode::Str(token.clone()));
            }
        }
    }

    if stack.len() != 1 {
        return Err("Missing ')' at end of string".to_string());
    }
    if need_bracket {
        return Err("Missing '(' at end of string".to_string());
    }

    Ok(stack.pop().unwrap())
}

/// Real `use_reduce`'s own `select_subset` (invoked whenever `subset` is
/// given): walks the parsed tree keeping only atoms reachable through a
/// conditional whose own flag is in `subset` -- `disjunction` is true
/// while directly inside a `"||"` group's own member list (each member
/// becomes its own list in the result rather than being spliced flat,
/// mirroring real portage's own "one result entry per alternative"
/// shape), `selected` is true once already inside a subset-matching
/// conditional (inherited into every descendant, so a plain, non-`?`
/// atom nested two levels under `test?` still gets kept). Every OTHER
/// conditional (not in `subset`) is still evaluated normally via
/// `is_active` -- `subset` only decides which already-active branch's
/// atoms make it into the *output*, never which branches are active in
/// the first place.
fn select_subset(
    nodes: &[DepNode],
    disjunction: bool,
    selected: bool,
    subset: &HashSet<String>,
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<Vec<DepNode>, String> {
    let mut result: Vec<DepNode> = Vec::new();
    let mut iter = nodes.iter();
    while let Some(node) = iter.next() {
        match node {
            DepNode::Group(children) => {
                // A bare "( ... )" grouping with no preceding flag?/"||"
                // marker at all -- selection state just passes through
                // unchanged, same as real select_subset's own
                // AttributeError ("token has no .endswith") branch.
                let sub = select_subset(children, false, selected, subset, uselist, mode)?;
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
                    return Err(format!("conditional '{s}' not followed by a group"));
                };
                if is_active(s, uselist, mode)? {
                    let flag = &s[..s.len() - 1];
                    let now_selected = selected || subset.contains(flag);
                    let sub = select_subset(children, false, now_selected, subset, uselist, mode)?;
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
                    return Err("'||' not followed by a group".to_string());
                };
                let sub = select_subset(children, true, selected, subset, uselist, mode)?;
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

/// `paren_enclose` equivalent: re-serializes a `DepNode` tree back into
/// a flat token stream (`Group` becomes a literal `"("`/`")"` pair
/// around its own contents), so the already subset-filtered result can
/// be fed straight into the ordinary (unmodified) `use_reduce_flat`
/// bracket/conditional handling below for its own final flattening.
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

/// Real `use_reduce`'s own `subset` parameter (`lib/portage/dep/
/// __init__.py`): real portage handles this as a genuine two-pass
/// operation even when `flat=True` is also given -- `subset` filtering
/// (`select_subset`, over the full nested `paren_reduce` structure) runs
/// *first*, producing an already-filtered dependency string, which is
/// *then* reduced normally (`flat=True`'s own bracket/conditional
/// handling). This mirrors that exactly: `build_dep_tree` (this crate's
/// own `paren_reduce` equivalent) -> `select_subset` -> `serialize_dep_tree`
/// (this crate's own `paren_enclose` equivalent) -> the ordinary
/// `use_reduce_flat`, completely unmodified. Grounded against real
/// portage's own `--with-test-deps` call site in `depgraph.py`:
/// `use_reduce(dep_string, uselist=use_enabled | {"test"}, ...,
/// subset={"test"})` extracts exactly the *additional* deps a `test?`
/// conditional (anywhere in the string, at any nesting depth)
/// contributes once `"test"` is forced on, discarding every
/// unconditional dep and every dep gated only by some *other*
/// conditional -- see `select_subset`'s own doc comment for the exact
/// walk.
pub fn use_reduce_flat_subset(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
    subset: &HashSet<String>,
) -> Result<Vec<String>, String> {
    let tree = build_dep_tree(tokens)?;
    let filtered = select_subset(&tree, false, false, subset, uselist, mode)?;
    let mut reserialized = Vec::new();
    serialize_dep_tree(&filtered, &mut reserialized);
    use_reduce_flat(&reserialized, uselist, mode)
}

/// One "alternative" inside a `"||"` group -- a single atom, a bracketed
/// multi-atom group, or a conditional (`flag?`) itself, each consumed as
/// one logical unit even though a conditional spans two sibling
/// `DepNode`s (the `flag?` marker plus its own `Group`) -- real
/// portage's own dependency-specification grammar allows a bare
/// conditional directly as a `"||"` alternative, not just atoms/groups.
fn next_alternative<'a>(
    iter: &mut std::slice::Iter<'a, DepNode>,
) -> Option<Result<Vec<DepNode>, String>> {
    let node = iter.next()?;
    Some(match node {
        DepNode::Str(s) if s.ends_with('?') => match iter.next() {
            Some(group @ DepNode::Group(_)) => Ok(vec![node.clone(), group.clone()]),
            _ => Err(format!("conditional '{s}' not followed by a group")),
        },
        other => Ok(vec![other.clone()]),
    })
}

/// Real `_add_pkg_dep_string`'s own `"||"` resolution, considerably
/// simplified: picks the first alternative every one of whose own atoms
/// `alternative_satisfiable` accepts (a caller-supplied probe -- this
/// crate stays atom-agnostic, matching its own existing "tokens stay
/// opaque strings" architecture, see the module doc comment), instead
/// of flattening every alternative into the result the way plain
/// `use_reduce_flat` always has. An alternative that resolves to zero
/// atoms at all (every token inside it gated by an inactive conditional)
/// counts as trivially satisfiable -- `alternative_satisfiable` is
/// expected to return `true` for an empty slice, the same vacuous-truth
/// real portage itself gives a no-cost alternative.
///
/// Falls back to keeping the *whole* `"||"` group exactly as
/// `use_reduce_flat` would have flattened it (literal `"||"` marker,
/// every alternative's own atoms, no selection at all) whenever *no*
/// alternative is currently satisfiable -- so a dependency this pilot
/// can't currently resolve is never silently dropped, preserving the
/// exact "never silently wrong about whether a dependency exists"
/// invariant `resolve_pretend_graph`'s own doc comment (portage-repo)
/// already established for the unconditional-flatten v1 this replaces.
/// Real portage's own considerably richer preference order (installed
/// packages first, backtracking on a later constraint failure, etc.)
/// isn't ported -- this pilot has no backtracking architecture at all --
/// just the single "first currently-resolvable alternative wins" rule.
pub fn use_reduce_flat_disjunctive(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
    alternative_satisfiable: &mut impl FnMut(&[String]) -> bool,
) -> Result<Vec<String>, String> {
    let tree = build_dep_tree(tokens)?;
    let resolved = resolve_disjunctions(&tree, uselist, mode, alternative_satisfiable)?;
    let mut reserialized = Vec::new();
    serialize_dep_tree(&resolved, &mut reserialized);
    use_reduce_flat(&reserialized, uselist, mode)
}

fn resolve_disjunctions(
    nodes: &[DepNode],
    uselist: &HashSet<String>,
    mode: MatchMode,
    alternative_satisfiable: &mut impl FnMut(&[String]) -> bool,
) -> Result<Vec<DepNode>, String> {
    let mut result: Vec<DepNode> = Vec::new();
    let mut iter = nodes.iter();
    while let Some(node) = iter.next() {
        match node {
            DepNode::Group(children) => {
                let resolved =
                    resolve_disjunctions(children, uselist, mode, alternative_satisfiable)?;
                result.push(DepNode::Group(resolved));
            }
            DepNode::Str(s) if s.ends_with('?') => {
                let Some(DepNode::Group(children)) = iter.next() else {
                    return Err(format!("conditional '{s}' not followed by a group"));
                };
                let resolved =
                    resolve_disjunctions(children, uselist, mode, alternative_satisfiable)?;
                result.push(DepNode::Str(s.clone()));
                result.push(DepNode::Group(resolved));
            }
            DepNode::Str(s) if s == "||" => {
                let Some(DepNode::Group(alternatives)) = iter.next() else {
                    return Err("'||' not followed by a group".to_string());
                };
                let mut chosen: Option<Vec<DepNode>> = None;
                let mut alt_iter = alternatives.iter();
                while let Some(alt) = next_alternative(&mut alt_iter) {
                    let alt_nodes = alt?;
                    let mut flat = Vec::new();
                    serialize_dep_tree(&alt_nodes, &mut flat);
                    let Ok(flat_atoms) = use_reduce_flat(&flat, uselist, mode) else {
                        continue;
                    };
                    if alternative_satisfiable(&flat_atoms) {
                        chosen = Some(resolve_disjunctions(
                            &alt_nodes,
                            uselist,
                            mode,
                            alternative_satisfiable,
                        )?);
                        break;
                    }
                }
                match chosen {
                    Some(alt_nodes) => result.extend(alt_nodes),
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
        // test? ( bar? ( dev-libs/a ) ) -- "bar" isn't in the subset, so
        // it's still evaluated normally (is_active) rather than being
        // treated as always-selected; only actually-active branches
        // contribute, exactly like without subset filtering at all.
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
        // "!test?" never matches "test" in the subset (real portage's
        // own `token[:-1] in subset` check keeps the leading "!"), so
        // this stays excluded even though "!test?" is itself active
        // (test is NOT in uselist here).
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
        // || ( foo? ( dev-libs/a ) test? ( dev-libs/b ) ) -- "foo" is
        // off, so only the test-gated alternative survives; the "||"
        // marker itself is preserved (matching use_reduce_flat's own
        // existing flat convention) since something inside it did.
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
        // "b" is the only satisfiable alternative -- "a" and "c" must
        // both be dropped entirely, not just deprioritized.
        let result = use_reduce_flat_disjunctive(
            &toks("|| ( dev-libs/a dev-libs/b dev-libs/c )"),
            &HashSet::new(),
            MatchMode::Normal,
            &mut |atoms| atoms == ["dev-libs/b"],
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/b"]);
    }

    #[test]
    fn disjunctive_falls_back_to_every_alternative_when_none_satisfiable() {
        // Nothing is satisfiable -- matches plain use_reduce_flat's own
        // "flatten everything" output exactly, so nothing regresses
        // when this pilot can't currently resolve any alternative.
        let result = use_reduce_flat_disjunctive(
            &toks("|| ( dev-libs/a dev-libs/b )"),
            &HashSet::new(),
            MatchMode::Normal,
            &mut |_| false,
        )
        .unwrap();
        assert_eq!(result, vec!["||", "dev-libs/a", "dev-libs/b"]);
    }

    #[test]
    fn disjunctive_treats_a_bracketed_multi_atom_alternative_as_one_unit() {
        // || ( ( dev-libs/a dev-libs/b ) dev-libs/c ) -- the first
        // alternative is the PAIR (a AND b together); it's only chosen
        // if BOTH are satisfiable at once, not either one alone.
        let result = use_reduce_flat_disjunctive(
            &toks("|| ( ( dev-libs/a dev-libs/b ) dev-libs/c )"),
            &HashSet::new(),
            MatchMode::Normal,
            &mut |atoms| atoms == ["dev-libs/a", "dev-libs/b"],
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a", "dev-libs/b"]);
    }

    #[test]
    fn disjunctive_skips_the_bracketed_pair_when_only_one_half_is_satisfiable() {
        // Same shape as above, but only "dev-libs/a" alone is
        // satisfiable -- the bracketed pair needs BOTH, so it's
        // rejected and the bare "dev-libs/c" alternative wins instead.
        let result = use_reduce_flat_disjunctive(
            &toks("|| ( ( dev-libs/a dev-libs/b ) dev-libs/c )"),
            &HashSet::new(),
            MatchMode::Normal,
            &mut |atoms| atoms == ["dev-libs/c"],
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/c"]);
    }

    #[test]
    fn disjunctive_treats_an_inactive_conditional_alternative_as_vacuously_satisfiable() {
        // || ( foo? ( dev-libs/a ) dev-libs/b ) with "foo" off -- the
        // first alternative flattens to nothing at all (a real,
        // legitimate "requires nothing" alternative), which must win
        // immediately rather than falling through to "dev-libs/b".
        let result = use_reduce_flat_disjunctive(
            &toks("|| ( foo? ( dev-libs/a ) dev-libs/b )"),
            &HashSet::new(),
            MatchMode::Normal,
            &mut |atoms: &[String]| atoms.is_empty(),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn disjunctive_leaves_non_disjunctive_deps_untouched() {
        let result = use_reduce_flat_disjunctive(
            &toks("dev-libs/a foo? ( dev-libs/b )"),
            &set(&["foo"]),
            MatchMode::Normal,
            &mut |_| false,
        )
        .unwrap();
        assert_eq!(result, vec!["dev-libs/a", "dev-libs/b"]);
    }
}
