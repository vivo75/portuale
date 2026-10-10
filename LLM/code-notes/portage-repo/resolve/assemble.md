# crates/portage-repo/src/resolve/assemble.rs
Assembling the final result.

## fn assemble_result::backtrack_restarts
- Backlog #59 S2: the search's restart count (`passes - 1` inside `backtracking_resolve`'s loop).

## fn assemble_result abi-rebuild fallback
- Portage depgraph's slot-operator auto-rebuild (backlog #24 S3): the consumers themselves are walked nodes now (seeded + flipped in `run_pass` from `params.slot_operator_replace_installed`). `collect_feedback` already ran the scan for every pass it decided.
- Backlog #268: raw request atoms for the #252 gate.

## fn assemble_result continuity pairs
- Backlog #214 (v2 `#24e`): continuity pairs for conflict-fired rebuilds (see `slot_conflict_abi_display_pairs`) -- shapes the scan already covers render exactly once.

## fn assemble_result rebuild-if entries
- Same `all_installed_packages` scan / entry shape as the slot-operator pass.

## fn assemble_result resolution-walk dump
- Emitted here (final entries, successful pass) rather than threaded through the BFS (see `resolver_trace::dump_resolution_walk`).

## fn assemble_result merge-order sort
- Backlog #278: Portage `get_cycles` iterates digraph insertion order.

## fn assemble_result buildpkgonly check
- Portage `depgraph.py:5706-5717` (see `GraphResult::buildpkgonly_deps_unsatisfied`). Backlog #72 B3.
- Backlog #143 S1: Portage's hard blocker edge defeats the all-zeros check the same way (upstream pg1 A/B: the three rows print, then the two `!!!` lines, rc 1). After the S1 gate the only uninstalls that can exist under `buildpkgonly` are strong-buildtime ones, exactly Portage's abort shape.

