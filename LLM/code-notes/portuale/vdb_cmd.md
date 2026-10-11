# crates/portuale/src/vdb_cmd.rs
The `vdb` applet: convert, verify, status, sweep, rebuild-index, import-stores and mount.

## module
- Origin: feat#157 / backlog #305, plan S2.7 (convert and verify); `mount` serves a read-only FUSE view of the database (S7; the view is `vdb_view.rs`, the `fuser` adapter `vdb_fuse.rs`; feature `vdb-fuse`).
- `rebuild-index` (S8.3) recomputes a database's derived index (`owner`, `dep_atom`, `needed`) from the stored entry files in one transaction.
- `status` lists the entries left mid-merge (S4.3, design §9.1): `merging` rows on a database, `-MERGING-<pf>` directories on files.

## fn import_stores
- The counter becomes the larger of the two, so it never goes back (#317 S4).

## tests: import_stores_copies_the_files_stores_and_keeps_the_larger_counter
- #317 S4: `import-stores` copies the five stores Portage's emerge keeps outside the VDB tree into a database; the counter never goes back.

## tests: rebuild_index_counts_agree_across_databases
- S8.3: `rebuild-index` on a converted fixture VDB reports the same row counts on sqlite and redb, leaves the entries equal to the files source, and refuses files and a missing database.
