# crates/portage-repo/src/resolve/pass/slot_conflicts.rs
Solves the pass's slot conflicts directly where one instance can go, and drops the loser.

## fn solve_pass_slot_conflicts
- A removal with no keeper is the degenerate all-unforced shape (no notice was recorded for it either); the row is deleted outright.
- The kept instance takes over the removed instance's merge row (Portage merges it; portuale holds one row per cp). Repointing keeps `required_by`, merge order and USE wiring stable. Per-flag `(flag, enabled)` pairs (the entry shape), not the `(VAR, body)` display pairs: vdb record for an installed keeper, tree candidate + effective USE for a merge keeper.
- The removed version's own walked deps stay attached: all S2 oracle removals are leaves; a non-leaf removal keeps stale deps (see `direct_solve_slot_conflicts`' repoint arm).
- An installed keeper becomes `AlreadyInstalled` like Portage's kept installed node.
- History (#142 S2 F3): the F2 owner-version gate files exactly the keeper's blockers and drops the losers'.

## gate threading (#571)
- `DirectSolveInput`, both `slot_conflict_flag_sets` sites and the pool use `ctx.gate()`.

## fn solve_pass_slot_conflicts (backlog #602 signal)
- Before the direct solve, a non-empty conflict list with `backtrack_max > 0` sets `ResolveCtx::slot_conflict_seen`. It is set here, not from the driver's `pass.slot_conflicts`, because a conflict this function solves directly never reaches the driver, and Portage enters complete mode for it all the same (`_resolve_conflicts` looks at the package tracker before `_process_slot_conflicts`). `backtracking_resolve` copies the flag into `GraphResult::slot_conflict_backtracking`.
