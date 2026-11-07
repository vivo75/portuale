//! A USE-only miss Portage meets first stops the walk with no restart, and a
//! pass that recorded an autounmask change before its missing-dependency stop
//! ends the search (#594). Expected values are host Portage 3.0.83 captures
//! in `LLM/evidence/2026-10-26-575-residues/r3/captures/`, cited per test.

use super::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_in};
use super::*;

fn merged(result: &GraphResult) -> Vec<String> {
    result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect()
}

/// The command line's default: `--autounmask` suggests USE changes.
pub(super) fn request_use_in(root: &Path, atoms: &[&str]) -> ResolveRequest {
    let mut req = request_in(root, atoms);
    req.autounmask_suggest_use = true;
    req
}

fn aborted(result: &GraphResult) -> bool {
    matches!(result.outcome, ResolveOutcome::Aborted { .. })
}

#[test]
fn use_only_miss_first_in_string_stops_with_no_restart() {
    // `A-e.txt`: rc 1, `(backtrack: 0/20)`, no merge list. `r594xdep[r594flag]`
    // has no fixable USE change (the visible `-1` lacks the IUSE), so Portage
    // stops there (D:3494-3499) and never files `maskeddep`.
    let root = hermetic_root(&[]);
    let r = request_use_in(&root, &["dev-libs/r594etop"])
        .resolve()
        .expect("the library reports the miss as an entry");
    assert_eq!(r.backtrack_restarts, 0, "{:?}", r.outcome);
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
    assert!(
        r.entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate)),
        "the miss stays an entry: {:?}",
        merged(&r)
    );
}

#[test]
fn use_only_miss_first_blocks_the_trigger_under_autounmask_n() {
    // `A-c-aun.txt`: rc 1, `(backtrack: 0/20)`. With `--autounmask=n` the
    // `r594vdep[r594flag]` change is not takable, so the USE-only miss comes
    // first in Portage's order (D:4784, both atoms package-less) and no
    // missing-dependency restart is filed.
    let root = hermetic_root(&[]);
    let r = resolve_in(&root, &["dev-libs/r594ctop"]);
    assert_eq!(r.backtrack_restarts, 0, "{:?}", r.outcome);
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
}

#[test]
fn masked_dep_first_in_string_still_backtracks() {
    // `A-c.txt`: rc 0, `(backtrack: 1/20)`, `r594cm-1` and `r594ctop-1`.
    // `maskeddep` has no package, so Portage files it before the fixable
    // USE miss is ever recorded (D:4784, D:3517).
    let root = hermetic_root(&[]);
    let r = request_use_in(&root, &["dev-libs/r594ctop"])
        .resolve()
        .expect("resolves");
    assert_eq!(r.backtrack_restarts, 1, "{:?}", r.outcome);
    assert_eq!(merged(&r), ["dev-libs/r594cm", "dev-libs/r594ctop"]);
}

#[test]
fn autounmask_change_before_the_stop_ends_the_search() {
    // `A-q.txt`: rc 1, `(backtrack: 0/20)`, the USE-change block,
    // "backtracking has terminated early", then the masked block. The pass
    // recorded the `r594vdep` USE change before `maskeddep` filed, so
    // `need_config_change` breaks first (D:11865-11883, D:12407-12411).
    let root = hermetic_root(&[]);
    let r = request_use_in(&root, &["dev-libs/r594qtop"])
        .resolve()
        .expect("the library reports the miss as an entry");
    assert_eq!(r.backtrack_restarts, 0, "{:?}", r.outcome);
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
}

#[test]
fn autounmask_change_before_the_stop_ends_the_search_within_backtrack_1() {
    // `A-q-bt1.txt`: rc 1, `(backtrack: 0/1)`.
    let root = hermetic_root(&[]);
    let mut req = request_use_in(&root, &["dev-libs/r594qtop"]);
    req.backtrack_max = 1;
    let r = req.resolve().expect("the library reports the miss");
    assert_eq!(r.backtrack_restarts, 0, "{:?}", r.outcome);
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
}

