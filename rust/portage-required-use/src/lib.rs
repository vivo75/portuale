// Rust port of real `portage.dep.check_required_use`
// (`lib/portage/dep/__init__.py`, `is_active`/`is_satisfied` plus the
// tokenizer/group-tree builder around them) -- the REQUIRED_USE
// (PMS 7.3.4/8.2) pilot slice from PROMPT.md's follow-up work.
//
// PMS 8.2's own grammar for REQUIRED_USE (a "specification style
// variable"): a leaf is `flag` or `!flag`; a group is either a bare
// all-of (`( item+ )`, implicit AND -- REQUIRED_USE's own top level is
// itself an implicit all-of group, no wrapping parens needed), an any-of
// (`|| ( item+ )`), an exactly-one-of (`^^ ( item+ )`), an at-most-one-of
// (`?? ( item+ )` -- EAPI-gated per PMS table 8.5, `eapi >= 5` per real
// `lib/portage/eapi.py`'s own `required_use_at_most_one_of` attribute;
// always recognized here, matching this repo's EAPI 5+ profile floor and
// this pilot's established "no EAPI parametrization" precedent
// elsewhere -- see e.g. `atom-harness`'s own scope-cut comment), or a
// use-conditional (`[!]flag? ( item+ )`). No `=`/`!=`/`?=` forms exist in
// REQUIRED_USE at all (those are USE-*dep*, atom-only forms -- see
// `portage-dep`'s own `UseDepOp`; a completely different grammar).
//
// KNOWN, DOCUMENTED SIMPLIFICATIONS vs. real `check_required_use`:
//   - Real `check_required_use` builds and returns a full
//     `_RequiredUseBranch` tree (bool-coercible via `__bool__`, but also
//     independently navigable) purely so a *caller* can later extract and
//     pretty-print exactly which sub-expression failed
//     (`human_readable_required_use`, real depgraph.py's own elaborate,
//     colorized "The following REQUIRED_USE flag constraints are
//     unsatisfied" report). This pilot only ever needs the final yes/no
//     verdict (`resolve_pretend_graph` reports a violation with a short,
//     honest, pilot-specific message showing the package's own full,
//     as-declared REQUIRED_USE string -- not real portage's own
//     "reduced," violation-only sub-expression -- same "pilot-specific
//     summary, not a port of real formatting" precedent `--help` already
//     set), so this port is a much simpler direct recursive-descent
//     boolean evaluator with no tree bookkeeping at all -- verified to
//     agree with real `check_required_use` on every case via the
//     `required-use-harness`/`required_use_harness.py` pair, driven by
//     the shared `test_required_use_contract.py` suite, the same
//     wraps-the-real-thing verification pattern `use-reduce-harness`
//     already established.
//   - `empty_groups_always_true` (real `lib/portage/eapi.py`:
//     `eapi <= Eapi("6")`) is never applied: an empty group (`( )`,
//     `|| ( )`, etc -- PMS's own formal grammar actually requires "one or
//     more" items, but real `check_required_use` is more lenient than
//     its own spec here) always falls through to ordinary per-operator
//     evaluation on an empty list (`||`/`^^` unsatisfied, `??`/a
//     use-conditional trivially satisfied) -- real EAPI 7+ behavior,
//     used unconditionally here, another instance of this pilot's
//     established "no EAPI parametrization" precedent. The only real
//     divergence this causes: a literal, degenerate `|| ( )` or
//     `^^ ( )` (which no real-world ebuild has a reason to write) would
//     evaluate differently under EAPI <= 6 than it does here.
//   - A referenced flag that isn't a real, declared IUSE flag on the
//     package is an error here, exactly like real `is_active`'s own
//     `iuse_match` check (`InvalidDependString`) -- ported as
//     `Err(String)`, propagated by `portage-repo` as a fatal error for
//     the whole `--pretend` run, matching real depgraph.py's own
//     REQUIRED_USE-violation severity (see that crate's own doc comment
//     for exactly where and why).

use std::collections::HashSet;

