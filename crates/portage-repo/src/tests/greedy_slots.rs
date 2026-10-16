//! #359 S3: the default solver's `_greedy_slots` expansion.
//!
//! The fixture tree lives in the sibling pmtest checkout (`fixtures/` is a
//! symlink); oracles are Portage 3.0.82.2 captures, never portuale's output.

use super::*;

fn resolve_updated(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    ResolveRequest {
        update: true,
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
    .map(|e| {
        (
            format!(
                "{}/{}:{}",
                e.category,
                e.package,
                e.slot.as_deref().unwrap_or("0")
            ),
            e.outcome,
        )
    })
    .collect()
}

#[test]
fn greedy_slots_below_best_keeps_only_lower_installed_slots() {
    // Pure filter: slot 1 below the top is kept, slot 3 above it is not,
    // the top slot itself never duplicates.
    let got = greedy_slots_below_best(
        GreedySlotsBelowBestArgs {
            top_slot: "2",
            top: &2,
            per_slot_best: &[("1", &1), ("3", &5)],
            installed_slots: &["1".to_string(), "2".to_string(), "3".to_string()],
            update: true,
        },
        |a, b| a < b,
    );
    assert_eq!(got, vec!["2".to_string(), "1".to_string()]);
}

#[test]
fn greedy_slots_below_best_is_a_noop_without_update() {
    let got = greedy_slots_below_best(
        GreedySlotsBelowBestArgs {
            top_slot: "2",
            top: &2,
            per_slot_best: &[("1", &1)],
            installed_slots: &["1".to_string()],
            update: false,
        },
        |a, b| a < b,
    );
    assert_eq!(got, vec!["2".to_string()]);
}

#[test]
fn greedy_slots_update_adds_the_installed_slot_upgrade() {
    // Oracle: Portage 3.0.82.2 `359-gs-pvu` (`NS gs-2:2` + `U gs-1.1:1`).
    let entries = resolve_updated("dev-libs/pg34-gs");
    assert!(
        entries.iter().any(|(n, o)| n == "dev-libs/pg34-gs:2"
            && matches!(o, PretendOutcome::New { version } if version == "2")),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|(n, o)| n == "dev-libs/pg34-gs:1"
            && matches!(o, PretendOutcome::Upgrade { from, to } if from == "1.0" && to == "1.1")),
        "{entries:?}"
    );
}

#[test]
fn greedy_slots_without_update_plans_the_new_slot_only() {
    // Oracle: Portage 3.0.82.2 `359-gs-pv` (`NS gs-2:2` alone).
    let root = fixtures_root();
    // `selective: false` mirrors the `emerge -pv` CLI shape (no `--update`,
    // no `--selective` trigger): the top level takes the best, like Portage.
    let mut req = ResolveRequest::new(
        &root,
        &root,
        &["dev-libs/pg34-gs".to_string()],
        &test_config(),
        &fixtures_root().join("distfiles"),
    );
    req.selective = false;
    let plain: Vec<(String, PretendOutcome)> = req
        .resolve()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| {
            (
                format!(
                    "{}/{}:{}",
                    e.category,
                    e.package,
                    e.slot.as_deref().unwrap_or("0")
                ),
                e.outcome,
            )
        })
        .collect();
    assert_eq!(plain.len(), 1, "{plain:?}");
    assert!(
        matches!(&plain[0], (n, PretendOutcome::New { version }) if n == "dev-libs/pg34-gs:2" && version == "2"),
        "{plain:?}"
    );
}

#[test]
fn greedy_slots_pinned_and_ranged_atoms_stay_put() {
    // Oracle: Portage 3.0.82.2 `359-gs-pvu-slot2` / `359-gs-pvu-eq2` (each a
    // lone `NS gs-2:2`): no installed slot matches the pin, so the vardb
    // half of `_greedy_slots` is empty.
    for atom in ["dev-libs/pg34-gs:2", "=dev-libs/pg34-gs-2"] {
        let entries = resolve_updated(atom);
        assert_eq!(entries.len(), 1, "{atom}: {entries:?}");
        assert!(
            matches!(&entries[0], (n, PretendOutcome::New { version }) if n == "dev-libs/pg34-gs:2" && version == "2"),
            "{atom}: {entries:?}"
        );
    }
}

#[test]
fn greedy_slots_range_atom_adds_the_installed_slot() {
    // Oracle: Portage 3.0.82.2 `s0-a-gs-range` (`NS gs-2:2` + `U gs-1.1:1`
    // for `>=dev-libs/pg34-gs-1.0 -pvu`): greedy covers range atoms (D5).
    let entries = resolve_updated(">=dev-libs/pg34-gs-1.0");
    assert!(
        entries.iter().any(|(n, o)| n == "dev-libs/pg34-gs:2"
            && matches!(o, PretendOutcome::New { version } if version == "2")),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|(n, o)| n == "dev-libs/pg34-gs:1"
            && matches!(o, PretendOutcome::Upgrade { from, to } if from == "1.0" && to == "1.1")),
        "{entries:?}"
    );
}

#[test]
fn greedy_slots_blocker_lookahead_drops_the_blocked_slot() {
    // Oracle: Portage 3.0.82.2 `s0-e-gsblk-pvu` (`NS gsblk-2:2` alone, then
    // the satisfied uninstall/block rows): gsblk-2 blocks `:1`, so the
    // lookahead drops the slot atom and no slot-1 merge is planned.
    let entries = resolve_updated("dev-libs/pg359-gsblk");
    assert!(
        entries.iter().any(|(n, o)| n == "dev-libs/pg359-gsblk:2"
            && matches!(o, PretendOutcome::New { version } if version == "2")),
        "{entries:?}"
    );
    assert!(
        !entries.iter().any(|(n, o)| n == "dev-libs/pg359-gsblk:1"
            && matches!(
                o,
                PretendOutcome::New { .. }
                    | PretendOutcome::Upgrade { .. }
                    | PretendOutcome::Downgrade { .. }
                    | PretendOutcome::Reinstall { .. }
            )),
        "{entries:?}"
    );
}

#[test]
fn greedy_slots_skips_set_members() {
    // Portage appends greedy atoms for `AtomArg` only, never set members
    // (plan §0.3.1): with the atom marked as a set member, no seed is added.
    let root = fixtures_root();
    let req = ResolveRequest {
        update: true,
        set_args: HashSet::from(["dev-libs/pg34-gs".to_string()]),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/pg34-gs".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    };
    let ctx = ResolveCtx::new(&req).expect("fixture ctx builds");
    let bp = BacktrackParams::initial(&req);
    let union = bp.union_constraints();
    let greedy = greedy_slot_atoms(GreedySlotsArgs {
        ctx: &ctx,
        config: &req.config,
        union_constraints: &union,
    });
    assert!(greedy.is_empty(), "{greedy:?}");
    // And unmarked, the same atom expands (guards the test above against a
    // vacuous helper).
    let req = ResolveRequest {
        update: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/pg34-gs".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    };
    let ctx = ResolveCtx::new(&req).expect("fixture ctx builds");
    let bp = BacktrackParams::initial(&req);
    let union = bp.union_constraints();
    let greedy = greedy_slot_atoms(GreedySlotsArgs {
        ctx: &ctx,
        config: &req.config,
        union_constraints: &union,
    });
    assert_eq!(greedy, vec!["dev-libs/pg34-gs:1".to_string()]);
}
