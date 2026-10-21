# crates/portage-vdb/src/frozen.rs
Per-thread "the VDB does not change now" scope (backlog #441 S1 / #457).

## fn freeze
- `backtracking_resolve` holds one for the whole resolve. Profile of `emerge -uDpvN @world` (2026-10-21): 15% of the resolve was `statx` from `FilesDb::aux_get_field` (one per read, as memo validity) and `category_generation` (`installed_candidates`, `libc_provider_cps`).
- Inside a scope each memo entry (`AUX_CACHE`, `GEN_MEMO`) is stamped with the epoch it was validated in and served without a `statx` while the epoch is unchanged. Outside a scope (`epoch() == 0`) behaviour is exactly the old always-revalidate.
- A resolve never writes the VDB. A write transaction on the same thread (`FilesTxn::new`, `commit`) calls `invalidate()`, which starts a new epoch, so a write inside a scope cannot be served stale.
- Staleness answer for the global-memo question in the S1 notes: no global key is needed; the scope is the answer. A concurrent external edit during one resolve is not seen, which is also what Portage's own per-run vartree caches do.
- Thread-local on purpose: a worker thread without a scope keeps revalidating.
