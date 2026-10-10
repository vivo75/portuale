# crates/portage-vdb/src/redb_db.rs
The redb backend: tables, open, reads, and the generation-validated cache.

## module
- The redb backend (feat#157, backlog #305 S5.2: tables, open and the read side; S5.3: writes). Design: `docs/vdb_to_db.md` §4, §7.2.
- Landed as plan steps S5.2 (tables, open, read side) and S5.3 (writes, in `write.rs`).
- `entry` keyed by `(state, category, pf)`: the interface has no `pn`/`ver` split (lib.rs module doc, item 1); a pending entry beside a live row (item 3, sqlite's `UNIQUE (category, pf, state)`).
- `entry_by_counter` keeps the first claimant on duplicate counters because a corpus can hold them.
- The writer bumps `generation` on every effective commit (sqlite's policy).

## fn RedbDb::reverse_dependents
- A superset of the true dependents (lib.rs module doc, item 8); the unsure marker comes from plan step S8.1.

## fn RedbDb::owners
- Plan step S8.2.

## tests: seeders
- Raw table writers bypass the production writer (the S5.3 writer); derived rows are filled as a production write would.

## tests: reverse_dependents
- Only entries whose deps carry an unclassifiable token (here the non-atoms "x" and "y", the unsure marker) come back (S8.1).

## tests: cache
- A commit that changes the data and bumps the generation (the S5.3 writer's contract) is seen without explicit invalidation.

## tests: rebuild_index
- Damaged index rows (a missing owner, a foreign owner, the unsure `dep_atom` marker an older build never wrote, a junk `needed` row) give wrong answers until `rebuild_index` restores exactly the rows the merge wrote (plan step S8.3).

## tests: write side
- Mirrors sqlite's write-side tests (plan step S2.5).
