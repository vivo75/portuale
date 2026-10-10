# crates/portage-repo/src/tests/feedback.rs
`collect_feedback` and dep-chain fill unit tests: prune triggers, overlays, pins.

## fn slot_operator_prune_rebuilds_fires_on_conflict_source_only
- Backlog #253 (half 1): the prune trigger chains the conflict half of Portage's missed updates (`_get_missed_updates`, `depgraph.py:1529-1565`, the `:1533-1536` chain).
- At this layer the conflict half rides `pass.skipped_updates` (the direct solve's removal rows, filed before the trigger runs) while the mask half comes out of `backtrack_missed_updates`; a conflict-source-only pass (non-empty replace set, no live conflicts, mask half empty, direct rows present) must take the prune restart exactly like a mask-half one.
- RED before the predicate chains both sources (the pass settles after the replace-growth restart instead).

## fn slot_operator_prune_rebuilds_stays_quiet_without_missed_updates
- Backlog #253 (half 1, no-over-fire guard): with neither half present the trigger stays quiet; a replace set alone settles.

## fn collect_feedback_folds_the_use_overlay_without_duplicating
- Backlog #161 S4: appends new flips and records while an exact `(atom, token)` duplicate stays single.

## fn collect_feedback_solves_a_two_want_conflict_into_constraints
- Backlog #161 S4: two want-atoms a single version satisfies become that package's retry constraints (Portage's solvable-conflict re-resolve).

## fn collect_feedback_keeps_a_single_want_out_of_constraints
- Backlog #161 S4: a lone want-atom is not a solvable conflict (the guard needs two), so the pass falls through to the mask trial instead of constraining a retry.

## fn collect_feedback_does_not_duplicate_logged_constraints
- Backlog #161 S4: retry constraints append only the missing wants; an already-logged atom is not pushed twice.

## fn collect_feedback_reports_no_rebuilds_when_the_pass_is_stuck
- Backlog #161 S4: with the search present but unsatisfiable, the slot-operator trigger stays off and the pass records an empty rebuild list instead of scanning.
- Setup is reachable (so a retried scan would find the stale consumer) but excluded (so the reverse-dependency scan stays quiet); the legs isolate the slot-operator gate.

## fn collect_feedback_keeps_an_enforced_pin_single
- Backlog #161 S4: an enforced reverse pin is retained once. A reachable consumer's runtime pin on a version the tree still carries (`souprov-1.0` stays satisfying) is enforced (holdable), not dropped for the residual report. Append leg (fresh params) and dedup leg (pre-retained pin, latched constraint keeps the pass quiet) share the setup.
- Backlog #209: the enforced-pin feedback travels as `RevDep` (same working copy as `Config`; the driver feeds it back in-pass under `--backtrack=0`).

## fn collect_feedback_collects_a_dropped_pin_once
- Backlog #161 S4: a dropped reverse pin is collected once. The `pinner`/`souprov` shape with a hard upgrade requirement drops the pin for the residual report. Three legs isolate the three sites: the conflict-gated first scan (append; early return isolates it), its dedup (a lone want falls through to the mask trial), and the unconditional second scan (dedup after mask-trial fall-through, with the mask trial's entries pre-covered so the pass settles).

## fn stale_trigger_falls_back_to_the_first_requirer
- Backlog #205 R4, stale-trigger leg: when the recorded trigger is no longer a requirer (masking retry, parent-flip rescue), the fill falls back to the legacy first-requirer start.
- Direct `fill_use_change_chains` shape: `abk0c` is required by `abk0a` (which ascends to the `abk0d` argument), while the recorded trigger names an `abk0d` that requires nothing here (stale).
- The stale-trigger leg: when the recorded trigger is no longer a requirer (masking retry, parent-flip rescue), the fill falls back to the legacy first-requirer start. `abk0c` is required by `abk0a` (which ascends to the `abk0d` argument), while the recorded trigger names a `abk0d` that requires nothing here -- stale.

## fn use_change_on_an_argument_package_yields_only_the_argument_row
- Backlog #248, expected value from Portage: a USE change on an argument package (no requirers) yields exactly the `(argument)` row; Portage's `_get_dep_chain` never prints the start node.
- Grounded in a fresh-host 3.0.82.2 staged-fixture probe 2026-09-29 (`emerge --pretend dev-libs/useflagpkg[-foo]` prints only `# required by dev-libs/useflagpkg[-foo] (argument)`). Direct `fill_use_change_chains` shape: the `useflagpkg` entry requires nothing, so there is no forcing parent to start from.

## fn keyword_change_on_an_argument_package_yields_only_the_argument_row
- Backlog #248, expected value from Portage: keyword twin of the above (same fresh-host probe family as the contract pin: `emerge --pretend --autounmask dev-libs/autounmaskkeywordpkg` prints only `# required by dev-libs/autounmaskkeywordpkg (argument)`).