#[test]
fn autounmask_backtrack_y_keeps_backtracking() {
    // `A-q-aby.txt`: rc 1, `(backtrack: 1/20)`. The control: with
    // `--autounmask-backtrack=y` Portage backtracks past the change.
    let root = hermetic_root(&[]);
    let mut req = request_use_in(&root, &["dev-libs/r594qtop"]);
    req.config.autounmask_backtrack = true;
    let r = req.resolve().expect("resolves");
    assert_eq!(r.backtrack_restarts, 1, "{:?}", r.outcome);
    // The masked block Portage also prints on this path stays absent (a
    // pre-existing report gap, not R3's): pin only the 1/20 and the rows.
    assert_eq!(
        merged(&r),
        [
            "dev-libs/r594qb",
            "dev-libs/r594vdep",
            "dev-libs/r594qa",
            "dev-libs/r594qtop"
        ]
    );
}

#[test]
fn masked_only_shapes_backtrack_once() {
    // `A-p.txt`, `A-k.txt`, `A-g.txt`: rc 0, `(backtrack: 1/20)`. Keepers:
    // package.mask, `~amd64` and absent deps file with no USE-only miss
    // ahead of them.
    let root = hermetic_root(&[]);
    for atom in [
        "dev-libs/r594ptop",
        "dev-libs/r594ktop",
        "dev-libs/r594gtop",
    ] {
        let r = request_use_in(&root, &[atom]).resolve().expect("resolves");
        assert_eq!(r.backtrack_restarts, 1, "{atom}: {:?}", r.outcome);
    }
}

/// The `(node, type)` chain of the one report kept, innermost first.
fn chain_nodes(chain: &[(String, String)]) -> Vec<&str> {
    chain.iter().map(|(n, _)| n.as_str()).collect()
}

#[test]
fn backtrack_0_reports_only_the_first_failure_and_no_later_conflict() {
    // Backlog #609, Portage 3.0.83 `emerge -pv --backtrack=0
    // dev-libs/r592atop` (`LLM/evidence/2026-11-06-sweep/609/`): rc 1,
    // `(backtrack: 0/0)`, only the `r592agone` miss under `r592ay-2`. The
    // LIFO walk pops `ay-2` first and stops, so `ax-2` never pulls `as-1`.
    let root = hermetic_root(&[]);
    let mut req = request_in(&root, &["dev-libs/r592atop"]);
    req.backtrack_max = 0;
    let r = req.resolve().expect("the library reports the miss");
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
    assert!(r.slot_conflicts.is_empty(), "{:?}", r.slot_conflicts);
    assert_eq!(r.plain_miss_deps.len(), 1, "{:?}", r.plain_miss_deps);
    assert_eq!(r.plain_miss_deps[0].atom, "dev-libs/r592agone");
    assert_eq!(
        chain_nodes(&r.plain_miss_deps[0].chain),
        [
            "dev-libs/r592ay-2::testrepo",
            "dev-libs/r592atop-1::testrepo",
            "dev-libs/r592atop"
        ]
    );
}

#[test]
fn a_use_stop_drops_the_later_masked_dependency_and_its_restart() {
    // Backlog #609, Portage 3.0.83 `emerge -pv --autounmask=n
    // dev-libs/r594qtop`: rc 1, `(backtrack: 0/20)`, only the `r594vdep`
    // USE block. `r594qa` (listed last) is popped first and stops the walk,
    // so `maskeddep` under `r594qb` is neither reported nor backtracked for.
    let root = hermetic_root(&[]);
    let r = resolve_in(&root, &["dev-libs/r594qtop"]);
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
    assert_eq!(r.backtrack_restarts, 0, "{:?}", r.outcome);
    assert!(r.masked_deps.is_empty(), "{:?}", r.masked_deps);
    assert_eq!(r.use_unsat_deps.len(), 1, "{:?}", r.use_unsat_deps);
    assert_eq!(r.use_unsat_deps[0].atom, "dev-libs/r594vdep[r594flag]");
    assert_eq!(
        r.fail_fast_unreached,
        [("dev-libs".to_string(), "maskeddep".to_string())]
    );
}

