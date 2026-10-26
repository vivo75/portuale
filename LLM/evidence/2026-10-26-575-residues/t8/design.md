# Task 8 — design for the resolver half of #580 (written 2026-10-26)

Inputs: `oracle.md` (Portage 3.0.83 trace), `premise.md`, `pins.md`,
`targets.md`, and the portuale tree at `7eba6c92`. Abbreviations: D, B, DC
and PT as in `oracle.md`.

## What Portage does (one mechanism, two parts)

**M1: the installed reverse-dependency consumer's `||` goes through
`dep_zapdeps`, and only the selected arm becomes a parent atom.** Portage
walks a reachable installed consumer as a graph node. Its `||` groups are
decided by `dep_zapdeps` (DC:339-818):
- in complete mode, against `_graph_trees` (D:8704-8710, D:6009-6016).
  There the package tracker hides an installed package that a same-slot
  merge node replaces (PT:230-252, `_replaced_by` PT:140-143), and
  "installed" means "present after the merge" (DC:603-616, DC:713-736);
- in the normal walk, with the downgrade guards `conflict_downgrade` and
  `installed_downgrade` (DC:480-490, DC:531-541, DC:634-635).

The preferred bins are one list (DC:379-380). Inside a bin the listed order
holds, apart from the promotions at DC:745-809. Only the selected atoms reach
`_add_parent_atom` (D:4151) and `_add_slot_operator_dep` (D:4158). An arm
that is not selected never pins anything.

**M2: a consumer pin the merge contradicts is a slot conflict, and it goes
through `_slot_confict_backtrack`.** `_process_slot_conflicts` (D:2123) →
`_solve_non_slot_operator_slot_conflicts` (D:1789; the solvable case) →
`_process_slot_conflict` (D:2149) → `_slot_confict_backtrack`
(D:2220-2304):
- one choice per conflicting package plus the in-graph `existing_node`
  (D:2227-2231);
- lower versions first (D:2235);
- similar packages grouped into one choice (D:2248-2264);
- choices sorted by conflict-atom count (D:2273).

Then `_feedback_slot_conflict` (B:185-195) makes one backtrack node per
choice. The masked version is skipped at selection (D:7910). The puller's
missing dependency is the second hop (D:3517 → B:197-206). A run where every
choice dead-ends falls back to `get_best_run` (B:287-298, D:12420-12439).
A pass that ends with a missing dependency returns before slot conflicts are
processed (D:5691-5696). Autounmask and REQUIRED_USE changes break the loop
(`need_config_change`, D:11838-11885, D:12407-12411).

## What portuale has today

- **The scan.** `reverse_dependency_constraints` → `constraints_from_consumer`
  (`resolve/rev_dep.rs:493`) flattens each installed consumer's dep string
  (`flat_dep_atoms`). It then drops the arms `or_branch_vdb_deselected_set`
  demotes (`reverse_dep_pins.rs:317`: keep the arm that matches an installed
  instance, demote its siblings). Every other atom becomes a `RevDepPin`:
  enforced when it can be held, dropped (`dropped_pins`) when a hard want
  contradicts it.
- **The single-choice heal.** `dropped_pin_mask_restart`
  (`reverse_dep_pins.rs:50`, called from `step_reverse_dep_pins` ~:575)
  builds the residual records (`build_residual_slot_conflicts`). For the
  first contradicted merge version it files **one** `MissingDep`-variant
  mask with `MaskReason::SlotConflict`, behind G0 (pass doomed), G1
  (merge-bound third-party puller) and G2 (`or_branch_superseded`).
- **The existing ports this design reuses:**
  - `disjunction_preference` (`resolve/or_preference.rs:168`), the shared
    `dep_zapdeps` bin port, with the downgrade guard
    `alternative_downgrade_demoted` (:26);
  - `slot_conflict_mask_choices` (`resolve/backtrack_params.rs:47`) and
    `step_slot_conflict_backtrack`
    (`resolve/feedback/slot_conflict_backtrack.rs:7`), the
    `_slot_confict_backtrack` port with one node per choice;
  - `step_missing_dep` (`resolve/feedback/missing_dep.rs:7`);
  - `Backtracker::get_best_run` (`resolve/backtracker.rs:151`).
- **Order.** `collect_feedback_inner` (`resolve/feedback/mod.rs`) runs
  `step_slot_conflict_backtrack` (:325) **before** `step_missing_dep`
  (:340). Portage's order is the reverse (D:5691-5696). G0 hides that
  difference for the rev-dep heal only.

## The port

**P1 (M1), the consumer's `||` selection.** In `constraints_from_consumer`,
replace the `or_branch_vdb_deselected_set` call with a selection by
`disjunction_preference` over the consumer's reduced `DepTree`
(`reduced_dep_tree`, the same reduction the scan uses). The view is the
post-merge one:
- an arm is available when a merge-bound entry matches it, or an installed
  instance that no same-slot merge-bound entry replaces matches it (PT:230-252);