/// Whether a REQUIRED_USE leaf token (`flag`, or `!flag` for negation --
/// real `is_active`) is satisfied against `enabled` (the package's own
/// effective USE) and `iuse` (its own declared IUSE) -- `Err` if the
/// flag (after stripping any leading `!`) isn't a real, declared IUSE
/// flag at all, exactly like real `is_active`'s own `iuse_match` check.
fn is_active(
    token: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<bool, String> {
    let (flag, negated) = match token.strip_prefix('!') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    if flag.is_empty() || !iuse.contains(flag) {
        return Err(format!("USE flag '{flag}' is not in IUSE"));
    }
    Ok(enabled.contains(flag) != negated)
}

/// Consumes tokens starting at `*pos` up to (not including) the next
/// unmatched `)` or end of input, returning each top-level item's own
/// satisfied bool, in order -- real `is_satisfied`'s own `argument` list
/// for whichever group (or the implicit top-level all-of) contains them.
fn parse_items(
    tokens: &[&str],
    pos: &mut usize,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<Vec<bool>, String> {
    let mut results = Vec::new();
    while *pos < tokens.len() && tokens[*pos] != ")" {
        let tok = tokens[*pos];
        if tok == "(" {
            // A bare all-of group (PMS 8.2's own "all-of" production) --
            // no preceding operator token.
            *pos += 1;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            results.push(!inner.contains(&false));
        } else if tok == "||" || tok == "^^" || tok == "??" {
            *pos += 1;
            expect_open_paren(tokens, pos)?;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            let true_count = inner.iter().filter(|&&b| b).count();
            let satisfied = match tok {
                "||" => true_count > 0,
                "^^" => true_count == 1,
                "??" => true_count <= 1,
                _ => unreachable!(),
            };
            results.push(satisfied);
        } else if let Some(cond) = tok.strip_suffix('?') {
            // A use-conditional group ("[!]flag? ( item+ )") -- cond is
            // "flag" or "!flag", passed to is_active exactly like a leaf
            // token would be, real is_active(op[:-1]).
            *pos += 1;
            expect_open_paren(tokens, pos)?;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            let active = is_active(cond, enabled, iuse)?;
            results.push(!active || !inner.contains(&false));
        } else {
            results.push(is_active(tok, enabled, iuse)?);
            *pos += 1;
        }
    }
    Ok(results)
}

fn expect_open_paren(tokens: &[&str], pos: &mut usize) -> Result<(), String> {
    if tokens.get(*pos) != Some(&"(") {
        return Err("malformed syntax: operator/conditional not followed by '('".to_string());
    }
    *pos += 1;
    Ok(())
}

fn expect_close_paren(tokens: &[&str], pos: &mut usize) -> Result<(), String> {
    if tokens.get(*pos) != Some(&")") {
        return Err("malformed syntax: unbalanced parentheses".to_string());
    }
    *pos += 1;
    Ok(())
}

/// Checks whether `required_use` (a package's own `REQUIRED_USE` string)
/// is satisfied by `enabled` (its own effective USE) given `iuse` (its
/// own declared IUSE) -- see the module doc comment for the full ported
/// algorithm and its documented simplifications vs. real
/// `check_required_use`. `Err` for either malformed syntax (unbalanced
/// parentheses, an operator/conditional not immediately followed by
/// `(`) or a referenced flag that isn't really declared in `iuse`.
pub fn check_required_use(
    required_use: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<bool, String> {
    let tokens: Vec<&str> = required_use.split_whitespace().collect();
    let mut pos = 0;
    let results = parse_items(&tokens, &mut pos, enabled, iuse)?;
    if pos != tokens.len() {
        return Err("malformed syntax: unbalanced parentheses".to_string());
    }
    Ok(!results.contains(&false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sets(enabled: &[&str], iuse: &[&str]) -> (HashSet<String>, HashSet<String>) {
        (
            enabled.iter().map(|s| s.to_string()).collect(),
            iuse.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn plain_flag_enabled_satisfies_itself() {
        let (enabled, iuse) = sets(&["foo"], &["foo"]);
        assert_eq!(check_required_use("foo", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn plain_flag_disabled_is_unsatisfied() {
        let (enabled, iuse) = sets(&[], &["foo"]);
        assert_eq!(check_required_use("foo", &enabled, &iuse), Ok(false));
    }

    #[test]
    fn negated_flag_disabled_satisfies_itself() {
        let (enabled, iuse) = sets(&[], &["foo"]);
        assert_eq!(check_required_use("!foo", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn undeclared_flag_is_an_error() {
        let (enabled, iuse) = sets(&[], &[]);
        assert!(check_required_use("foo", &enabled, &iuse).is_err());
    }

    #[test]
    fn any_of_needs_at_least_one() {
        let (enabled, iuse) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("|| ( a b )", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("|| ( a b )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn exactly_one_of_rejects_zero_and_two() {
        let (_, iuse) = sets(&[], &["a", "b"]);
        let (none, _) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &none, &iuse), Ok(false));
        let (both, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &both, &iuse), Ok(false));
        let (one, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &one, &iuse), Ok(true));
    }

    #[test]
    fn at_most_one_of_accepts_zero_and_one_rejects_two() {
        let (_, iuse) = sets(&[], &["a", "b"]);
        let (none, _) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &none, &iuse), Ok(true));
        let (one, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &one, &iuse), Ok(true));
        let (both, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &both, &iuse), Ok(false));
    }

    #[test]
    fn use_conditional_group_only_applies_when_the_flag_is_active() {
        let (enabled, iuse) = sets(&["foo"], &["foo", "bar"]);
        // foo active -> bar must be enabled too, but it isn't.
        assert_eq!(
            check_required_use("foo? ( bar )", &enabled, &iuse),
            Ok(false)
        );
        let (enabled, _) = sets(&[], &["foo", "bar"]);
        // foo inactive -> the whole group is trivially satisfied.
        assert_eq!(
            check_required_use("foo? ( bar )", &enabled, &iuse),
            Ok(true)
        );
    }

    #[test]
    fn negated_conditional_flag() {
        let (enabled, iuse) = sets(&[], &["foo", "bar"]);
        // !foo active (foo is disabled) -> bar must be enabled too.
        assert_eq!(
            check_required_use("!foo? ( bar )", &enabled, &iuse),
            Ok(false)
        );
    }

    #[test]
    fn bare_all_of_group_is_implicit_and() {
        let (enabled, iuse) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("( a b )", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("( a b )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn top_level_is_an_implicit_all_of_with_no_wrapping_parens_needed() {
        let (enabled, iuse) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("a b", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("a b", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn nested_groups() {
        let (enabled, iuse) = sets(&["foo", "a"], &["foo", "a", "b", "c"]);
        // foo active -> exactly one of (a, ^^(b c)) -- a is enabled, b/c
        // aren't, so the inner ^^ is unsatisfied (0 true) and the outer
        // || needs at least one true among [a=true, ^^=false] -> true.
        assert_eq!(
            check_required_use("foo? ( || ( a ^^ ( b c ) ) )", &enabled, &iuse),
            Ok(true)
        );
    }

    #[test]
    fn empty_group_uses_ordinary_operator_evaluation_not_vacuous_true() {
        // Documented simplification: no EAPI<=6 "empty groups always
        // true" special case -- see the module doc comment.
        let (enabled, iuse) = sets(&[], &[]);
        assert_eq!(check_required_use("|| ( )", &enabled, &iuse), Ok(false));
        assert_eq!(check_required_use("?? ( )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn unbalanced_parens_is_an_error() {
        let (enabled, iuse) = sets(&[], &["a"]);
        assert!(check_required_use("( a", &enabled, &iuse).is_err());
        assert!(check_required_use("a )", &enabled, &iuse).is_err());
    }

    #[test]
    fn operator_not_followed_by_open_paren_is_an_error() {
        let (enabled, iuse) = sets(&[], &["a"]);
        assert!(check_required_use("||", &enabled, &iuse).is_err());
        assert!(check_required_use("|| a", &enabled, &iuse).is_err());
        assert!(check_required_use("foo?", &enabled, &iuse).is_err());
    }
}
