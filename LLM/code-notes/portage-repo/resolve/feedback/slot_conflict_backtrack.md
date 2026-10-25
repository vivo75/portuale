# crates/portage-repo/src/resolve/feedback/slot_conflict_backtrack.rs
Mask-feedback arm for the first conflicting slot's ranked choices.

## fn step_slot_conflict_backtrack
- C3: Portage `_slot_confict_backtrack` (one node per ranked choice, first conflicting slot only -- Portage `_feedback_slot_conflicts` takes `conflicts_data[0]`). The old bundled trial (highest child + every lower-alternative puller) is gone: puller-masking now happens only through a later pass's own missing-dep feedback, exactly like Portage, where a downgraded parent is abandoned via its unsatisfiable atom rather than pre-masked.
- Novelty mirrors the old trial-dedup, compared on the full entry (neg + reason, the same granularity the node-dedup uses): at least one group member new to the search state. A repeat with recomputed parents is a distinct state and explores -- the mask budget still caps it.
- #297: Portage's `_process_slot_conflicts` also runs `_slot_operator_trigger_reinstalls` in the same pass, so the replace set rides the conflict's node (`infos` carries `config` and `slot conflict` together) instead of costing a separate restart once the masks settle.

## gate threading (#571)
- `SlotConflictMaskChoicesArgs` and both scan arg structs use `ctx.gate()`.
