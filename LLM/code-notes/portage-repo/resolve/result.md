# crates/portage-repo/src/resolve/result.rs
The resolver's result and how a run aborts.

## struct ChangedDepsReportEntry
- See `resolve_pretend_graph`. Consistent with every other "no vdb-metadata reader" simplification in this crate (e.g. `enqueue_dependencies`).

## enum AbortReason
- Backlog #19, `docs/history/abort-path-spec.md`. Slice 3 produces it from the settled graph; later slices render it and give its error block.
- No "autounmask + cycle" variant: the oracle (spec §4d, `abort-au-cycle` / `abort-au-restart-cycle`) shows autounmask changes with a cycle only cut the backtrack loop at try 0 (`need_config_change`, `:11736-11760`); the list shown is still `_show_circular_deps`' stuck remainder plus the autounmask block.

## variant AbortReason::MaskedDep
- Spec §4a.

## variant AbortReason::UnsatisfiedAtom
- Spec §4b: nothing matches at all, or a `[use]`-dep mismatch no autounmask flip resolves (Portage's "there are no ebuilds built with USE flags" flavour).

## variant AbortReason::UnserializableCycle
- Spec §4c. Cumulative counters in the display.

## variant AbortReason::VirtualCycle
- Backlog #193. Portage prints `!!! virtual cycle detected:` with one `  {cpv}::{repo}` line per probe-stack member (`select_files`, `:5002-5013`).

## enum ResolveOutcome
- Portage shows no list at all for masked/unsat shapes.

## fn abort_path_enabled
- Same pattern as `PORTAGE_SERIALIZE_FRONTIER_DISABLE` in `merge_order.rs`; avoids growing `ResolveRequest` and the ~60-arg `resolve_pretend_graph` marshaller.

## fn abort_outcome
- Portage's `_backtrack_depgraph` vs portuale's `'backtrack` loop (which rebuilds `entries` per pass).
- Oracle `abort-masked-cycle` (no list, no circular block). Backlog #244 closed the cut where it shows: the `aub0` two-losing-deps shape the fixtures once lacked.
- Portage's `_select_package(without_use)` → rebuild-or-hard-abort (`:3473-3483`, `:6880-6900`); nothing-matches goes to the missing-dependency backtrack. Portuale used to rescue every installed-only NVC (no merge-bound requirer → `Complete`), which hid #132 on the live tree: the evaluated `-wayland` qtbase atom settled `NoVisibleCandidate` with only installed requirers, the block rendered, exit 0 where Portage aborts (`#111 S0`, `#132` arm D oracle `l0-fx-20260921T231520Z`). Oracled by the `abort-au-plain` capture (with `--autounmask-use` off); `_select_package(dep.atom.without_use)` succeeding still returns 0 (`:3479-3483`).
- `parent_cpv` has the same `cat/pkg-version` shape as `merge_bound_cpv`.
- Backlog #245/#287 for the dual-root index handling.

## field GraphResult::outcome
- Slice 3.

## field GraphResult::backtrack_restarts
- #59 S2 (K1 option (a)). `--json` only.

## field GraphResult::skipped_updates
- Backlog #90 (S2) + #92. `--columns` has no gate.

## field GraphResult::skipped_missing_deps
- Backlog #129 (S1).

## field GraphResult::skipped_missing_dep_full
- Backlog #198.

## field GraphResult::autounmask_no_clean_tail
- Backlog #217. Portage's `_needed_use_config_changes` grown for a digraph node; `want_restart_for_use_change` (`:7719`); oracle `aucasctop` (prints the notice, `fixtures/abort-captures/`); `_apply_parent_use_changes` (`:5820`); `_have_autounmask_changes()`; the `_resolve` tail (`:5793`, "reserved for cases where there are *zero* other problems"); the early return (`:11713-11717`); oracle `abort-au-plain`.

## field GraphResult::settled_with_backtracking
- Backlog #244. Portage's `--backtrack=30` hint (shown only when `not _allow_backtracking`).

## field GraphResult::circular_deps
- Backlog #278: `discovery` order stands in for Portage's digraph insertion order; starts at Portage's `[0]`.

## field GraphResult::circular_dependency
- Backlog #221.

## field GraphResult::cycle_display
- Installed and unresolvable entries have no merge line to re-display. Portage's cycle-only `--tree` re-display minus tree nesting.

## field GraphResult::masked_deps / use_unsat_deps / plain_miss_deps
- Backlog #20 for the USE block. #135 (d) for the plain-miss block. Portage's precedence renders USE ahead of masked; the caller does the same.

## fn GraphResult::autounmask_backtrack_disabled
- Backlog #217. Portage's `_have_autounmask_changes`, `_allow_backtracking`, `--autounmask-continue` implying opt-back-in; set at exactly one site (`need_config_change:11752`).

## struct AutounmaskChange
- Portage's `autounmask_unrestricted_atoms` defaults to `"n"`; `check_if_latest(check_visibility=True)` (bug #536392); `autounmask_unrestricted_atoms` for masks.

## field AutounmaskChange::trigger
- Backlog #205. Portage renders keyword/license/mask changes without the unsatisfied filter.
