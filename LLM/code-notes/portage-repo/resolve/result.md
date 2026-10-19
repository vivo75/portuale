# crates/portage-repo/src/resolve/result.rs
The resolver's result and how a run aborts.

## struct ChangedDepsReportEntry
- See `resolve_pretend_graph`. Consistent with every other "no vdb-metadata reader" simplification in this crate (e.g. `enqueue_dependencies`).
- `--changed-deps-report`: an installed package, still in the graph at `version`, whose vdb-recorded dependency strings differ from the repo's current ebuild for that exact version (`deps_changed`) -- but reported, not reinstalled (see `resolve_pretend_graph`'s own doc comment).

## enum AbortReason
- Backlog #19, `docs/history/abort-path-spec.md`. Slice 3 produces it from the settled graph; later slices render it and give its error block.
- No "autounmask + cycle" variant: the oracle (spec §4d, `abort-au-cycle` / `abort-au-restart-cycle`) shows autounmask changes with a cycle only cut the backtrack loop at try 0 (`need_config_change`, `:11736-11760`); the list shown is still `_show_circular_deps`' stuck remainder plus the autounmask block.
- Why a resolve was abandoned (backlog #19, `docs/history/abort-path-spec.md`): Portage's `_create_graph` returns 0 the moment a required dep cannot be satisfied (`depgraph.py:3254-3271`) or `_serialize_tasks` gives up on an unserializable cycle (`:10262-10294`) — and `action_build` then runs `display_problems()` without ever calling `display()` (`actions.py:460-462`), so the merge list shown is at most a partial one and the exit status is 1.
- Original comment citations: `autounmask_*_changes`, per the oracle's spec §4d). Portage: depgraph.py _create_graph:3254-3271, _serialize_tasks:10262-10294; actions.py:460-462; backlog #19, docs/history/abort-path-spec.md

## variant AbortReason::MaskedDep
- Spec §4a.
- Original comment citations: A dependency atom matching masked-only ebuilds (spec §4a): the disclosure half

## variant AbortReason::UnsatisfiedAtom
- Spec §4b: nothing matches at all, or a `[use]`-dep mismatch no autounmask flip resolves (Portage's "there are no ebuilds built with USE flags" flavour).
- Original comment citations: A dependency atom no visible package satisfies for any other reason (spec §4b): Portage: depgraph.py:3479-3483 no-ebuilds-with-USE-flags flavour

## variant AbortReason::UnserializableCycle
- Spec §4c. Cumulative counters in the display.
- Original comment citations: `_serialize_tasks` drained everything serializable and got stuck (spec §4c): cut (dedup-by-design, Gate G0.2).

## variant AbortReason::VirtualCycle
- Backlog #193. Portage prints `!!! virtual cycle detected:` with one `  {cpv}::{repo}` line per probe-stack member (`select_files`, `:5002-5013`).
- Portage raises out of the walk (`_virtual_cycle_error`, `depgraph.py:6215-6216`), prints `!!! virtual cycle detected:` with one ` {cpv}::{repo}` line per probe-stack member (`select_files`, `:5002-5013`), and fails with no merge list.

## enum ResolveOutcome
- Portage shows no list at all for masked/unsat shapes.
- Original comment citations: falsy-success paths as data. The `Aborted` partial list is what Slice 4 renders Portage: depgraph.py:5676-5683

## fn abort_path_enabled
- Same pattern as `PORTAGE_SERIALIZE_FRONTIER_DISABLE` in `merge_order.rs`; avoids growing `ResolveRequest` and the ~60-arg `resolve_pretend_graph` marshaller.
- Original comment citations: Rollout gate for the abort path (Gate G0.4, flag-gated): unset or any value but `0` enables the Slice-5 abort rendering + exit codes; `PORTUALE_ABORT_PATH=0`

## fn abort_outcome
- Portage's `_backtrack_depgraph` vs portuale's `'backtrack` loop (which rebuilds `entries` per pass).
- Oracle `abort-masked-cycle` (no list, no circular block). Backlog #244 closed the cut where it shows: the `aub0` two-losing-deps shape the fixtures once lacked.
- Portage's `_select_package(without_use)` → rebuild-or-hard-abort (`:3473-3483`, `:6880-6900`); nothing-matches goes to the missing-dependency backtrack. Portuale used to rescue every installed-only NVC (no merge-bound requirer → `Complete`), which hid #132 on the live tree: the evaluated `-wayland` qtbase atom settled `NoVisibleCandidate` with only installed requirers, the block rendered, exit 0 where Portage aborts (`#111 S0`, `#132` arm D oracle `l0-fx-20260921T231520Z`). Oracled by the `abort-au-plain` capture (with `--autounmask-use` off); `_select_package(dep.atom.without_use)` succeeding still returns 0 (`:3479-3483`).
- `parent_cpv` has the same `cat/pkg-version` shape as `merge_bound_cpv`.
- Backlog #245/#287 for the dual-root index handling.
- Derives the abort outcome from the settled graph — Portage's three abandon sites as a post-loop classification, since portuale's `'backtrack` loop (unlike Portage's `_backtrack_depgraph`) rebuilds `entries` per pass and the final pass is the one Portage would display (spec §6, "the final (best-run-equivalent) pass deciding").
- Precedence follows Portage's control flow: a walk-time failure (`_add_dep` returning 0 → `_create_graph` 0 → `_resolve` returns before `altlist()`, `depgraph.py:5676-5681`) wins over a serialize-time one (the `_serialize_tasks` give-up inside `altlist()`), so a masked/unsat dependency beats a cycle in the same graph (oracle: `abort-masked-cycle` — no list, no circular block).
- Backlog #244 closed the cut where it shows: the autounmask-breakage clean pass (the `aub0` two-losing-deps shape the fixtures once lacked) is re-cut afterwards to Portage's fail-fast walk -- first failure, its own chain, only the slot conflicts admitted before it (`fail_fast_truncate`, `merge_order::replay_create_graph`).
- Its `_initially_unsatisfied_deps` path (`:3425-3437` complete-mode beyond-depth, `:3458-3470` masked-parent backward-compat, both "error message but do not cause the dependency calculation to fail") is narrow -- a use-constrained miss goes to `_select_package(without_use)` → rebuild-or-hard-abort instead (`:3473-3483`, `:6880-6900`), and a nothing-matches miss to the missing-dependency backtrack.
- Backlog #193: Portage's virtual-cycle failure is an exception raised out of the walk (`_virtual_cycle_error` propagating through `_resolve` to `select_files`, `depgraph.py:5002`), so it preempts every serialize-time failure the way a walk-time masked/unsat miss does -- checked before both.
- Original comment citations: pass is the one Portage would display, spec §6).

## field GraphResult::outcome
- Slice 3.
- Original comment citations: graph either way; the renderer switches to `partial` in Slice 4.

## field GraphResult::backtrack_restarts
- #59 S2 (K1 option (a)). `--json` only.

## field GraphResult::skipped_updates
- Backlog #90 (S2) + #92. `--columns` has no gate.
- Rendered after the merge list by `pretend.rs`, suppressed under `--quiet` unless `--debug` (Portage `_show_missed_update`); `--json` returns before the block and `--columns` has no gate.

## field GraphResult::skipped_missing_deps
- Backlog #129 (S1).

## field GraphResult::skipped_missing_dep_full
- Backlog #198.

## field GraphResult::autounmask_no_clean_tail
- Backlog #217. Portage's `_needed_use_config_changes` grown for a digraph node; `want_restart_for_use_change` (`:7719`); oracle `aucasctop` (prints the notice, `fixtures/abort-captures/`); `_apply_parent_use_changes` (`:5820`); `_have_autounmask_changes()`; the `_resolve` tail (`:5793`, "reserved for cases where there are *zero* other problems"); the early return (`:11713-11717`); oracle `abort-au-plain`.
- Backlog #217: the settled pass took an autounmask path on which Portage's final `_resolve` would NOT have set `_success_without_autounmask` -- so `need_config_change` (`depgraph.py:11708`) reaches its third branch (`:11752`) and sets `_autounmask_backtrack_disabled`, and the "terminated early" notice prints.
- Two shapes, both observed on the final pass: * a `package.use` flip was folded for an already-graphed package (the already-resolved-slot re-check or the parent flip with `--autounmask-backtrack=y` grew the settled `autounmask_use_config` accumulator -- Portage `_needed_use_config_changes` grown for a digraph node).

## field GraphResult::settled_with_backtracking
- Backlog #244. Portage's `--backtrack=30` hint (shown only when `not _allow_backtracking`).
- Backlog #244: the reported graph comes from a pass Portage runs with backtracking still allowed -- the loop ended early on `need_config_change` (`depgraph.py:12228-12234`) -- so Portage's slot-conflict advisory omits its `--backtrack=30` hint (shown only when `not _allow_backtracking`, `depgraph.py:1750-1759`).
- Original comment citations: Portage: depgraph.py:12228-12234,1750-1759

## field GraphResult::circular_deps
- Backlog #278: `discovery` order stands in for Portage's digraph insertion order; starts at Portage's `[0]`.

## field GraphResult::circular_dependency
- Backlog #221.
- Backlog #221: the settling pass's recorded cycle edges (Portage `backtrack_infos["config"]["circular_dependency"]`), empty when no backtrack restart happened.
- The caller (`pretend.rs`) feeds it back into the display-side `kept_alt_branches` / `resolved_dep_targets` derivations so `--tree` and the blocker disposition keep the cycle-breaking branch the walk took.

## field GraphResult::cycle_display
- Installed and unresolvable entries have no merge line to re-display. Portage's cycle-only `--tree` re-display minus tree nesting.

## field GraphResult::masked_deps / use_unsat_deps / plain_miss_deps
- Backlog #20 for the USE block. #135 (d) for the plain-miss block. Portage's precedence renders USE ahead of masked; the caller does the same.

## fn GraphResult::autounmask_backtrack_disabled
- Backlog #217. Portage's `_have_autounmask_changes`, `_allow_backtracking`, `--autounmask-continue` implying opt-back-in; set at exactly one site (`need_config_change:11752`).
- Backlog #217: Portage `_dynamic_config._autounmask_backtrack_disabled` as a pure predicate over the settled result -- the gate for `_display_autounmask`'s "backtracking has terminated early" tail (`depgraph.py:11093`, set at exactly one site, `need_config_change:11752`).
- Portage sets the flag iff autounmask changes exist (`_have_autounmask_changes`), backtracking is allowed (`_allow_backtracking`, i.e. `--backtrack` budget > 0) and not opted back in (`--autounmask-backtrack=y`, or `--autounmask-continue` implying it), AND `need_config_change` reaches its third branch: the early return (`:11713-11717`) did not fire -- neither `_success_without_autounmask` (the `_resolve` tail `:5793`, "reserved for cases where there are *zero* other problems") nor `_required_use_unsatisfied` (`:3652`) -- and the slot-conflict handler branch (`:11719-11736`) found no changes to suggest.

## struct AutounmaskChange
- Portage's `autounmask_unrestricted_atoms` defaults to `"n"`; `check_if_latest(check_visibility=True)` (bug #536392); `autounmask_unrestricted_atoms` for masks.

## field AutounmaskChange::trigger
- Backlog #205. Portage renders keyword/license/mask changes without the unsatisfied filter.

## field GraphResult::orphan_blockers
- Backlog #80: unresolved blocker rows whose owner has no display entry (scan-collected, absent from the graph).

## field GraphResult::buildpkgonly_deps_unsatisfied
- `--buildpkgonly`'s own Portage depgraph check (`lib/_emerge/depgraph.py:5706-5717`): `true` when some entry that would newly merge (`New`/`Upgrade`/`Downgrade`/`Reinstall` -- anything but `AlreadyInstalled`/`NoVisibleCandidate`) has a `required_by` owner that would *also* newly merge.
- Original comment citations: Portage: lib/_emerge/depgraph.py:5706-5717; display_problems

## field GraphResult::prune_unresolved
- Backlog #295: the best-run pass (Portage's `allow_backtracking=False` re-run) still carries a non-empty replace set with missed updates and no prune latch.
- Portage's `_resolve` (`depgraph.py:5763-5775`) then wants the `prune_rebuilds` restart, cannot take it, and returns failure: the graph is displayed and the run exits 1.

## field GraphResult::pprovided_atoms
- Directly-requested (top-level) atoms that matched a `package.provided` entry (`config.package_provided`) and were therefore *not* resolved -- Portage `depgraph.py:5497-5615`'s own `_pprovided_args`.
- The caller (`pretend.rs`) prints Portage `depgraph.py:11192-11235`'s `WARNING: … listed in package.provided:` block for these, in requested order.
- Original comment citations: Portage: depgraph.py:5497-5615 _pprovided_args; :11192-11235; dep_check.py:1052

## field GraphResult::missing_args
- Backlog #286: top-level `@world`/`@selected` set members that selected no package on the settled pass.
- Original comment citations: Portage: depgraph.py:5577-5605 _missing_args; display_problems

## field GraphResult::autounmask_keyword_changes
- Portage `--autounmask` keyword changes that were applied to make the graph resolve (Portage `depgraph.py::_display_autounmask`'s `unstable_keyword_msg`): one per entry that only became visible because `autounmask_suggest_keywords` let `resolve_pretend` accept a `KEYWORDS`-alone mask.
- The caller (`pretend.rs`) prints the `The following keyword changes are necessary to proceed:` block from these, after the merge list.

## field GraphResult::autounmask_use_changes
- Printed as the `The following USE changes are necessary to proceed:` block, after the keyword one.
- On by default (`autounmask_suggest_use`, unlike the keyword kind).

## field GraphResult::autounmask_mask_changes
- Printed as the `The following mask changes are necessary to proceed:` block, between keyword and USE.
- Off unless `--autounmask-keep-masks=n`. v1 cut: Portage's `# <filename>:` + masking-comment lines (no source-file provenance on portuale's `package_mask` list).

## field GraphResult::large_cycle_count
- Portage `circular_dependency_handler.large_cycle_count`: the full elementary-cycle enumeration (`merge_order::cycle_report`, Portage `digraph.get_cycles` over the `medium_soft` rung) counted more than three records.

## field GraphResult::masked_deps
- Masked-only dependency disclosures (Portage `_show_unsatisfied_dep`'s "All ebuilds that could satisfy … have been masked" block for a *dependency* atom): one per dependency `NoVisibleCandidate` entry whose atom still matches ebuilds by version, every one of which is masked.
- The caller (`pretend.rs`) renders each in place of the bare `!!! no visible ebuild for dependency` line, followed by the `(dependency required by …)` chain walked out of the final entries.

## field GraphResult::plain_miss_deps
- Plain-miss dependency disclosures (#135 (d), Portage's `emerge: there are no ebuilds to satisfy "<atom>"` block): one per dependency `NoVisibleCandidate` that is neither masked-only nor `[use]`-unsatisfied.

## field AutounmaskChange::atom
- Original comment citations: Portage: autounmask_unrestricted_atoms default n; check_if_latest (bug #536392)

## field GraphResult::rebuild_if_reinstalls
- Backlog #394: `(cat, pkg)` of the installed parents the `--rebuild-if-*` restart force-reinstalled (seeded parents that settled as merges). Sorted, deduped. Only the red `r` column reads it (consumer-only -- the merged dep stays untagged); unlike `abi_rebuilds` there is no "causing rebuilds" block. Portage: `_gen_reinstall_sets`; `output.py`, `PkgAttrDisplay.force_reinstall`.
