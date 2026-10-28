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
