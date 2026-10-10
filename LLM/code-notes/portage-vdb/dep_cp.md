# crates/portage-vdb/src/dep_cp.rs
Conservative `category/package` extraction behind the `dep_atom` prefilter index.

## module
- Moved out of `sqlite/write.rs` unchanged (S5.3).
- `dep_atom` holds a superset of the dependency atoms, found without `portage-dep` (vdb lib module doc, item 8). The caller of `reverse_dependents` still does the USE reduction and atom match.

## const UNSURE_CP
- S8.1: every token that is neither structure nor read by `dep_cp` is stored with `cp = ""`, and `reverse_dependents(cp)` returns entries with a row for `cp` or for `""`, so an entry whose deps could not be fully indexed is never lost. A database written before S8.1 has no markers and needs its index rebuilt (S8.3).

## fn dep_index_key
- `Some(UNSURE_CP)` is for a token that might still be an atom the matcher accepts (a wildcard, a bare versioned name, an odd version).
