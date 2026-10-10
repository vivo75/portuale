# crates/portage-vdb/src/files.rs
`FilesDb`: the historic `var/db/pkg` directory tree.

## module
- The read side of per-entry and `aux` reads (S1.2) is the former `portage-repo` code, moved unchanged: the same `openat`/`statx` sequence and the same in-process memo (thread-local, validated by the entry directory's `st_mtime_ns`).
- The merge's write side is in `files_write.rs` (S1.4: the pending entry, the counter, the preserved-libs registry and the config memory).
- `reverse_dependents` stays above this crate on `files` (crate module doc item 19; S1.5 moved `owners`, `read_file_all`, `delete_entry`, `replace_file`; S1.6 moved `world` / `world_sets`).

## fn dir_mtime_nanos
- Moved from `portage-repo::dir_mtime_nanos`. An external in-place rewrite of a file inside a running process is not caught -- the same in-process-cache property Portage's `vardb` dbapi has; no portuale writer does it.

## fn read_metadata_file
- Portage `vartree.py:115-187`. Other `#` lines ignored; a line without `=` skipped; `k, v = line.split("=", 1)` with the last duplicate winning.

## fn read_vdb_file
- Portage applies the same `" ".join(myd.split())` normalisation on this path (`vartree.py:1044-1046`), so the bytes are the same whether the snapshot validated or not.
- Invalid UTF-8 decodes lossy (`U+FFFD`), matching `encoding="utf-8", errors="replace"` (`vartree.py:1035-1039`), instead of collapsing to `""` (backlog #125, audit O15).

## fn is_valid_aux_slot
- `slot = [\w][\w+.-]*` ASCII (`versions.py:38`, `re.ASCII`); the `/sub` half iff the EAPI has `slot_operator` (`versions.py:76-90`). No EAPI parametrization inside the EAPI 5+ floor (`agent-context.md`: every live EAPI has `slot_operator`, `eapi.py:319`).
- An entry whose own `EAPI` is empty (for which Portage would use the single-slot shape after its `EAPI ""->"0"` step) is out of scope -- that translation is #115's documented cut, and no such entry has a portuale-visible consumer anyway (Phase 12 S0.3). An empty `SLOT` keeps #126's O5 caller contracts.

## fn translate_aux_slot
- Portage translates in the outer `aux_get` (`vartree.py:967-972`); here `aux_get_field` serves both roles, so it lives here, for the `SLOT` key only.

## fn aux_get_field (`FilesDb`)
- Portage `_aux_get(cpv, wants, st)` (`vartree.py:975-1053`), moved from `portage-repo::vdb_aux_get`. Costs one `statx` of the entry directory on a memo hit (`Ok(None)` when it is not a directory).
- Memo is Portage's `_aux_cache["packages"]` (`vartree.py:909-973`); `cache_these = _aux_cache_keys union wants` is why a validated snapshot fills all 23 fields at once.
- Residual in-place-rewrite hole is Portage's own documented one (why it calls `_bump_mtime` on both sides of `aux_update`).
- Portage's `aux_get` `EAPI == "" -> "0"` translation is not here: callers default individually and that parity question is a filed residue. The invalid-`SLOT` half is here (#115 S1).
- Existence test and validity check share one `statx` (#112).

## fn claim_paths / owner_wanted
- Shared with the SQLite backend. `wanted` is built once per `owners` call: a lookup per line instead of a scan of every queried path (#314).
- The `files` scan uses this since #314; S8.2 for every backend.

## fn entries / category_entries
- Portage `vardbapi._excluded_dirs`.

## fn category_entries (d_type filter)
- The `path().is_dir()` fallback keeps exact semantics for symlinked entries and `DT_UNKNOWN` filesystems.

## fn read_file
- One `open`, no separate existence `stat`; a missing entry directory and a missing file are both `None`.

## fn read_file_all
- Moved from `needed_elf::read_all_needed_entries`: the same walk and the same order of `open`s. Today's `read_to_string(..).unwrap_or_default()` is why an unreadable file is `None`.

## fn entry_image
- Sorted by name like `list_files`; a sub-directory is not part of an entry and is skipped. Directory `stat`ed first; reads do not change its mtime. Stamp judged by the reader rule (`parse_metadata_text` + dir-mtime equality).

## fn entry_stat
- Only the small `metadata` file is read, to judge its stamp.

## fn owners
- Moved from `ebuild_merge::find_owners` (its directory walk plus the per-entry `installed_contents_files`). Keeps the `statx` that `resolve_vdb_entry` tests the entry with before the read.

## fn open_vdb_dir
- N12: the remote `server:<path>` shadow and its `pull_dir` copy. D4-store behaviour changed across S1.2/S1.7.

## fn from_cli_path
- A path ending in the VDB location means the root three levels up, so the D4 stores come along.

## fn new
- `root` is used as given: no canonicalisation, no I/O.

## tests: generation_*
- The `all_installed_packages` cache key. Moved with `vdb_fingerprint` from portage-repo.
