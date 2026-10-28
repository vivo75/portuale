//! The printed `(backtrack: N/M)` count is Portage's `backtracked`.
//!
//! Expected values come from host Portage 3.0.83 on scratch copies of the
//! fixtures (`LLM/evidence/2026-10-26-575-residues/r6/captures/full-table.md`).
//! `D` = `lib/_emerge/depgraph.py`. Portage counts every loop pass that does
//! not end the loop (D:12407-12418): a restart always (D:12414-12416, even
//! when every child it adds is rejected), any other failed pass only while
//! unexplored nodes remain (`elif backtracker`, D:12417-12418).

use super::tests_consumer_pin_conflict::{
    hermetic_root, request_in, resolve_complete_in, resolve_in,
};
use super::*;

/// One `resolve` with an explicit `--backtrack` budget.
fn resolve_in_backtrack(root: &Path, atoms: &[&str], backtrack_max: u32) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.backtrack_max = backtrack_max;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// One `resolve` with `--autounmask-backtrack=y` for several top-level atoms.
fn resolve_autounmask_backtrack_atoms(atoms: &[&str]) -> GraphResult {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    config.autounmask_backtrack = true;
    let owned: Vec<String> = atoms.iter().map(|a| (*a).to_string()).collect();
    super::graph_result_autounmask_cfg_atoms(&config, &owned)
}

#[test]
fn missing_dep_abort_counts_portages_single_try() {
    // `emerge --pretend dev-libs/ormissbad`: Portage 3.0.83 prints
    // `backtrack: 1/20` (`R1 F0`); the failed last walk still ends a loop
    // pass. `r6/captures/full-table.md`, row `fx-ormissbad`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/ormissbad"]);
    assert_eq!(result.backtrack_restarts, 1);
}

#[test]
fn masked_mid_abort_counts_portages_single_try() {
    // `emerge --pretend dev-libs/abort-masked-mid`: Portage 3.0.83 prints
    // `backtrack: 1/20` (`R1 F0`). `r6/captures/full-table.md`, row
    // `fx-abort-masked-mid`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/abort-masked-mid"]);
    assert_eq!(result.backtrack_restarts, 1);
}

#[test]
fn unsolvable_slot_conflict_counts_every_portage_try() {
    // `emerge --pretend dev-libs/slotconflictunsolvable`: Portage 3.0.83
    // prints `backtrack: 6/20` (`R2 R2 R2 F+ R1 R1 F0`).
    // `r6/captures/full-table.md`, row `fx-slotconflictunsolvable`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/slotconflictunsolvable"]);
    assert_eq!(result.backtrack_restarts, 6);
}

#[test]
fn slot_use_group_counts_portages_four_tries() {
    // `emerge --pretend dev-libs/slotusegroup`: Portage 3.0.83 prints
    // `backtrack: 4/20` (`R2 R2 R2 F+ F0`).
    // `r6/captures/full-table.md`, row `fx-slotusegroup`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/slotusegroup"]);
    assert_eq!(result.backtrack_restarts, 4);
}

#[test]
fn missing_dep_chain_counts_plain_walk_like_portage() {
    // `emerge --pretend dev-libs/r6c0`: Portage 3.0.83 prints `backtrack:
    // 4/20` (`R1 R1 R1 R1 F0`) for the `r6c0 -> r6c1 -> r6c2 -> r6c3 ->
    // r6gone` chain. `r6/captures/full-table.md`, row `ch-d`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/r6c0"]);
    assert_eq!(result.backtrack_restarts, 4);
}

#[test]
fn depth_capped_chain_counts_the_all_rejected_restart() {
    // Every child of the last restart is rejected (`mask_steps>max_depth`),
    // yet Portage counts the restart (D:12414-12416): `R1 R0` at
    // `--backtrack=2` prints `2/2`, `R1 R1 R0` at `--backtrack=3` prints
    // `3/3`, `R1 R1 R1 R0` at `--backtrack=5` prints `4/5`.
    // `r6/captures/full-table.md`, rows `ch-b2`, `ch-b3`, `ch-b5`.
    let root = hermetic_root(&[]);
    for (max, portage) in [(2, 2), (3, 3), (5, 4)] {
        let result = resolve_in_backtrack(&root, &["dev-libs/r6c0"], max);
        assert_eq!(result.backtrack_restarts, portage, "--backtrack={max}");
    }
}

#[test]
fn grow_flip_portage_would_not_restart_for_is_not_counted() {
    // `--autounmask-backtrack=y dev-libs/r594vdep dev-libs/r594qa` (either
    // order): Portage 3.0.83 stops at the first pass (`B:need_config_change`,
    // 0/20). The flip on `r594vdep` changes no flattened dependency and
    // breaks no parent USE dep, so `want_restart_for_use_change` (D:7747-7787)
    // is false. `r6/captures/full-table.md`, rows `fxq-1`, `fxq-2`.
    for atoms in [
        ["dev-libs/r594vdep", "dev-libs/r594qa"],
        ["dev-libs/r594qa", "dev-libs/r594vdep"],
    ] {
        let result = resolve_autounmask_backtrack_atoms(&atoms);
        assert_eq!(result.backtrack_restarts, 0, "{atoms:?}");
    }
}

#[test]
fn grow_flip_that_changes_deps_still_counts() {
    // `--autounmask-backtrack=y dev-libs/aucasctop`: the `cascade` flip on
    // `aucascmid` gates `aucascleaf`, so Portage restarts (1/20).
    let result = super::graph_result_autounmask_backtrack("dev-libs/aucasctop");
    assert_eq!(result.backtrack_restarts, 1);
}

#[test]
fn breakage_argument_order_stays_at_three() {
    // `--autounmask-backtrack=y dev-libs/aub0a dev-libs/aub0b dev-libs/aub0c`:
    // Portage 3.0.83 prints `backtrack: 3/20`. The failed last walk (+1) and
    // the grow flip Portage never reaches (-1) cancel out.
    // `r6/captures/full-table.md`, row `aub-y-abc`.
    let result = super::aub0_result("a b c", true);
    assert_eq!(result.backtrack_restarts, 3);
}

#[test]
fn etop_control_stays_at_zero() {
    // `emerge --pretend dev-libs/r594etop`: Portage 3.0.83 prints
    // `backtrack: 0/20` (`F0`). `r6/captures/full-table.md`, row
    // `fx-r594etop`.
    let root = hermetic_root(&[]);
    let result = resolve_complete_in(&root, &["dev-libs/r594etop"]);
    assert_eq!(result.backtrack_restarts, 0);
}
