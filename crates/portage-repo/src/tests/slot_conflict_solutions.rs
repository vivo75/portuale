//! The slot-conflict handler's search, on the shapes of Portage's
//! `lib/portage/tests/resolver/test_slot_collisions.py::testSlotCollision`
//! (Portage 3.0.83, run live under `ResolverPlayground` with
//! `PYTHONHASHSEED=0`).

use super::*;

fn flags(list: &[&str]) -> HashSet<String> {
    list.iter().map(|f| (*f).to_string()).collect()
}

/// A package node. `cpv` is `cat/pkg-version`; `slot` fills the match string.
fn node(cpv: &str, slot: &str, installed: bool, iuse: &[&str], on: &[&str]) -> HandlerPkg {
    HandlerPkg {
        key: PkgKey {
            cpv: cpv.to_string(),
            installed,
        },
        match_str: format!("{cpv}:{slot}/{slot}::test_repo"),
        iuse: flags(iuse),
        use_enabled: flags(on),
        required_use: String::new(),
    }
}

fn from_pkg(pkg: &HandlerPkg, atom: &str, unevaluated: &str) -> HandlerParent {
    HandlerParent {
        kind: HandlerParentKind::Package(Box::new(pkg.clone())),
        atom: atom.to_string(),
        unevaluated: unevaluated.to_string(),
    }
}

fn from_argument(atom: &str) -> HandlerParent {
    HandlerParent {
        kind: HandlerParentKind::Argument,
        atom: atom.to_string(),
        unevaluated: atom.to_string(),
    }
}

fn change(cpv: &str, list: &[(&str, bool)]) -> UseChange {
    UseChange {
        cpv: cpv.to_string(),
        flags: list.iter().map(|(f, v)| ((*f).to_string(), *v)).collect(),
    }
}

