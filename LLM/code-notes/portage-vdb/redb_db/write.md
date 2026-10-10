# crates/portage-vdb/src/redb_db/write.rs
`RedbDb`'s write side: `RedbTxn` and the derived multimaps.

## module
- The write side landed as plan step S5.3.
- Derived rows keep the invariant "rows = f(files)": removing them re-derives from the stored file first.
- The unsure `dep_atom` marker (key `""`) comes from plan step S8.1.

## fn RedbTxn::seal_entry
- Value shaping is lossy, like Portage's consolidation (`errors="replace"`, #309).
