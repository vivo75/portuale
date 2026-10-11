# crates/portage-vdb/src/files_write.rs
`FilesDb`'s write side: the merge's pending entry, counter, registry and config memory.

## module
- S1.4. Every function is the code `portuale::ebuild_merge` ran before S1.4: the same paths (joined, never canonicalised), the same `std::fs` calls in the same order, the same error texts (`"<path>: <io error>"`).
- Crate module doc item 20: nothing atomic, no lock, `commit` is only an ordering point.
- Formats: the registry is Portage `PreservedLibsRegistry.store()`'s `json.dumps(..., ensure_ascii=False, indent="\t", sort_keys=True)`; the config memory is `grabdict`/`writedict`'s `"path value\n"` lines. S2.6: the database backends store the parsed values, so a converter must read and write the `files` bytes.

## const COUNTER_PATH / PRESERVED_LIBS_PATH / CONFIG_MEMORY_PATH / WORLD_PATH / WORLD_SETS_PATH
- Portage `const.py`: `CACHE_PATH` + `vartree.py:420`; `PRIVATE_PATH` + the registry's hardcoded file name; `PRIVATE_PATH` + `CONFIG_MEMORY_FILE` (`vardbapi._conf_mem_file`); `WORLD_FILE`; `WORLD_SETS_FILE`.

## fn read_world / read_world_sets
- Moved from `pretend::read_world_atoms` / `read_world_sets`.

## fn read_preserved_libs
- Moved from `ebuild_merge::read_plib_registry`. Portage `load()`'s graceful degrade to `{}`. `pruneNonExisting` (lstat-based) stays with the caller (N6).

## fn read_config_memory
- Moved from `ebuild_merge::read_cfgfiledict`.

## struct FilesTxn
- The `emerge --deselect` rewrite of both world files made `create_dir_all` once.

## fn set_mtime_ns
- Uses `futimens` on a read-only descriptor (works on directories; the owner needs no write permission).

## fn write_image
- Steps 1-5 of `insert_entry` inside the pending dir.

