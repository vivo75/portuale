# crates/portage-vdb/src/files.rs
`FilesDb`: the historic `var/db/pkg` directory tree.

## module
- The read side of per-entry and `aux` reads (S1.2) is the former `portage-repo` code, moved unchanged: the same `openat`/`statx` sequence and the same in-process memo (thread-local, validated by the entry directory's `st_mtime_ns`).
- The merge's write side is in `files_write.rs` (S1.4: the pending entry, the counter, the preserved-libs registry and the config memory).
- `reverse_dependents` stays above this crate on `files` (crate module doc item 19; S1.5 moved `owners`, `read_file_all`, `delete_entry`, `replace_file`; S1.6 moved `world` / `world_sets`).

## fn dir_mtime_nanos
- Moved from `portage-repo::dir_mtime_nanos`. An external in-place rewrite of a file inside a running process is not caught -- the same in-process-cache property Portage's `vardb` dbapi has; no portuale writer does it.
- `st_mtime` of `p` in nanos, 0 when the path is missing or unreadable. The invalidation signal for category-scoped caches (moved from `portage-repo::dir_mtime_nanos`): every portuale vdb mutation replaces or removes a package dir (`publish_vdb_tmp` `remove_dir_all` + `rename`; unmerge removes the dir), so the *category* dir mtime catches all of them. An external in-place rewrite of a file inside a running process is not caught -- the same in-process-cache property Portage's `vardb` dbapi has; no portuale writer does it.

## fn read_metadata_file
- Portage `vartree.py:115-187`. Other `#` lines ignored; a line without `=` skipped; `k, v = line.split("=", 1)` with the last duplicate winning.
- Portage `_read_metadata_file(path, dir_st)` (`vartree.py:115-187`): parse and validate a consolidated `metadata` snapshot. `None` unless `#format=` parses to [`METADATA_FILE_FORMAT_VERSION`] **and** `#dir_mtime=` parses and equals the package directory's `st_mtime_ns`. A `#format=` this version does not know is rejected immediately (Portage abandons the file on the version line, before parsing the rest); other `#` lines are ignored; a line without `=` is skipped; `k, v = line.split("=", 1)` with the last duplicate winning. Portage writes `#dir_mtime=` last, so a file left truncated by an interrupted write lacks it and is rejected rather than read as a short snapshot.

## fn read_vdb_file
- Portage applies the same `" ".join(myd.split())` normalisation on this path (`vartree.py:1044-1046`), so the bytes are the same whether the snapshot validated or not.
- Invalid UTF-8 decodes lossy (`U+FFFD`), matching `encoding="utf-8", errors="replace"` (`vartree.py:1035-1039`), instead of collapsing to `""` (backlog #125, audit O15).

## fn is_valid_aux_slot
- `slot = [\w][\w+.-]*` ASCII (`versions.py:38`, `re.ASCII`); the `/sub` half iff the EAPI has `slot_operator` (`versions.py:76-90`). No EAPI parametrization inside the EAPI 5+ floor (`agent-context.md`: every live EAPI has `slot_operator`, `eapi.py:319`).
- An entry whose own `EAPI` is empty (for which Portage would use the single-slot shape after its `EAPI ""->"0"` step) is out of scope -- that translation is #115's documented cut, and no such entry has a portuale-visible consumer anyway (Phase 12 S0.3). An empty `SLOT` keeps #126's O5 caller contracts.
- Whether `value` is a `SLOT` Portage `aux_get` keeps instead of translating to `"0"`: `slot(/slot)?` with `slot = [\w][\w+.-]*` (ASCII — `versions.py:38`, `re.ASCII`), the `/sub` half iff the EAPI has `slot_operator` (`versions.py:76-90`). Portuale does no EAPI parametrization inside the EAPI 5+ floor (`agent-context.md`: every live EAPI has `slot_operator`, `eapi.py:319`), so this is always the operator shape; an entry whose own `EAPI` is empty (for which Portage would use the single-slot shape after its `EAPI ""→"0"` step) is out of scope — that translation is #115's documented cut, and no such entry has a portuale-visible consumer anyway (Phase 12 S0.3). Empty is *not* invalid here: a missing/empty `SLOT` stays `""` at this layer and keeps #126's O5 caller contracts.

## fn translate_aux_slot
- Portage translates in the outer `aux_get` (`vartree.py:967-972`); here `aux_get_field` serves both roles, so it lives here, for the `SLOT` key only.
- Portage `aux_get`'s invalid-`SLOT` → `"0"` translation, applied at the one seam every vdb `SLOT` read flows through. Portage translates in the outer `aux_get` (`vartree.py:967-972`); here [`FilesDb::aux_get_field`] serves both roles, so it lives here, for the `SLOT` key only, over present (non-empty) values. The memoised value is the translated one (observably identical: the translation is idempotent, `"0"` is valid).

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
- Every regular file of the live entry (sorted by name, like `list_files`; a sub-directory is not part of an entry and is skipped), exact bytes, `st_mode` and `st_mtime_ns`; the directory's `st_mode` and `st_mtime_ns`; and the `metadata` stamp state by the Portage reader rule ([`parse_metadata_text`] + equality with the directory's `st_mtime_ns`). The directory is `stat`ed first; reads do not change its mtime.

## fn entry_stat
- Only the small `metadata` file is read, to judge its stamp.

## fn owners
- Moved from `ebuild_merge::find_owners` (its directory walk plus the per-entry `installed_contents_files`). Keeps the `statx` that `resolve_vdb_entry` tests the entry with before the read.

## fn open_vdb_dir
- N12: the remote `server:<path>` shadow and its `pull_dir` copy. D4-store behaviour changed across S1.2/S1.7.

## fn from_cli_path
- A path ending in the VDB location means the root three levels up, so the D4 stores come along.
- The `files` backend a command-line `files:PATH` names: `PATH` is a root, or the VDB directory itself (a path ending in the VDB location means the root three levels up, so the D4 stores come along). Any other existing directory is a bare VDB directory ([`FilesDb::open_vdb_dir`]: `root()` is `None`, no D4 stores). `None` when `PATH` is no directory.

## fn new
- `root` is used as given: no canonicalisation, no I/O.

## tests: generation_*
- The `all_installed_packages` cache key. Moved with `vdb_fingerprint` from portage-repo.

## fn fingerprint
- A cheap fingerprint of the VDB directory's structure (moved from `portage-repo::vdb_fingerprint`): the vdb dir's own mtime plus every category dir's mtime, xor-folded with a count. Any package merge/unmerge changes the mtime of `var/db/pkg` (new category) or of a category dir (new/removed package dir), so a matching fingerprint means the installed set is unchanged. ~30 `stat`s vs the ~2000 `SLOT` file reads a full scan does.

## fn aux_get_field
- Portage `_aux_get(cpv, wants, st)` (`vartree.py:975-1053`) for one in-set key (moved from `portage-repo::vdb_aux_get`): `stat` the package dir once, try the consolidated `metadata` snapshot, and serve an in-set key from it with **no** `open()`; otherwise read the individual file (the pre-#109 path). `None` when the entry directory does not exist (one `statx`, no more).
- The snapshot rule is Portage's own (`vartree.py:1010-1017`): a validated snapshot is *complete*, so a field missing from it had no individual file either and is served as `""` instead of paying an `open()` that would just fail. Safe by construction for added fields -- adding a per-field file bumps the package dir's mtime, which invalidates the snapshot -- and the residual in-place-rewrite hole is Portage's own documented one (why it calls `_bump_mtime` on both sides of `aux_update`).
- Portage `aux_get`'s `_aux_cache["packages"]` (`vartree.py:909-973`): per-instance metadata keyed on the package dir's `st_mtime_ns`, so a second key on the same instance costs one `stat` instead of a snapshot read (and a per-key `open()` on the fallback path). `cache_these = _aux_cache_keys ∪ wants` is why a validated snapshot fills **all** 23 fields at once; the fallback path resolves lazily per key and records each resolved value, including `""`.
- The in-process staleness property is Portage's own: an in-place rewrite of a field file leaves the dir mtime alone, so a value read once is served until the dir changes. Portage calls `_bump_mtime` on both sides of `aux_update` for exactly that reason; the same property is why portuale's merge path (which replaces the whole entry dir) is safe. Thread-local so the lookup stays lock-free, like `EUF_CACHE`.

## fn claim_paths
- The claims of one entry's `CONTENTS` text (the per-line rule of [`FilesDb::owners`], shared with the SQLite backend): a line whose first two words are a kind (`obj`, `sym`, `dir`, `dev`, `fif`, `bin`) and a path claims that path; one leading `/` is ignored on both sides. Each claim pairs the first of `paths` it matches with `key`. `wanted` is [`owner_wanted`] of `paths`, built once per `owners` call: a lookup per line instead of a scan of every queried path (#314).

## fn owner_wanted
- For `owners` on every backend (S8.2; the `files` scan since #314): each distinct normal form (one leading `/` stripped, as [`claim_paths`] compares) of `paths`, with the first input path that has it, which is the one `claim_paths` reports.