## fn assemble_result hard cycles
- `topological_merge_order` already left such a cycle in discovery order (it can't linearize one).

## fn assemble_result restart cycles
- Backlog #221: either backtracking never ran (`--backtrack=0`, where Portage fails instead of retrying: pg5's runtime-closing ring) or a retry ran and the demotion did not break it (Portage's `unsolved_cycle` -- e.g. a single-branch `||` closing the ring, demoted but still picked as last resort). Portage's own error block covers these (its `get_cycles` runs over the `medium_soft` rung, which keeps unsatisfied runtime edges). Same precedence as `collect_feedback`'s own trigger; with backtracking on, a clean pass carrying a trigger cycle always retries out of `collect_feedback`, so a settled one either broke the ring (nothing to report) or is unsolved (recorded map).
- Backlog #278: this leg stays on the old lowest-index rotation on purpose -- it only fires when the hard reporter above is empty (persisting soft-containing rings Portage prints from its own fuller-graph `shortest_cycle[0]`), and rotating those to entries order would move pins (e.g. `sbrA`) Portage grounds elsewhere. It also dedupes the one-cycle-per-hard-edge repeats into a single ring.

## fn assemble_result cycle enumeration
- Portage `circular_dependency_handler`, fed by `get_cycles` over the `medium_soft` rung. Only a reported hard cycle consumes either number.

## fn assemble_result autounmask-use records
- Backtracking slice: recorded during the already-resolved-slot re-check (see `autounmask_use_change_records`) -- the "necessary to proceed" block.

## fn assemble_result autounmask coalescing
- Portage `_display_autounmask` iterates `_needed_use_config_changes.items()` (one entry per `pkg`) and joins every flipped flag of that pkg into a single `>=<cpv> flag -flag …` line, in the order the flips were discovered. E.g. `>=dev-qt/qtbase-6.11.1` needing both `cups` and `vulkan` (each from a different consumer) landed on two lines. Join tokens, drop a flag already present.

## fn assemble_result USE-line refresh
- Portage `_pkg_use_enabled` consults `_needed_use_config_changes` for the display regardless.

## fn assemble_result residual slot conflicts
- Portage's nomerge-node collision report. Backlog #57: the keeper-reachable triangle (othermod's `<2.0` against the merged 2.0, and keeper's dropped pin against the same pair). Portage renders one block whose installed side lists both parents -- exactly the residual record, which carries the dropped consumer pins on top of the same hard pullers.

## fn assemble_result withhold rows
- Backlog #90 (S2) + #92: same shape, same rc 0; the renderer cannot tell the producers apart.
- Backlog #209: Portage never reports the slot-operator path -- the minimizer/probe selection writes no `_runtime_pkg_mask` and no `_conflict_missed_update`, so `_show_missed_update` (`_emerge/depgraph.py:1566`) has nothing to render (the r25 shape merges silently at `backtrack: 0/20`, both bound forms of the consumer's pin). A plain-pin withhold still synthesizes its rows: Portage reaches it through the slot-conflict solver, whose "Record missed updates" tail (`:2089-2110`) runs with no `_allow_backtracking` gate, so the notice prints even with backtracking off (the #90 S2 two-target cell pins it).
- Withhold rows: alphabetical by package, then atom and consumer. Backlog #270: like Portage's `_conflict_missed_update` dict (insertion order of the solver's removals, `depgraph.py:1529-1565`), not alphabetical.

## fn assemble_result masked/use/plain-miss chains
- The atom+masked data was recorded at each `NoVisibleCandidate` push. Backlog #20 for the `[use]` disclosures.
- Backlog #135 (d): the binary-dep path creates NVC entries at the merge-entry push, where the disclosure block does not run -- the reason `--usepkgonly`'s `newpkg` miss used to fall through to the bare line with `plain_miss_deps` empty.
- Backlog #195 (round 2): Portage drops the collected item from `_unsatisfied_deps_for_display`. Invariant: the `parent_flip_recorded` latch is only ever inserted on the record-and-fail path, after `suggested_parent_use_candidate` returned `Some` -- so every latched key has `parent_use_suggestion.is_some()`, and the latch-keyed walk-time gate suppresses exactly the entries these `is_some()` gates suppress. One corner breaks the converse: when the re-probe fails after the suggestion returned `Some` (the `NoVisibleCandidate` break in the `'parent_flip` arm), the entry keeps `Some` with no latch -- walk-time records a miss block for it, but the sweep and the abort renderer still skip it. Benign (no corpus cell hits it) and deliberate: the flip genuinely did not rescue the dep.
- Backlog #135 (e); backlog #248 (same no-start-node rule as the USE fill).

## fn assemble_result outcome classification
- Backlog #19 Slice 3 (see `abort_outcome`). Backlog #193: Portage's `_virt_deps_visible` recursion over the selected providers; like `find_hard_cycles` it is unconditional -- its own cheap gate (no merge-bound virtual, no probe) keeps ordinary resolves free.

## fn assemble_result missed-updates gate
- Backlog #129 (S1): Portage `display_problems` (`depgraph.py:11127-11132`) -- slot collisions or unsatisfied blockers hide missed updates as possibly irrelevant.
- Portage chains the mask dict before the handler removals (`:1533-1536`); both sources collapse per `(root, slot_atom)` keeping the highest missed pkg (`:1553-1562`, see `collapse_skipped_updates`); direct-solve rows keep first-seen block order.
- Backlog #292: portuale restarted over a conflict Portage settles in its in-pass solver, so the same record sits in both sources. Rows inside the slot keep their order.

## fn assemble_result autounmask_no_clean_tail
- Backlog #217: the Settle/Feedback params carry the pass's overlay union (see `collect_feedback`'s `grown`) -- i.e. Portage's final pass grew `_needed_use_config_changes` for a digraph node and wanted a restart instead of reaching the `_success_without_autounmask` tail -- or the pass rescued a failed dep via a parent USE flip (Portage `_apply_parent_use_changes` collecting post-failure).