## struct VdbLock
- Portage `lockdir(<vdb>)` (`locks.py:167`, `lockfile(..., wantnewlockfile=1)`, `wantnewlockfile` implies `unlinkfile` (`locks.py:247-251)`).
- For `/var/db/pkg` the lockfile is `/var/db/.pkg.portage_lockfile`.
- Portage's `vardbapi.lock()` is reentrant and held across a whole merge, which `files` does not model (no other portuale holder of this lock exists, so this cannot self-deadlock).
- `Drop` is Portage `unlockfile` with `unlinkfile`.

## fn write_atomic
- Portage `util/__init__.py`, `atomic_ofstream`. A new file keeps the umask-derived default mode.

## fn begin_entry
- Moved from `ebuild_merge::create_vdb_tmp` (Portage `treewalk()`'s `self.dbdir = self.dbtmpdir; self.delete(); ensure_dirs(...)`). The category dir of a removed stale entry is not `rmdir`ed (Portage does; S0.1 row 3).

## fn put_entry_file / copy_entry_file
- As `populate_vdb_tmp` / `write_vdb_tmp_contents` did.

## fn next_counter
- Portage `counter_tick_core` (`vartree.py:1372-1397`) over `get_counter_tick_core` (`1304-1370`): `vardbapi.lock()` -> `lockdir(<vdb>)`. A counter file that fell behind (corrupt, restored, lost) cannot hand out a value below an installed package's, which would trip AUTOCLEAN.
- Skips a missing entry, field, or non-integer value, like `int(aux_get(cpv, ["COUNTER"])[0])` with `except (KeyError, OverflowError, ValueError): continue`.

## fn seal_entry
- Moved from `ebuild_merge::write_consolidated_metadata_file` (Portage `_write_metadata_file` + `_stamp_metadata_file`, `vartree.py:188-229`, `not delete_individual`).
- Non-UTF-8 fields open with `errors="replace"` (`vartree.py:268`), sealed as lossy text like an unsealed read (#309).
- Moved from `ebuild_merge::write_consolidated_metadata_file` (Portage `_write_metadata_file` + `_stamp_metadata_file`, `vartree.py: 188-229`, `not delete_individual`): every [`METADATA_FILE_FIELDS`] file in the pending dir that reads as UTF-8, whitespace-normalised, sorted, under `#format=1`, written with `std::fs::write`; then a `stat` of the dir and `#dir_mtime=<st_mtime_ns>` appended in a second open. No-op when no field file exists.
- Portage `_consolidate_to_metadata_file` opens with `errors="replace"` (`vartree.py:268`), so a non-UTF-8 field is sealed as its lossy text, like an unsealed read (#309).
- Portage appends `#dir_mtime=` in a separate `open(..., "a")` step, *after* the body is on disk, so `st_mtime_ns` reflects the body write (the last dir change) and the append itself -- no new dir entry -- leaves it untouched.

## fn finish_entry
- Moved from `ebuild_merge::publish_vdb_tmp` (Portage `self.dbdir = self.dbpkgdir; self.delete(); _movefile(dbtmpdir, dbpkgdir)`).

## fn insert_entry
- S2.6; plan 0.7. Stale detail: if the stored stamp equals the directory's mtime, rewrite it in place as that value minus one. `metadata` gets mode 0600 for the append; directory `chmod` touches ctime only.

## fn delete_entry
- Moved from `ebuild_unmerge::delete_vdb_dir` (Portage `dblink.delete()`). Best-effort `remove_dir` ignored when another entry still lives there (N14).

## fn replace_file
- Moved from `remove_from_contents`' two `std::fs::write` calls.

## fn set_world / set_world_sets
- Moved from `pretend` (`update_world_file`, `deselect_from_world`, `run_deselect`). An empty list writes an empty file.

## fn set_preserved_libs
- Moved from `ebuild_merge::write_plib_registry`. Portage `store()`'s `_data == _data_orig` (backlog #167). Not Portage's `atomic_ofstream`.

## fn set_config_memory
- Moved from `ebuild_merge::write_cfgfiledict`.

## fn parse_json_string
- Moved from `ebuild_merge::parse_json_string`. Handles the `\"`/`\\`/`\/`/`\n`/`\t`/`\r`/`\b`/`\f`/`\uXXXX` escapes Portage's `json.dumps` may emit for a path.

## fn parse_preserved_libs
- Moved from `ebuild_merge::parse_plib_registry`. Portage `load()`'s graceful degrade to `{}`.

## fn format_preserved_libs
- Moved from `ebuild_merge::write_plib_registry`. Byte-for-byte Python's output: `"` and `\` escaped, C0 controls as short forms or `\u00xx`, everything else (including non-ASCII and DEL) raw UTF-8.
- Portage `store()`'s `json.dumps(..., ensure_ascii=False, indent="\t", sort_keys=True)` layout (the `BTreeMap` keeps keys sorted). An empty dict is exactly `{}`, and there is no trailing newline, byte for byte Python's output. Moved from `ebuild_merge::write_plib_registry`.

## tests: merge_sequence_publishes_a_stamped_entry
- Pending entry hidden from readers; the counter ticks from a missing store to 0; a stale pending entry is wiped by the next `begin_entry`.

## tests: next_counter_is_above_every_installed_entry_counter
- Value derived from Portage's source (`max_counter + 1`), checked by hand against it.

## tests: next_counter_replaces_the_file_atomically_keeping_its_mode
- Keeps the replaced file's mode, leaves no temporary behind, writes the bare integer with no newline.

## tests: concurrent_ticks_are_unique
- Threads here stand in for Portage's processes.

## fn read_pending
- [`InstalledDb::read_pending_file`](crate::InstalledDb::read_pending_file): one `open` of `-MERGING-<pf>/<name>`, no existence `stat`. A missing pending entry and a missing file are both `Ok(None)`.

## fn read_world
- Moved from `pretend::read_world_atoms`: one `read_to_string` of `<root>/var/lib/portage/world`; a missing file (`NotFound` only) is an empty world, any other failure is [`Error::Io`]. Lines are trimmed; blank, `#` and `@` lines are dropped.

## fn read_world_sets
- Moved from `pretend::read_world_sets`: as [`FilesDb::read_world`], keeping only `@` lines and stripping every leading `@`.

## fn json_quote
- Portage `json.dumps(..., ensure_ascii=False)`: `"` and `\` escaped, C0 controls as the short forms (`\b \t \n \f \r`) or `\u00xx`, everything else (including non-ASCII and DEL) raw UTF-8.

## fn next_counter_is_above_every_installed_entry_counter
- Portage `get_counter_tick_core` (`vartree.py:1304-1370`): the next counter is above every installed entry's `COUNTER` even when the `counter` file is behind (or missing), and entries whose `COUNTER` is absent or not an integer are skipped (`except (KeyError, OverflowError, ValueError): continue`). Value derived from the Portage source (`max_counter + 1`), checked by hand against it.

## fn next_counter_waits_for_the_vdb_lock
- The tick takes Portage's VDB lock (`<vdb parent>/.<basename>.portage_lockfile`): while another holder has it, `next_counter` blocks.
