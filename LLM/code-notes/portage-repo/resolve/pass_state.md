# crates/portage-repo/src/resolve/pass_state.rs
The state and result of one resolver pass.

## struct PassResult
- Phase A2 (023). C2: plus the overlay trio, folded into the node params by the driver before deciding.

## field PassResult::skipped_updates
- Backlog #90 (S2): removal rows from the direct solve (run_pass tail) plus constraint-withhold rows (collect_feedback's reverse-pin site).

## field PassResult::orphan_blockers
- Backlog #80. Filed by `file_blocker_conflicts` (this pass's scan).

## field PassResult::missing_args
- Backlog #286 (see `PassState::missing_args`).

## field PassResult::parent_flip_rescued
- Backlog #217: the `'parent_flip` off-arm records the flip and fails instead of the overlay. Portage's counterpart collects the flip post-failure (`_apply_parent_use_changes`, `depgraph.py:5820`), so the `_success_without_autounmask` tail is pre-empted and the "terminated early" notice prints.

## field PassResult::abi_rebuilds
- #24 S3. Stashed by `collect_feedback`. `None` = the scan did not run for this pass (the `get_best_run` re-pass, which never goes through `collect_feedback`). Portage's `_forced_rebuilds`.

## field PassResult::suppressed_nvc
- #57 (see `PassState::suppressed_nvc`).

## field PassResult::parent_atoms
- #59 S1. Portage's `_add_parent_atom`.

## struct PassState
- Phase A3 (023): the walk's-Everything per-pass `let mut`s. Field docs are the original declaration-site comments, moved verbatim. Today the walk writes `bp` directly; the overlay trio is where the code already has them.

## field PassState::visited_atoms
- E.g. a bare "dev-libs/foo" and a slotted "dev-libs/foo:1" can both need resolving though they'd share a coarser key. Exists purely for termination, not slot-resolution decisions.

## field PassState::resolved_slots
- Track X Slice B (#242): Portage keys graph nodes by package object (`Package.__hash__`), so a running-root atom never reuses a target-root slot (nor vice versa). Uniformly `false` on same-root runs: identical keys.

## field PassState::resolved_depths
- Backlog #218. Portage's `pkg.depth`, set once in `_add_pkg` (`depgraph.py:3763`): the delta atoms queue exactly where a fresh walk would put them.

## field PassState::installed_slots
- #57. An `AlreadyInstalled` `GraphEntry` carries no slot (read from the vdb `InstalledRef`); the `unreachable!` at the merge-outcome slot check stays truthful. Portage's `_add_pkg` adds installed nomerge nodes to `_package_tracker` exactly like merge-bound ones (`depgraph.py:3736`), so a slot holding one of each is an ordinary slot collision (`PackageTracker.conflicts()`); portuale's walker used to see installed nodes only through the early `resolved_version.is_none()` branch, which never consulted `resolved_slots` at all -- the whole of backlog #57. Track X Slice B (#242): an installed instance in each root is two nodes.

## field PassState::other_outcomes
- Portage's installed nodes are keyed by `pkg.slot_atom` (`_package_tracker`): `llvm-core/clang:22` and `:23` coexisting is exactly the #74 S2a shape. Track X Slice B (#242): keyed by resolving root.

## field PassState::root_deps_build_seen
- See `resolve_root_deps_build_entries`.

## field PassState::slot_pullers
- C3's `slot_conflict_mask_choices` reads the triples to build each choice's `conflict_atoms` (Portage's parent side); parent downgrades now arrive via missing-dep feedback on a later pass, not pre-masking (see the step-3 site).

## field PassState::required_use_violations
- Portage's `_add_pkg` does NOT abort the whole graph walk on a violation, unlike a top-level atom's own `NoVisibleCandidate`. Matches Portage's `_unsatisfied_deps_for_display` list (checked once, at the very end of the resolve) rather than portuale's previous "abort on the first hit" shortcut.

## field PassState::missing_args
- Backlog #286. A repeat visit of the same atom does not re-record.

## field PassState::suppressed_nvc
- #57. An ordinary pass is unaffected.
