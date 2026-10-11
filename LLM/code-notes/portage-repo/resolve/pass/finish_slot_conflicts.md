# crates/portage-repo/src/resolve/pass/finish_slot_conflicts.rs
Merge, solve and synthesize the pass slot conflicts.

## fn finish_slot_conflicts
- Backlog #90 (S2): Portage `_process_slot_conflicts` solves first, then `_validate_blockers` (`depgraph.py:2116-2125`). The common conflict-free pass pays nothing; rc stays 0 when no `SlotConflict` survives a removal, untouched conflicts keep their #62 rc 1.
- Backlog #270: a lone record misses parents walked later (a sibling pulling the same cp after detection), and the direct solve would remove an instance Portage keeps forced (`mlocaml`/`mllabltk`: Portage reports the unsolvable conflict, `depgraph.py:1858-1870`). Portage's solver reads the final `_parent_atoms`; at `--backtrack=0`, where the built slot-operator-trigger exemption no longer hides such records (see the walk's conflict arm), the lone records are re-filed from the live pullers like the merged ones.

- Backlog #80: unresolved rows whose owner has no display entry ride out on the result (`orphan_blockers`).
- Backlogs #90 (S2), #142 (S2 F1): one joint record per slot, so the solver keeps the jointly-satisfying version; removals ride `skipped_updates` to the renderer.
- Backlog #148: display-complete, walk-light rows.

