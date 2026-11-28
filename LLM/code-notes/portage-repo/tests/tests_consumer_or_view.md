# crates/portage-repo/src/tests/tests_consumer_or_view.rs
An installed consumer's `||` is chosen the way `dep_zapdeps` chooses it, in the view Portage uses (#580 10b).

## module
- Oracle: Portage 3.0.83 on scratch copies of the fixtures, captures in `LLM/evidence/2026-10-26-575-residues/t10b/captures/` (`t1-*`, `t2-*`, `t3-*`, `q19-order-*`, each `*-portage.txt` with its `*-portuale-before.txt` and `*-portuale.txt`). Probe scripts `run-pair.sh`, `probe-*.sh` in the same folder.
- `t2_swap_slotop_matches_portage` is `#[ignore]`: Portage's T2 conflict comes from the built `prov:0/5.42=` atom alone (D:8803-8834), which portuale does not model.
- `sibling_virtuals_share_the_atom_a_sibling_chose` passes on HEAD too (the installed-arm rule gave the same rows); it guards the Q19 pre-pass.

## fn w604_root
- Backlog numbers removed from the comment (original text): ---- #604: the leak is keyed like Portage's atom intern key ----

## fn installed_virtual_takes_no_atom_from_its_parent
- Backlog numbers removed from the comment (original text): #604. The parent's plain `=5.42*` never shares the virtual's dep_check (D:4872-4904), and its `||` pick is parsed with EAPI 8 where the installed virtual's atoms carry none (DC:866-871), so neither leaks into the installed virtual. Portage 3.0.83, `-p --backtrack=0 --verbose-conflicts` (pmtest `test_604_*`, shapes A and C): the virtual is never a `=5.42*` parent, and the conflict prints once.

## fn merge_bound_virtual_takes_the_parents_or_pick_of_its_eapi
- Backlog numbers removed from the comment (original text): #604. With the virtual merge-bound its RDEPEND is parsed with its own EAPI 8, the parent's: the parent's `||` pick `=5.42*` is the virtual's own atom object, so the virtual is a `=5.42*` parent too (D:6122-6133). A plain parent atom is not in the call and does not leak. Portage 3.0.83 (shapes D and B).

## (see source)
- Backlog numbers removed from the comment (original text): ---- #606: an installed consumer's selected arm is a parent of the merge it selects ----

## fn installed_virtual_selecting_the_merge_is_its_parent
- Backlog numbers removed from the comment (original text): #606. Complete mode evaluates the installed `virtual/p590vva`; with `p590vprov-5.44` in the graph its `=5.44*` arm is chosen, and a dependency the graph satisfies is a parent atom of that node (D:8799-8809 only sends graph-unsatisfied ones to the vdb test). Portage 3.0.83 `emerge -p --backtrack=0 --verbose-conflicts dev-libs/p590vpull` (clusterB `q19-bt0`): the 5.44 instance lists `p590vpull-2` and `p590vva-1`; the 5.42 one both virtuals.

## fn q19_backtracks_six_times_like_portage
- Backlog numbers removed from the comment (original text): #606. Both choices carry two conflict atoms, so the version-desc order stands and 5.42 is masked first: 5.42, +5.44, +p590vpull-2, the dead end on the argument, then 5.44 alone and +p590vpull-2 (Portage 3.0.83 `--debug`, clusterB `q19-portage-debug`): `backtrack: 6/20`, `N p590vpull-1`.
