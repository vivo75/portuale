# crates/portage-vdb/src/sqlite/write.rs
`SqliteDb`'s write side: `SqliteTxn` and the derived tables.

## module
- The write side landed as plan step S2.5; see the crate module doc, "Write transactions", for the model.

## fn SqliteTxn::seal_entry
- Value shaping is lossy, like Portage's consolidation (`errors="replace"`, #309).

## fn derive_needed
- Ports Portage's `NeededEntry.parse`.
