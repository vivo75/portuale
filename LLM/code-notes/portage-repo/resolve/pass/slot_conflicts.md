# crates/portage-repo/src/resolve/pass/slot_conflicts.rs
Solves the pass's slot conflicts directly where one instance can go, and drops the loser.

## fn solve_pass_slot_conflicts
- A removal with no keeper is the degenerate all-unforced shape (no notice was recorded for it either); the row is deleted outright.
- The removed version's own walked deps stay attached: all S2 oracle removals are leaves; a non-leaf removal keeps stale deps (see `direct_solve_slot_conflicts`' repoint arm).
- An installed keeper becomes `AlreadyInstalled` like Portage's kept installed node.
- History (#142 S2 F3): the F2 owner-version gate files exactly the keeper's blockers and drops the losers'.