- the bins and the listed order are the existing port's;
- the downgrade guard applies as it does in the forward walk.

The selected arm's atoms are the only ones that go on to
`constraints_from_atom`. The same selection replaces the call at
`slot_operator/installed_parent_update_probe.rs:129`; for the probe the
view is the pass graph, as Portage's `_slot_operator_deps` only hold
selected atoms (D:4158, D:3104). Interface:
`pub(crate) fn installed_consumer_or_selection(args: InstalledConsumerOrSelectionArgs<'_>) -> HashSet<String>`
returns the arm texts *not* selected (verbatim plus constraint-normalised),
so that the two call sites swap one function for the other and nothing
else changes shape. Put it in `resolve/or_preference.rs`, next to
`disjunction_preference`.

Open point for Task 9 S0: whether `disjunction_preference`'s
availability probe can be given the post-merge view through
`DisjunctionPreferenceArgs` (`entries` + `constraints`) as it is, or needs a
`view: OrView` field (`Forward` | `PostMerge`). If it needs a field, the
forward walk passes `Forward` and its behaviour must stay byte-identical
(the full gate proves it).

**P2 (M2), the contradicted pin as a slot conflict.** Replace
`dropped_pin_mask_restart` with the slot-conflict route: a residual record
from `build_residual_slot_conflicts` whose merge version a dropped pin
contradicts becomes a `SlotConflict` in `pass.slot_conflicts`, with:
- the installed instance as the `existing_node`;
- the consumer pins (with `built_render` first, as #568) as the parents of
  the installed side, and the pullers as the parents of the merge side.

`step_slot_conflict_backtrack` then files it through
`slot_conflict_mask_choices`, one node per choice in Portage's order. The
puller re-pick comes from `step_missing_dep` on the next pass, as it does
for Portage. This removes the single-choice shortcut. G1's outcome then
comes from the dead-ending argument plus `get_best_run`, and the backtrack
counts become Portage's (`targets.md`, T3 and the k3/igw rows).

Open point for Task 9 S0: the record has to be formed **inside** the pass
(before `step_slot_conflict_backtrack` at `feedback/mod.rs:325`), not in
`step_reverse_dep_pins` (:344), which runs later. The early scan at the top
of `collect_feedback_inner` (the `if !pass.slot_conflicts.is_empty()` block)
is the natural place, widened to run when `dropped_pins` is non-empty.

**P3, pass order (the G0 part).** A pass that has a missing or masked
dependency does not feed back the slot conflicts that P2 forms, which is
Portage's D:5691-5696 for this conflict source. An autounmask or USE change
in the pass stops the heal as `need_config_change` does (D:12407-12411).
This is G0's predicate stated as Portage's rule, at the P2 site only. Moving
`step_missing_dep` ahead of `step_slot_conflict_backtrack` for **every**
slot conflict is the general form. It moves other L0 cells, so it is not
part of #580: file it under §R as its own item with its own L0 evaluation.

## The heuristics

| Heuristic | Verdict | Why |
|---|---|---|
| `or_branch_vdb_deselected_set` | **retired by the port (P1)** | It approximates M1 with an installed-preference rule. Portage picks by bins over the post-merge view, which gives a different arm on T1 and R4 (`oracle.md` C1a, C1c). |
| G2 (`or_branch_superseded`) | **retired by the port (P1)** | With M1 the stale arm never becomes a pin, so there is nothing for G2 to exclude (`oracle.md` C4). |
| G1 (`has_merge_bound_puller`) | **retired by the port (P2)** | Portage backtracks over the conflict and dead-ends on the argument. P2 plus `get_best_run` gives the same rows and Portage's counts (`oracle.md` C3). |
| G0 (doomed-pass gate) | **reduced to P3** | The rule stays, stated as Portage's order (D:5691-5696) plus the config-change break (D:12407-12411). The heal is deferred, not forbidden, as on the oracle's counter-shape (`oracle.md` C2). |

Retirement order for Task 10: `or_branch_vdb_deselected_set` and G2
together (one change, since both are M1 and G2 has nothing left to do once
P1 lands), then G1, then G0 → P3. P1 lands in Task 9 with the heuristics
still on. P2 and P3 land in Task 10, because they *are* the retirement of
G1 and G0.

## Not established (inputs for the re-review)

- Why Portage evaluates the consumer's `||` only in complete mode (observed,
  not traced to a line: `oracle.md` E).
- cgp (#222) and the bed's perl heals were not traced. On the bed P1 could
  change which arm pins when a virtual lists the installed arm second.
- `--dynamic-deps=n` in the rev-dep scan (`rev_dep.rs:557-561`) is not
  checked. The T1–T3 fixtures edit tree, cache and vdb together, so they
  do not depend on it.
- The trailer order and ` for <ROOT>` differences seen on the G0
  counter-shape (`oracle.md` C2) are display-only. File them under §R if
  they are not already open.
