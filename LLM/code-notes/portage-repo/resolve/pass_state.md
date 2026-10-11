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
- REQUIRED_USE (see the check further below, in the main BFS loop): Portage depgraph.py's own `_add_pkg` sets `_dynamic_config._required_use_unsatisfied = True` and returns 0 on a violation -- it does NOT abort the whole graph walk, unlike a top-level atom's own `NoVisibleCandidate`.

## field PassState::missing_args
- Backlog #286. A repeat visit of the same atom does not re-record.
- Portage drops them into `_missing_args` (`depgraph.py:5577-5605`) and continues the walk; they ride into [`PassResult::missing_args`] below.

## field PassState::suppressed_nvc
- #57. An ordinary pass is unaffected.

## field PassState::minimize_pins
- #236 B: `_minimize_children` verdicts for an owner's same-cp sibling atoms, keyed `(owner cp, atom text)` -- filled when the first sibling pops, consumed as each one pops (see `run_pass`).

## field PassState::top_pinned_installed
- Backlog #285: per-pass memo for [`arg_pinned_installed_reuse`] -- `cat/pkg` -> the `(version, main slot)` pairs the top-level atoms on that cp keep installed.

## field PassState::installed_meta_memo
- A1 (#26): per-pass cache for `installed_dep_string`'s `Effective` layer, keyed `(category, package, version, key)`.

## field PassState::parent_flip_recorded
- Backlog #195 (round 2): the `(category, package)` keys of dependencies whose `NoVisibleCandidate` the `'parent_flip` off-arm collected into a parent USE flip (record-and-fail, Portage `_apply_parent_use_changes`, `depgraph.py:5820`).
- Portage drops each collected item from `_unsatisfied_deps_for_display` (`remaining_items`), so no masked/use-unsat/plain-miss disclosure is recorded for them below.

## field PassState::changed_deps_report_seen
- `--changed-deps-report`: Portage `_changed_deps_pkgs` is a dict keyed by the installed `Package` object, so a repeat visit to the same installed category/package/version (e.g. via both a bare "dev-libs/foo" and an explicit "dev-libs/foo:0" atom text, or a diamond dependency) naturally collapses to one entry -- mirrored here with an explicit dedup set, keyed the same way, preserving first-encountered order (Portage dict iteration order) rather than sorting.

## field PassState::queue
- Each queued atom carries its own depth (0 for a directly-requested top-level atom, parent's depth + 1 for anything reached only via a dependency string) -- only consulted by `deep.recurses_at` below, for deciding whether an AlreadyInstalled package's own further dependencies get walked; every other outcome ignores it entirely (see `Deep`'s own doc comment) -- and the `(category, package)` that pushed it, if any (`None` for a directly-requested top-level atom), only consulted by `required_by_map` below, for `GraphEntry`'s own `required_by` field.

## field PassState::autounmask_keyword_changes
- Portage `--autounmask` changes applied during this walk -- see `GraphResult::autounmask_keyword_changes` / `autounmask_use_changes`.

## field PassState::required_by_map
- (category, package) -> every distinct owner that reached it via a dependency string, accumulated separately from the BFS's own dedup/recursion decisions below (`visited_atoms`/`resolved_slots`/ `other_outcomes`) so a diamond dependency's *second* (deduped) owner still gets recorded even though it never triggers a new resolution -- merged into `entries` in a single post-pass at the end, mirroring `pending_blockers`/`resolve_blockers`'s own "accumulate now, merge once the whole graph is known" shape.

## field PassState::edge_kind_map
- An edge counts as unbreakable only when `has_hard && !has_soft` -- one run-time or optional route makes the whole edge breakable, matching Portage's per-edge `DepPriority`.

## field PassState::use_overlay
- This pass's `--autounmask-use` flips (`(cat, pkg) -> {flag -> desired}`), overlaying `bp.autounmask_use_config` (see `overlay_use_want`).