/// Case 0: `dev-libs/A B C D`, `--autounmask=n`. Backtracked 0; one
/// solution, `[{A-1: foo True, D-1: foo True}]` (`D` reaches `A` through
/// `A[foo=]`, so the parent's flag moves with it).
#[test]
fn use_dep_on_an_ebuild_and_an_equal_use_dep_parent_change_together() {
    let ebuild = node("dev-libs/A-1", "0", false, &["foo"], &[]);
    let installed = node("dev-libs/A-1", "0", true, &["foo"], &["foo"]);
    let c = node("dev-libs/C-1", "0", false, &[], &[]);
    let d = node("dev-libs/D-1", "0", false, &["foo"], &[]);
    let conflict = HandlerConflict {
        pkgs: vec![ebuild, installed],
        parents: vec![
            from_argument("dev-libs/A"),
            from_pkg(&c, "dev-libs/A[foo]", "dev-libs/A[foo]"),
            from_pkg(&d, "dev-libs/A[-foo]", "dev-libs/A[foo=]"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 1);
    assert_eq!(
        got.changes,
        vec![vec![
            change("dev-libs/A-1", &[("foo", true)]),
            change("dev-libs/D-1", &[("foo", true)]),
        ]]
    );
}

/// Case 6: `sci-libs/L sci-libs/M`. Backtracked 0; one solution
/// `[{K-1: foo False, M-1: foo False}]`. The comment at
/// `depgraph.py:11856-11862` describes this shape.
#[test]
fn ebuild_with_foo_against_installed_without_it_turns_foo_off_on_both() {
    let ebuild = node("sci-libs/K-1", "0", false, &["foo"], &["foo"]);
    let installed = node("sci-libs/K-1", "0", true, &["foo"], &[]);
    let l = node("sci-libs/L-1", "0", false, &[], &[]);
    let m = node("sci-libs/M-1", "0", false, &["foo"], &["foo"]);
    let conflict = HandlerConflict {
        pkgs: vec![ebuild, installed],
        parents: vec![
            from_pkg(&l, "sci-libs/K[-foo]", "sci-libs/K[-foo]"),
            from_pkg(&m, "sci-libs/K[foo]", "sci-libs/K[foo=]"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.conflict_count, 1);
    assert_eq!(got.solution_count, 1);
    assert_eq!(
        got.changes,
        vec![vec![
            change("sci-libs/K-1", &[("foo", false)]),
            change("sci-libs/M-1", &[("foo", false)]),
        ]]
    );
}

/// Case 7: `sci-libs/P sci-libs/Q:2`, `--update --complete-graph`. Two
/// solutions reduce to one change; the installed parent `Q-2` is changed in
/// the first solution only, so `_add_change` drops that solution
/// (`slot_collision.py:205-224`). The header still counts two solutions
/// (quirk Q22).
#[test]
fn two_solutions_reduce_to_one_change() {
    let ebuild = node("sci-libs/Q-1", "1", false, &["bar", "foo"], &["bar"]);
    let installed = node("sci-libs/Q-1", "1", true, &["bar", "foo"], &["bar", "foo"]);
    let p = node("sci-libs/P-1", "0", false, &["foo"], &[]);
    let q2 = node("sci-libs/Q-2", "2", true, &["bar", "foo"], &["bar", "foo"]);
    let conflict = HandlerConflict {
        pkgs: vec![ebuild, installed],
        parents: vec![
            from_pkg(&p, "sci-libs/Q:1[-foo]", "sci-libs/Q:1[foo=]"),
            from_pkg(&q2, "sci-libs/Q:1[bar,foo]", "sci-libs/Q:1[bar?,foo?]"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 2);
    assert_eq!(
        got.changes,
        vec![vec![
            change("sci-libs/Q-1", &[("foo", true)]),
            change("sci-libs/P-1", &[("foo", true)]),
        ]]
    );
}

/// The shape of case 6 with `L` also asking `bar?`: the first candidate
/// turns `bar` off on `K`, which is already off, so Portage rejects it
/// (quirk Q21) and the explanation suggests the spurious `+bar`.
#[test]
fn turning_off_a_flag_that_is_already_off_invalidates_the_candidate() {
    let ebuild = node("sci-libs/K-1", "0", false, &["foo", "bar"], &["foo"]);
    let installed = node("sci-libs/K-1", "0", true, &["foo", "bar"], &[]);
    let l = node("sci-libs/L-1", "0", false, &["bar"], &[]);
    let m = node("sci-libs/M-1", "0", false, &["foo"], &["foo"]);
    let conflict = HandlerConflict {
        pkgs: vec![ebuild, installed],
        parents: vec![
            from_pkg(&l, "sci-libs/K[-foo]", "sci-libs/K[-foo,bar?]"),
            from_pkg(&m, "sci-libs/K[foo]", "sci-libs/K[foo=]"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 1);
    assert_eq!(
        got.changes,
        vec![vec![
            change("sci-libs/K-1", &[("foo", false), ("bar", true)]),
            change("sci-libs/M-1", &[("foo", false)]),
        ]]
    );
}

/// Case 1: `sys-libs/A..F`. `J-2` has only `bar`, `J-1` only `foo`, and the
/// parents want both, so no configuration passes the IUSE check
/// (`slot_collision.py:848-856`): no solution, no change.
#[test]
fn flags_missing_from_iuse_leave_no_solution() {
    let j2 = node("sys-libs/J-2", "0", false, &["bar"], &["bar"]);
    let j1 = node("sys-libs/J-1", "0", false, &["foo"], &["foo"]);
    let a = node("sys-libs/A-1", "0", false, &["foo"], &["foo"]);
    let b = node("sys-libs/B-1", "0", false, &["bar"], &["bar"]);
    let conflict = HandlerConflict {
        pkgs: vec![j2, j1],
        parents: vec![
            from_pkg(&a, "sys-libs/J[foo]", "sys-libs/J[foo=]"),
            from_pkg(&b, "sys-libs/J[bar]", "sys-libs/J[bar=]"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 0);
    assert!(got.changes.is_empty());
}

/// Cases 2 and 3: `=app-misc/X-1 =app-misc/Y-1` and the `<`/`>` twin. Every
/// configuration fails the version check (`slot_collision.py:837-846`).
#[test]
fn version_conflicts_have_no_solution() {
    let x = node("app-misc/X-1", "0", false, &[], &[]);
    let y = node("app-misc/Y-1", "0", false, &[], &[]);
    let pkgs = vec![
        node("app-misc/Z-1", "0", false, &[], &[]),
        node("app-misc/Z-2", "0", false, &[], &[]),
    ];
    let eq = HandlerConflict {
        pkgs: pkgs.clone(),
        parents: vec![
            from_pkg(&y, "=app-misc/Z-1", "=app-misc/Z-1"),
            from_pkg(&x, "=app-misc/Z-2", "=app-misc/Z-2"),
        ],
    };
    let range = HandlerConflict {
        pkgs,
        parents: vec![
            from_pkg(&y, ">app-misc/Z-1", ">app-misc/Z-1"),
            from_pkg(&x, "<app-misc/Z-2", "<app-misc/Z-2"),
        ],
    };
    for conflict in [eq, range] {
        let got = solve_slot_conflicts(&[conflict]);
        assert_eq!(got.solution_count, 0);
        assert!(got.changes.is_empty());
    }
}

/// Case 4: `=app-misc/E-1 =app-misc/F-1`. One parent asks `E[foo?]`, which
/// evaluates to a plain atom, and the other pins `=E-1`: nothing to change.
#[test]
fn mixed_use_and_version_parents_have_no_solution() {
    let e = node("app-misc/E-1", "0", false, &["foo"], &[]);
    let f = node("app-misc/F-1", "0", false, &["foo"], &[]);
    let conflict = HandlerConflict {
        pkgs: vec![
            node("dev-libs/E-1", "0", false, &[], &[]),
            node("dev-libs/E-2", "0", false, &["foo"], &[]),
        ],
        parents: vec![
            from_pkg(&e, "dev-libs/E", "dev-libs/E[foo?]"),
            from_pkg(&f, "=dev-libs/E-1", "=dev-libs/E-1"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 0);
    assert!(got.changes.is_empty());
}

/// Case 5: `dev-lang/perl:0/5.12 dev-lang/perl:0/5.16 =dev-lang/perl-5.12*`.
/// The parents are arguments; no configuration satisfies all of them.
#[test]
fn argument_parents_on_two_sub_slots_have_no_solution() {
    let conflict = HandlerConflict {
        pkgs: vec![
            node("dev-lang/perl-5.12", "0", false, &[], &[]),
            node("dev-lang/perl-5.16", "0", false, &[], &[]),
        ],
        parents: vec![
            from_argument("dev-lang/perl:0/0"),
            from_argument("=dev-lang/perl-5.12*"),
        ],
    };
    let got = solve_slot_conflicts(&[conflict]);
    assert_eq!(got.solution_count, 0);
}

fn solvable_conflict(n: usize) -> HandlerConflict {
    let cpv = format!("dev-libs/S{n}-1");
    HandlerConflict {
        pkgs: vec![
            node(&cpv, "0", false, &["foo"], &[]),
            node(&cpv, "0", true, &["foo"], &["foo"]),
        ],
        parents: vec![from_argument(&format!("dev-libs/S{n}[foo]"))],
    }
}

fn version_pinned_conflict() -> HandlerConflict {
    HandlerConflict {
        pkgs: vec![
            node("dev-libs/V-2", "0", false, &[], &[]),
            node("dev-libs/V-1", "0", true, &[], &[]),
        ],
        parents: vec![from_argument("=dev-libs/V-1")],
    }
}

/// The all-ebuild configuration is tried first; with four conflicts or
/// fewer the search goes on to the next configuration
/// (`slot_collision.py:170-179`).
#[test]
fn search_goes_past_the_all_ebuild_configuration_with_few_conflicts() {
    let got = solve_slot_conflicts(&[solvable_conflict(1), version_pinned_conflict()]);
    assert_eq!(got.solution_count, 1);
    assert_eq!(
        got.changes,
        vec![vec![change("dev-libs/S1-1", &[("foo", true)])]]
    );
}

/// With more than four slot conflicts only the all-ebuild configuration is
/// checked (`len(conflict_pkgs) > 4`, one entry per slot conflict).
#[test]
fn search_stops_after_the_first_configuration_with_more_than_four_conflicts() {
    let conflicts: Vec<HandlerConflict> = (1..=4)
        .map(solvable_conflict)
        .chain(std::iter::once(version_pinned_conflict()))
        .collect();
    let got = solve_slot_conflicts(&conflicts);
    assert_eq!(got.conflict_count, 5);
    assert_eq!(got.solution_count, 0);
    assert!(got.changes.is_empty());
}

/// A change that is a subset of another makes the larger one redundant
/// (`slot_collision.py:187-224`); a change that is already covered is ignored.
#[test]
fn add_change_keeps_only_minimal_changes() {
    let small = vec![change("a/b-1", &[("foo", true)])];
    let big = vec![change("a/b-1", &[("foo", true), ("bar", false)])];
    let mut changes = vec![big.clone()];
    add_change(&mut changes, small.clone());
    assert_eq!(changes, vec![small.clone()]);
    add_change(&mut changes, big);
    assert_eq!(changes, vec![small]);
}

/// Configurations are enumerated with the last conflict varying fastest
/// (`_configuration_generator._next`).
#[test]
fn configurations_are_enumerated_last_conflict_first() {
    let a = node("a/x-1", "0", false, &[], &[]);
    let b = node("a/x-2", "0", false, &[], &[]);
    let ordered = vec![vec![&a, &b], vec![&a, &b]];
    let mut ids = vec![0, 0];
    let mut seen = vec![ids.clone()];
    while next_configuration(&mut ids, &ordered) {
        seen.push(ids.clone());
    }
    assert_eq!(seen, vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]]);
}
