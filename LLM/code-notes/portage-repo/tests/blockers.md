# crates/portage-repo/src/tests/blockers.rs
Blocker resolution tests: seed holds, buildpkgonly gate, per-cell classification.

## fn set_seed_holds_the_instance_an_installed_holders_slot_edge_selects
- Backlog #296: an installed holder's recorded `bdprov:0` is a seed atom like a world atom; it holds the slot-0 instance a slot-1 merge would otherwise uninstall. `:=` / bare edges hold nothing (a bare `:=` rebinds to the merge).

## fn set_seed_holds_the_instance_only_a_slot_atom_selects
- Backlog #289: a set atom that selects the old instance holds it (Portage `SetArg` parent); a bare atom that the merged new instance also matches does not (it selects the new slot), nor does another slot's atom, a set, or a blocker.

## fn resolve_blockers_matches_a_graph_resolved_package_with_no_installed_candidates
- #68 S2 (cell e): a merge-bound match with a merging parent is unresolved outright; Portage prints `B` and exits 1. The pre-S2 expectation (`false`) was the forward-order phantom.

## fn resolve_blockers_drops_a_use_dep_blocker_the_target_does_not_satisfy
- `!!dev-libs/target[wantblock]` against a graphed target whose `use_flags_display` has `wantblock` off is a satisfied no-op, dropped (Portage omits the `[blocks]` line).
- `(+)` default: `gone` is absent from the target's IUSE, so it stands in as enabled and `-gone` is unmet.
- A plain (no `[use]`) blocker still fires, and a use-dep the target DOES satisfy fires too.

## fn resolve_blockers_buildpkgonly_gate
- #143 S1, expected value from Portage (upstream `test_blocker.py::testBlockerBuildpkgonly`, oracle `differential-test-bed/logs/fx-b1-blocker.json` cases 6-11): `depgraph.py::_add_pkg_dep_string` blanks RDEPEND/PDEPEND/IDEPEND for a non-built package, and `_validate_blockers` clears `depends_on_order` for every blocker that is not a hard (`!!`) buildtime one.
- Under `buildpkgonly` only a strong buildtime blocker files (resolved by uninstall, the abort's rows; the exit itself is `buildpkgonly_deps_unsatisfied`'s); soft and runtime-keyed blockers are satisfied silently. A hard RDEPEND blocker is never even walked (the RDEPEND blanking).
- #143 S1: Portage's `--buildpkgonly` blocker gate (upstream `test_blocker.py::testBlockerBuildpkgonly`, oracle `differential-test-bed/logs/fx-b1-blocker.json` cases 6-11); Portage never even walks a hard RDEPEND blocker (the RDEPEND blanking); a hard DEPEND blocker is kept under buildpkgonly and resolved by uninstalling the installed match.

## fn resolve_blockers_drops_a_blocker_from_a_superseded_walk_candidate
- #142 S2 F2 (upstream `test_blocker.py::testBlocker`, oracle `differential-test-bed/logs/fx-b1-blocker.json` cases 0-5): every pg0 order walks several X candidates but merges one, and Portage only ever sees blockers of graph nodes (`_validate_blockers` iterates the digraph). This is what made the uninstall target follow walk order (Y-2/Y-3) instead of the merged candidate (Y-1).

## fn resolve_blockers_keeps_an_installed_match_replaced_in_its_slot_as_satisfied
- #68 S1 / #72 B1 (cell a): the row is kept, tagged `Replacement`; the renderer hides it until B2 models Portage's scheduling predicate.
- Cell a': Portage's same-slot replacement rule; a different-slot installed match is removed (#72 B3).
- Cell a: installed `blocked-1.0:0` is replaced by the merge-bound `blocked-2.0:0`, so Portage's tracker no longer holds it and the block resolves to the replacement merge.

## fn resolve_blockers_uninstalls_a_settled_block_when_the_cp_moves_slot
- #284 (`app-misc/bdprov` shape), expected value from Portage: the provider upgrades 2 -> 3 into a new slot while its own `!blocked:0` soft-blocks the old installed instance. Portage uninstalls the old slot (`[uninstall]` + `[blocks b]`, rc 0); the request parents the merge, not the old instance, so the cp-keyed retry closure must not hold it once the run re-resolves the cp elsewhere.
- Case 2: the bound consumer rebuilds (`:=`), so its live dep rebounds through its own walk.
- Control: a `:0`-pinned consumer (no rebind) still holds the old instance; the edge survives the #284 skip, so the row stays unresolved even with the closure veto lifted.

## fn resolve_blockers_classifies_final_graph_matches_by_parent_operation
- #68 S2/S3 oracle cells b-h.
- Cell d is Portage's `b`. The complete-mode re-mark comes from the L0 `sys-apps/systemd-utils` shape: Portage's digraph holds the installed closure, so the vdb reverse scan (documented fallback) marks it unresolved again; the consumer must be an installed vdb package to be found.
- Cell g matches two instances (#72 B1); the satisfied nomerge arm is #77 A1 (removes the owner, ordered after the blocked instance via `anchor`).
- #73 cell g: the `@world`/`@selected`/`@system` closure is a SetArg parent. #73 S0 cell f / T11: `merge_bound_match` gates the whole nomerge arm.

## fn resolve_blockers_tags_a_scanned_nomerge_owner_with_its_own_removal
- #77 A1 / A0 n1: the owner is the vardb `Package` the all-installed scan registered (`owner_installed: true`). Never walked, so no entry even when the target cp's merge entry exists; Portage removes the owner, ordered after the merge-bound blocked instance (`inst_task`).
- Backlog #80: a closure member's unresolved row is no longer dropped; it rides out as an orphan for the trailing blocker group (A0 n1b).

## fn unwalked_scan_skips_without_merge_bound_entries_and_blocker_free_owners
- #83: (a) with nothing merge-bound the whole scan is a no-op even when an installed owner records a blocker (`resolve_blockers` would drop every row it could produce); (b) an owner whose effective runtime deps carry no `!` token cannot contribute a blocker.
