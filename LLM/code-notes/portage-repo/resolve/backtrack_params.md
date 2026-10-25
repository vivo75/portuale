# crates/portage-repo/src/resolve/backtrack_params.rs
Parameters a backtracking pass runs with.

## struct MaskChoice
- Phase C3 (023): Portage's `conflict_atoms`. Phase C4: Portage's `similar_pkgs` grouping -- same `cat/pkg`, same slot, visible, not excluded, not already masked, not already a conflict party.
- One ranked mask choice for a slot conflict: mask `target` (a `(cat, pkg, ver)` triple), opposed by `parents` (the pulling `((cat, pkg, ver), atom)` triples whose atom does NOT match the masked version -- Portage's `conflict_atoms`). One choice becomes one sibling node; DFS explores the last-added first.

## fn slot_conflict_mask_choices
- Phase C3 (023): port of Portage `lib/_emerge/depgraph.py::_slot_confict_backtrack` (note the upstream one-`l` spelling), C4 extended with the `_iter_similar_available` grouping. Portage's bug-692746 append; exactly like Portage's sort-then-`pop()`; the same documented subslot approximation C1 used, kept so the two call sites cannot disagree. Portage's `conflict_pkgs.sort(reverse=True)`.
- Portage keeps `@selected` in the set for the same null effect (top-level-argument pseudo-pullers).
- Similar-grouping gates: same `cat/pkg` (the listing is scoped by cp), same main slot, `is_visible` under the effective config (Portage passes no `autounmask_level` at its own call site either), not `--exclude`d (same `matches_config_entry` idiom as selection), not already masked (exact-`!=cpv`-string presence -- same-version-different-repo is treated as masked, absent from fixtures), and not already a conflict party. The ebuild-only listing moots Portage's installed-skip and its built/binpkg-visibility branches (no counterpart, documented cut). Portage's `if conflict_atoms` gate; deterministic group order.
- Portage's `backtrack_data.sort`.
- Two known ceilings, both documented (neither is exercised by any fixture, and ranking-only effects cannot change the explored set, only its order): portuale keeps two per conflict -- existing + current -- where Portage ranks every pulled version, so a 3+-version conflict drops its third candidate; and `conflict_atoms` membership is version+slot only (Portage joins atoms via `atom.match(pkg.with_use(...))`, so a parent that version-matches but USE-mismatches counts as conflicting here and merely inflates the choice's length). Portage's `instances[0]` is always the resolved version, so the 692746 append only fires when the instance list is somehow empty.

## enum BacktrackFeedback
- Phase C2 (023); exactly as the old live-`bp` code did. A pass with both growth and a conflict takes the growth -- Portage would park the config node and explore the conflict first; that combined ordering is a deliberate C2 cut.

## enum BacktrackFeedback::RevDep
- Backlog #209 (`_complete_graph`'s installed-consumer pins, `collect_feedback`'s scan). Portage `_resolve_conflicts` (`_emerge/depgraph.py:9444`) calls `_complete_graph()` (`:8562`). Behaves identically inside the search.

## enum BacktrackFeedback::Circular
- Backlog #216 (Portage `_serialize_tasks`' `not selected_nodes` branch setting `_need_restart` with a grown `circular_dependency` map). Settles the pass as-is under `--backtrack=0`, where it renders the circular abort like Portage's own pass-0 display.

## enum PassDecision::DeadEnd
- Top-level or latched parent, USE-only mismatch. See `Backtracker::adopt_current`. Without this, such a pass would settle straight into the abort path and hide the unmasked best run.

## struct BacktrackParams
- Phase A1 (023): the cross-pass half of `backtracking_resolve`'s old `let mut` block. Cloned per search node once C2 lands. The C1 trial quartet (`mask_phase`/`mask_trial_spent`/`mask_negatives`/`pre_trial_nvc`) and the `Reverting` pass had no Portage counterpart -- portuale-only, removed in C2 when `get_best_run` replaced them.
- Portage `lib/_emerge/resolver/backtracking.py::BacktrackParameter` carries `runtime_pkg_mask` (a `cpv -> mask-reason` dict fed by every feedback path), `needed_use_config_changes` (the autounmask accumulator) and the search bookkeeping (`depth`, `mask_steps`).

## struct BacktrackParams::missing_dep_masked
- Portage's `dep.parent not in _runtime_pkg_mask` check.
- Portage `backtracking.py::_feedback_missing_dep`: a dependency atom with no matching package makes `_backtrack_depgraph` mask that atom's parent and retry, so `dep_zapdeps` re-picks a `||` group whose chosen alternative had an unsatisfiable subtree.

## struct BacktrackParams::reverse_dep_masked
- See `reverse_dependency_constraints`.

## struct BacktrackParams::dropped_pins
- See `RevDepPin` and `build_residual_slot_conflicts`. A later pass picking a compatible version after all yields no record.

## struct BacktrackParams::reverse_dep_pins
- Backlog #90 (S2) + #92.

## struct BacktrackParams::slot_constraints
- Portage `_process_slot_conflicts` -- a single version can satisfy every parent atom that landed on the slot. Since C1 this map holds only the positive atom sets (C1 split).
- Portage `_emerge/resolver/backtracking.py`: the resolver runs the whole graph walk, and if the first pass hits a solvable slot conflict (Portage `_process_slot_conflicts`), it records the extra constraints and re-runs the entire walk from scratch. Keyed by `cat/pkg`, the union of every atom text that targeted a conflicted package.

## struct BacktrackParams::runtime_pkg_mask
- C1 split of the old combined map. Portage keys by package; the per-`cat/pkg` bucket keeps every reader's lookup shape.

## struct BacktrackParams::mask_order
- Portuale's buckets hash insertion order away; this recovers it. Appended exactly where a negative is first pushed (a reason replace keeps its first-seen position, like Portage's dict assignment).
- Backlog #129 (S1): first-insertion order of the `runtime_pkg_mask` negatives above, as `((cat, pkg), neg)` pairs across buckets. Portage's `_get_missed_updates` iterates the mask dict in insertion order and the notice renders in that order.

## struct BacktrackParams::mask_steps
- Mask-producing retries (slot-conflict choices, missing-dep parents). Portage's `--backtrack=N` bounds this per node (`Backtracker._add`), not the pass count; advanced by `Backtracker::feedback`, never in the walk.

## struct BacktrackParams::depth
- Every retry including config-growth passes. Portage's node depth; advanced by `Backtracker::feedback`, never in the walk.

## struct BacktrackParams::autounmask_use_config
- Applied by `backtracking.py::_feedback_config` on the next attempt. Written in-walk (see the A3 overlay); read back within the same pass.
- A dependency atom `X[flag]` that lands on an already-resolved slot whose package can't satisfy `[flag]` -- but a `package.use` flip could -- folds the flip in here (`(cat, pkg) -> {flag -> desired}`) and re-runs the whole walk, so the freshly-walked package's `flag?`-gated deps appear. Survives across passes like `slot_constraints`; converges when a pass adds nothing new.

## struct BacktrackParams::autounmask_use_change_records
- After the re-resolve the atom's `[flag]` is satisfied (by the accumulator) so nothing re-records it. Written in-walk (see the A3 overlay); read back within the same pass.

## struct BacktrackParams::autounmask_use_broke
- The same flag ends up wanted both on and off (`myparams["autounmask"] = False`). Written in-walk (see the A3 overlay); read back within the same pass.

## struct BacktrackParams::autounmask_suggest_flags
- Declared `mut` so the `_autounmask_breakage` fallback (Portage `depgraph.py:12262`) can switch all four off for one final clean pass once an accumulated autounmask change turns out to break another dependency.

## struct BacktrackParams::slot_operator_replace_installed
- Backlog #24 S3 (`depgraph.py:2400`/`2361`/`2881` write it, `resolver/backtracking.py::_feedback_config` 237-257 consumes it, `_gen_reinstall_sets` 5457-5480 turns it into the pseudo `SetArg` `__auto_slot_operator_replace_installed__` with `force_reinstall=True`, `reset_depth=False`). Added by `collect_feedback`'s scan; flips `AlreadyInstalled` to `Reinstall { slot_operator_rebuild: true }`. Portage `_replace_installed_atom` (3059-3087) returns the available ebuild's `slot_atom`; portuale `resolved_slots` is keyed by `(cat, pkg, slot)` but a dependency walk never pulls two slots of one cp.

## struct BacktrackParams::slot_operator_forced_upgrade
- Backlog #236 A (Portage `_emerge/depgraph.py:2400-2452`): the installed child joins the replace set there, and `@__auto_slot_operator_replace_installed__`'s `force_reinstall` (installed instances excluded from selection) makes the restart pick the probe's candidate. Portuale's replace-set flip reinstalls the same version. Filled by `slot_operator_update_force_scan`.

## struct BacktrackParams::slot_operator_undone
- Backlog #24 S4. Its vdb `:S/SS=` binding is still stale -- that is why rule 8 demoted it. Unlike the caller-side `already` set (Portage has no analogue of this latch to compare -- its undo mutates the graph in place).

## struct BacktrackParams::prune_rebuilds
- Backlog #213 (v2 `#24d`): Portage restarts with `config["prune_rebuilds"]` when `_slot_operator_replace_installed` meets missed updates (`_resolve`, `depgraph.py:5763-5780`; `_ENABLE_PRUNE_REBUILDS` `:628`, always true upstream; the `prune_rebuilds` backtrack parameter `:710`, consumed by `resolver/backtracking.py::_feedback_config:247-257`). Portage `backtracking.py:76`.

## struct BacktrackParams::circular_dependency
- Backlog #221: Portage `backtrack_infos["config"]["circular_dependency"]` (`_emerge/depgraph.py:10270-10287`, written by `_serialize_tasks` when no leaf node selects and consumed by `resolver/backtracking.py::_feedback_config` on the restart). Portage `circular_dependency.setdefault(node, set()).add(circular_child)`; Portage `dep_zapdeps` (`lib/portage/dep/dep_check.py:673-691`) demotes any `||` alternative with an atom matching such a child (`circular_atom`) to the `other` bin (bugs 703440, 705986). See `slot_operator_replace_installed`'s narrowing note. Portage's `unsolved_cycle`. A grown map is a distinct search state, or the retry node would dedup against its parent. Backlog #216's fresh-self-loop records share this map (same rotation, `cycle_restartable`'s scope gate in `collect_feedback`): one field, structured children, both triggers.

## struct CircularDepChild
- Backlog #221. `match_from_list` on the assembled candidate string, the same shape `best_installed_matching` matches installed candidates with. Carried into `kept_alt_branches` / `resolved_dep_targets` display derivations.
- Backlog #221: one in-cycle dependency of a `circular_dependency` parent above -- Portage's `circular_child` package object, reduced to the fields `atom.match(child)` needs: the child's own `cat/pkg-version` plus the slot/sub-slot an atom's `:slot` pin would match against.

## fn BacktrackParams::union_constraints
- Matches the old combined map exactly (see the `!`-prefix handling in `atom_currently_satisfiable` and `resolve_pretend`).

## fn BacktrackParams::masked_negatives_for
- Used by the missing-dep trigger.

## fn circular_child_candidate
- The `cat/pkg-version[:slot[/sub]]` candidate string Portage's `atom.match(circular_child)` sees, shared by the walk's demotion and the merge-order suppression's (see `suppressed_alt_edges`): the two must agree on what matches, or the re-derivation keeps the branch the walk dropped and a phantom edge re-closes the cycle.

## field BacktrackParams::rebuild_if_replace_installed
- Backlog #394: the `--rebuild-if-*` twin of `slot_operator_replace_installed` -- installed parents the post-walk scan named behind a merged build dep. Seeded and flipped exactly like the slot-operator set (see `seed_queue`, `outcome_fallbacks`), but the flip is a reasonless `Reinstall` (`slot_operator_rebuild: false`; the red `r` comes from `GraphResult::rebuild_if_reinstalls`). Portage: `_rebuild_config.rebuild_list` + `__auto_rebuild__` (`depgraph.py:5460-5480`, `force_reinstall=True`, `reset_depth=False`). Monotone; participates in `params_equal` like the slot-operator set.

## gate threading (#571)
- `SlotConflictMaskChoicesArgs` carries the gate; missed-update sibling enumeration skips unlisted categories.
