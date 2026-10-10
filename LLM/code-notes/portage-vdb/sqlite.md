# crates/portage-vdb/src/sqlite.rs
The SQLite backend: schema, open, reads, and the `data_version`-validated cache.

## module
- The SQLite backend (feat#157, backlog #305 S2.3: schema and open; S2.4: the read side; S2.5: writes in `write.rs`, transaction model in the crate doc). Design: `docs/vdb_to_db.md` §4, §7.1.
- Landed as plan steps S2.3 (schema and open), S2.4 (read side), S2.5 (writes in `write.rs`).
- `meta.schema_version` is independent of `METADATA_FILE_FORMAT_VERSION` (plan section 1, Q4).
- WAL needs shared memory (design section 4).

## const SCHEMA
- Differences from `docs/vdb_to_db.md` section 7.1, all from the current interface (lib.rs module doc): `entry` keyed by `(category, pf)` (item 1), pending entries (item 3), per-entry `metadata` stamp state (item 12).
- `counter` is not unique because a corpus can hold duplicates.
- `UNIQUE (category, pf, state)` (a pending same-`pf` replacement is a `merging` row beside the `installed` row) came from a read-side finding; `preserved_lib.path` nullable and `world*.pos` exist because the conformance suite needs both round trips. `SCHEMA_VERSION` stays 1: nothing is released.

## fn load_live
- One `SELECT` over `entry` plus one scan of `entry_file`, joined by `entry_id`: a per-entry join does a B-tree seek per name and is 4x slower on a tree-sized VDB; the scan skips the large blobs' overflow pages.

## fn SqliteDb::generation
- `meta.generation` moves on every committed write (the S2.5 writer's contract).

## fn SqliteDb::read_file_at
- Incremental blob I/O would avoid reading the blob to cut it, but needs rusqlite's `blob` feature (deferred read-path work).

## fn SqliteDb::reverse_dependents
- A superset of the true dependents (lib.rs module doc, item 8); the unsure marker comes from plan step S8.1.

## fn SqliteDb::owners
- Plan step S8.2.

## tests: read side
- The read side (S2.4) is verified against `FilesDb`: the database is seeded with raw SQL from what a `FilesDb` shows, and every read is compared.
- A sealed non-UTF-8 field serves its lossy text, like an unsealed one (backlog #309).
- Only entries whose deps carry an unclassifiable token come back (S8.1).

## tests: rebuild_index
- Damaged index rows give wrong answers until `rebuild_index` restores exactly the rows the merge wrote; this includes the unsure `dep_atom` marker an older build never wrote. The read cache is validated by `PRAGMA data_version` (plan step S3.2).
