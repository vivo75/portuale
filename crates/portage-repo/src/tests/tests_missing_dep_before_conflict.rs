//! A dependency that stops Portage's walk files no slot-conflict feedback in
//! that pass (#592). Expected values are host Portage 3.0.83 captures in
//! `LLM/evidence/2026-10-26-575-residues/r1/captures/`, cited per test.

use super::tests_consumer_pin_conflict::{
    hermetic_root, kinds, portage_tries, request_in, resolve_in, trace_search,
};
use super::*;

fn merged(result: &GraphResult) -> Vec<String> {
    result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect()
}

fn with_backtrack(root: &Path, atom: &str, max: u32) -> GraphResult {
    let mut req = request_in(root, &[atom]);
    req.backtrack_max = max;
    req.resolve().expect("resolves")
}

#[test]
fn plain_miss_stops_the_walk_before_the_conflict() {
    // `A-portage.txt`: rc 0, `(backtrack: 1/20)`, as-1 ax-2 ay-1 atop-1. ay-2
    // hits the absent r592agone before ax-2 pulls as-1 (D:3535, D:3276-3280).
    let root = hermetic_root(&[]);
    let r = resolve_in(&root, &["dev-libs/r592atop"]);
    assert_eq!(r.backtrack_restarts, 1);
    assert_eq!(
        merged(&r),
        [
            "dev-libs/r592as",
            "dev-libs/r592ax",
            "dev-libs/r592ay",
            "dev-libs/r592atop"
        ]
    );
}

#[test]
fn plain_miss_settles_within_one_try() {
    // `Abacktrack1-portage.txt`: rc 0, `(backtrack: 1/1)`, the same 4 rows.
    let root = hermetic_root(&[]);
    let r = with_backtrack(&root, "dev-libs/r592atop", 1);
    assert_eq!(r.backtrack_restarts, 1);
    assert_eq!(r.entries.len(), 4);
}

#[test]
fn masked_dep_stops_the_walk_before_the_conflict() {
    // `C-portage.txt` and `Cbacktrack1-portage.txt`: rc 0, 1/20 and 1/1.
    let root = hermetic_root(&[]);
    assert_eq!(
        resolve_in(&root, &["dev-libs/r592ctop"]).backtrack_restarts,
        1
    );
    let r = with_backtrack(&root, "dev-libs/r592ctop", 1);
    assert_eq!(r.backtrack_restarts, 1);
    assert_eq!(r.entries.len(), 4);
}

#[test]
fn use_only_miss_without_autounmask_backtracks_nothing() {
    // `Dautounmaskn-portage.txt`: rc 1, `(backtrack: 0/20)`, no merge list.
    // A USE-only miss files nothing (D:3488-3497) and the walk stops.
    let root = hermetic_root(&[]);
    let req = request_in(&root, &["dev-libs/r592dtop"]);
    let r = req
        .resolve()
        .expect("the library reports the miss as an entry");
    assert_eq!(r.backtrack_restarts, 0);
    assert!(
        r.entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    );
    let trace = trace_search(&req);
    assert_eq!(portage_tries(&trace), 0, "{:?}", kinds(&trace));
}

#[test]
fn use_only_miss_beside_a_conflict_without_autounmask_backtracks_nothing() {
    // `Fautounmaskn-portage.txt`: rc 1, `(backtrack: 0/20)`.
    let root = hermetic_root(&[]);
    let req = request_in(&root, &["dev-libs/r592ftop"]);
    let r = req
        .resolve()
        .expect("the library reports the miss as an entry");
    assert_eq!(r.backtrack_restarts, 0);
    assert!(
        r.entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    );
    let trace = trace_search(&req);
    assert_eq!(portage_tries(&trace), 0, "{:?}", kinds(&trace));
}

#[test]
fn conflict_before_the_miss_still_backtracks() {
    // `E-portage.txt`: the conflict is added before the miss, so it is
    // processed: rc 0, 6/20, 5 rows.
    let root = hermetic_root(&[]);
    let r = resolve_in(&root, &["dev-libs/r592etop"]);
    assert_eq!(r.backtrack_restarts, 6);
    assert_eq!(r.entries.len(), 5);
}