#[test]
fn the_first_masked_dependency_wins_over_a_later_one() {
    // Backlog #609, Portage 3.0.83 `emerge -pv [--backtrack=0]
    // dev-libs/r594htop`: rc 1, `(backtrack: 0/20)` and `0/0`, only the
    // `r594xdep[r594flag]` all-masked block under `r594ha-2`.
    let root = hermetic_root(&[]);
    for max in [20, 0] {
        let mut req = request_use_in(&root, &["dev-libs/r594htop"]);
        req.backtrack_max = max;
        let r = req.resolve().expect("the library reports the miss");
        assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
        assert_eq!(r.backtrack_restarts, 0, "{max}: {:?}", r.outcome);
        let atoms: Vec<&str> = r.masked_deps.iter().map(|m| m.atom.as_str()).collect();
        assert_eq!(atoms, ["dev-libs/r594xdep[r594flag]"], "{max}");
        assert_eq!(
            chain_nodes(&r.masked_deps[0].chain),
            [
                "dev-libs/r594ha-2::testrepo",
                "dev-libs/r594htop-1::testrepo",
                "dev-libs/r594htop"
            ]
        );
        assert!(
            matches!(
                &r.outcome,
                ResolveOutcome::Aborted { reason: AbortReason::MaskedDep { atom, .. }, .. }
                    if atom == "dev-libs/r594xdep[r594flag]"
            ),
            "{:?}",
            r.outcome
        );
    }
}

#[test]
fn a_failing_or_branch_keeps_its_missing_dependency_restart() {
    // Portage 3.0.83 `emerge -p --update --deep dev-libs/opartly` and
    // `--emptytree`: rc 1, `(backtrack: 1/20)`. The failing atoms sit in a
    // `||` group, which Portage picks in `_pop_disjunction`; the replay does
    // not model that, so the #609 cut leaves the trigger alone.
    let root = hermetic_root(&[]);
    let mut req = request_in(&root, &["dev-libs/opartly"]);
    req.update = true;
    req.deep = Deep::Unlimited;
    let r = req.resolve().expect("the library reports the miss");
    assert!(aborted(&r), "rc 1, got {:?}", r.outcome);
    assert_eq!(r.backtrack_restarts, 1, "{:?}", r.outcome);
}

#[test]
fn a_world_file_member_chains_through_selected() {
    // Backlog #611(e), Portage 3.0.83 on a fixture copy with the atom in the
    // world file, `emerge -p --backtrack=0 @world`
    // (`LLM/evidence/2026-11-06-sweep/611e/`): the masked block's chain ends
    // `"@selected" [set]`, `"@world" [argument]`, and the autounmask comment
    // `# required by @selected`, `# required by @world (argument)`.
    let root = hermetic_root(&[]);
    let mut req = request_in(&root, &["dev-libs/maskneedpkg"]);
    req.backtrack_max = 0;
    req.argument_sets.insert(
        "dev-libs/maskneedpkg".to_string(),
        "@world @selected".to_string(),
    );
    let r = req.resolve().expect("the library reports the miss");
    assert_eq!(r.masked_deps.len(), 1, "{:?}", r.masked_deps);
    let tail: Vec<(&str, &str)> = r.masked_deps[0]
        .chain
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    assert_eq!(
        tail,
        [
            ("dev-libs/maskneedpkg-1.0::testrepo", "ebuild"),
            ("@selected", "set"),
            ("@world", "argument")
        ]
    );
    let mut req = request_use_in(&root, &["dev-libs/r594qa"]);
    req.backtrack_max = 0;
    req.argument_sets.insert(
        "dev-libs/r594qa".to_string(),
        "@world @selected".to_string(),
    );
    let r = req.resolve().expect("the library reports the change");
    let chain = &r.autounmask_use_changes[0].dep_chain;
    assert_eq!(
        chain,
        &[
            "required by dev-libs/r594qa-2::testrepo",
            "required by @selected",
            "required by @world (argument)"
        ]
    );
}
