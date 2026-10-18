//! #369 S5: the deferred `||` stack (`_dep_disjunctive_stack`).
//!
//! Splitter unit tests pin the expansion-time contract; the walk tests pin
//! Portage 3.0.82.2's rows from the container captures (never portuale's
//! output): `LLM/evidence/2026-10-15-plan-359/portage/369-rb-pv.txt`
//! (`y b a rb`) and `369-ra-pv.txt` (`y a b ra`).

use super::*;
use std::collections::HashSet;

fn toks(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn uselist(flags: &[&str]) -> HashSet<String> {
    flags.iter().map(|s| s.to_string()).collect()
}

#[test]
fn split_leaves_group_free_strings_untouched() {
    // No `||`: nothing defers, so the walk stays byte-identical.
    let (plains, groups) = split_off_disjunctions(&toks("dev-libs/a dev-libs/b"), &uselist(&[]));
    assert_eq!(plains, toks("dev-libs/a dev-libs/b"));
    assert!(groups.is_empty());
}

#[test]
fn split_extracts_one_group_bare() {
    let (plains, groups) = split_off_disjunctions(
        &toks("dev-libs/c || ( dev-libs/a dev-libs/b ) dev-libs/d"),
        &uselist(&[]),
    );
    assert_eq!(plains, toks("dev-libs/c dev-libs/d"));
    assert_eq!(groups, vec![toks("|| ( dev-libs/a dev-libs/b )")]);
}

#[test]
fn split_extracts_a_group_nested_in_plain_parens() {
    // Portage recurses `_queue_disjunctive_deps` into non-`||` lists, so a
    // group inside plain parens defers too, leaving no empty wrapper (which
    // would fail the downstream flattener).
    let (plains, groups) = split_off_disjunctions(
        &toks("dev-libs/c ( || ( dev-libs/a dev-libs/b ) )"),
        &uselist(&[]),
    );
    assert_eq!(plains, toks("dev-libs/c"));
    assert_eq!(groups, vec![toks("|| ( dev-libs/a dev-libs/b )")]);
}

#[test]
fn split_applies_conditionals_before_extracting() {
    // Portage runs `use_reduce` before queueing disjunctions: an active
    // conditional is entered (its group defers bare), an inactive one drops
    // the group with it.
    let (plains, groups) = split_off_disjunctions(
        &toks("foo? ( || ( dev-libs/a dev-libs/b ) ) dev-libs/c"),
        &uselist(&["foo"]),
    );
    assert_eq!(plains, toks("dev-libs/c"));
    assert_eq!(groups, vec![toks("|| ( dev-libs/a dev-libs/b )")]);

    let (plains, groups) = split_off_disjunctions(
        &toks("foo? ( || ( dev-libs/a dev-libs/b ) ) dev-libs/c"),
        &uselist(&[]),
    );
    assert_eq!(plains, toks("dev-libs/c"));
    assert!(groups.is_empty());

    // Negated conditionals evaluate the same way.
    let (_, groups) = split_off_disjunctions(
        &toks("!foo? ( || ( dev-libs/a dev-libs/b ) )"),
        &uselist(&[]),
    );
    assert_eq!(groups.len(), 1);
}

#[test]
fn split_drops_an_active_conditional_emptied_by_its_contents() {
    // `foo? ( !bar? ( y ) )` with foo and bar on: the inner conditional is
    // inactive, so nothing of the group survives — and no `foo? ( )`
    // wrapper is emitted (an empty group would fail the downstream
    // flattener with `expected: dependency string`, dropping the owner's
    // whole dep string; seen live on `=dev-libs/cyc0w-3` pass 2, where
    // `dev-libs/cyc0z` expands with foo and bar on).
    let (plains, groups) = split_off_disjunctions(
        &toks("foo? ( !bar? ( dev-libs/y ) ) dev-libs/c"),
        &uselist(&["foo", "bar"]),
    );
    assert_eq!(plains, toks("dev-libs/c"));
    assert!(groups.is_empty());
}

#[test]
fn split_returns_malformed_strings_unsplit() {
    // `||` without a group (and unbalanced parens) bail out, so the
    // downstream flattener reports exactly what it would without deferral.
    for bad in ["|| dev-libs/a", "dev-libs/a )", "|| ( dev-libs/a"] {
        let tokens = toks(bad);
        let (plains, groups) = split_off_disjunctions(&tokens, &uselist(&[]));
        assert_eq!(plains, tokens, "{bad}");
        assert!(groups.is_empty(), "{bad}");
    }
}

fn resolve_names(atom_str: &str) -> Vec<String> {
    let root = fixtures_root();
    ResolveRequest {
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| format!("{}/{}", e.category, e.package))
    .collect()
}

#[test]
fn deferred_disjunction_resolves_rb_to_the_later_queued_alternative() {
    // Portage 3.0.82.2 (`369-rb-pv.txt`): `y b a rb` — `a`'s `|| ( x y )`
    // resolves only after the plain queue drains, by which time `b`'s plain
    // `y` is in the graph. Choosing at expand time plans `x` beside `y`.
    let mut got = resolve_names("dev-libs/pg366-rb");
    got.sort();
    assert_eq!(
        got,
        vec![
            "dev-libs/pg366-a",
            "dev-libs/pg366-b",
            "dev-libs/pg366-rb",
            "dev-libs/pg366-y",
        ]
    );
}

#[test]
fn deferred_disjunction_keeps_ra_on_the_in_graph_alternative() {
    // Control (`369-ra-pv.txt`): `y a b ra` — agreed before deferral, must
    // not move.
    let mut got = resolve_names("dev-libs/pg366-ra");
    got.sort();
    assert_eq!(
        got,
        vec![
            "dev-libs/pg366-a",
            "dev-libs/pg366-b",
            "dev-libs/pg366-ra",
            "dev-libs/pg366-y",
        ]
    );
}
