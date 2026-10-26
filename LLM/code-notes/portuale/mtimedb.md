# crates/portuale/src/mtimedb.rs
`/var/cache/edb/mtimedb` persistence for the `resume` mergelist, its `resume_backup` rotation, the `info` dir-mtime memo, and the `updates` per-file mtime record.

## module
- Ports Portage's `resume` support: `_emerge/Scheduler.py::_save_resume_list` + `_emerge/actions.py`'s `--resume` handling (backlog #168). The resolved plan's mergelist is saved to `mtimedb["resume"]` **before the first merge runs** (Portage `Scheduler.merge()`'s own `_save_resume_list` call), so a SIGKILL mid-merge still leaves a list for `emerge --resume`. Each successful merge then removes its own entry (`remove_merged_entry`, Scheduler.py:1595-1602), deleting the key once the list empties; a clean failure re-saves the still-unmerged tail. Each entry is `[type, root, cpv, operation]` along with the original atom args (`mtimedb["resume"]["favorites"]`). `emerge --resume` reads them back and merges them in order; `emerge --resume --skipfirst` drops the first (the one that failed) before continuing.
- Portuale writes Portage's full shape (hand-rolled, tab-indented like Portage `json.dumps(indent="\t", sort_keys=True)`, no trailing newline): every load normalizes the top level the way `MtimeDB._load` does (`old` replaces `updates`, `cur` is dropped, `starttime`/`version`/`info`/`ldpath`/`updates` defaults are filled, every other key is dropped with `Deleting invalid mtimedb key: <k>` on stderr), and every write commits the whole normalized object at once, skipping the write when the render is unchanged (`MtimeDB.commit`, `util/mtimedb.py:49-117`). Sections portuale never parses (`ldpath`, `resume`, `resume_backup`) are kept as raw text and splice back verbatim -- reformatting a `resume` section would restamp a foreign mergelist's stored root with the current one. Tolerant of a Portage-written file with the same shape.
- The entry point (`pretend/mod.rs::run`, ahead of the global-updates call) loads the target root, then the running root when it differs, exactly where Portage's `load_emerge_config` builds its `MtimeDB`s (`lib/_emerge/actions.py:3378-3391`, called from `_emerge/main.py:1360` for every action but the pre-config exits) -- so `--info`, `--version`, `--pretend` and merges all warn, while `--help` stays silent. Only the first load per root per run reports warnings (a `WARNED` set); every `read_*`/`write_*` loads the file fresh and silently, so each op is a read-modify-write off current bytes -- no stale-cache class for the in-process-setup/spawned-run/in-process-read test pattern, and parallel same-root runs converge like Portage's own per-op dict.
- `resume_backup` rotation (`rotate_resume_to_backup`, actions.py:664-672): right before a fresh, non-`--resume` merge starts, an existing `resume` entry whose mergelist has more than one item is preserved as `resume_backup` (replacing whatever backup existed) rather than just being overwritten -- `emerge --resume` later promotes `resume_backup` back to `resume` when `resume` itself is absent (actions.py:222-224, `read_resume_list`'s own fallback), so an accidental fresh `emerge <atom>` doesn't destroy a still-recoverable resume point. `clear_resume_list` only ever removes `resume` itself (Scheduler.py:1599-1601's own `del mtimedb["resume"]` once the mergelist empties), leaving `resume_backup` untouched.
- `mtimedb["info"]`: Portage's `MtimeDB` (util/mtimedb.py) carries an `"info"` key mapping each absolute GNU-info directory to that directory's `st_mtime` (whole seconds) the last time Portage's `chk_updated_info_files` (util/_info_files.py) regenerated its `dir` index -- or skipped regenerating because the mtime already matched. `read_info_mtimes`/`write_info_mtimes` round-trip that map through the same file without disturbing `resume`/`resume_backup` (Portage `commit()` persists the whole dict at once).
- Two deliberate narrowings:
  - `version` is `portuale-<crate version>` (today `portuale-300.83.0`), not Portage's `3.0.83`: Portage stamps `str(portage.VERSION)` on every write but never reads the key back, so the string never causes a Portage write by itself (see `LLM/portage-quirks.md`). `ldpath` is `--sync`/`env-update` state portuale never manages, `starttime` Portage bookkeeping with no reader here; both round-trip (defaults `{}`/`0`, foreign values verbatim).
  - Values are whole-second `i64`s (Portage `os.stat(...)[stat.ST_MTIME]` is already an int). A foreign-written float (`123.0`) is accepted on read and truncated; what portuale writes back is always an integer, matching Portage's steady state.
- Binary-entry replay: each mergelist item carries Portage's own `type` tag (`"ebuild"` or `"binary"`), so `emerge --resume` can dispatch a resumed binary package through `emerge_getbinpkg::run_merge_plan` the same way `--getbinpkg` itself does, rather than only ever replaying source entries. This also closes the "build-time flags (`--usepkg` etc.)" gap `myopts` used to have no room for: Portage re-derives usepkg/getbinpkg preference from `myopts` and re-decides binary-vs-source at resume time, but portuale's own mergelist already records that *decision* directly (each entry's own `ResumeEntryKind`, fixed at the point the original run resolved it) -- arguably more robust than re-deciding from restored flags, so `myopts` itself still only carries `--oneshot`/`--onlydeps` (the two flags that govern *world*-recording, unrelated to which entries are binary). Cut: a resumed binary entry always resolves from the local `$PKGDIR` (`ResumeEntryKind::Binary` maps to `GraphEntry::remote_binary: false`) -- Portage re-fetches from a remote binhost too if the original run would have; portuale doesn't try to re-derive "was this fetched remotely" from the failed run's own state.

## enum ResumeEntryKind
- Portage's own `mergelist` entry `type` tag (`["ebuild"|"binary", <root>, <cpv>, <action>]`) -- which kind of merge `emerge --resume` replays this entry as.

## fn format_section_body
- Portage `json.dumps(..., sort_keys=True)` shape (tab-indented, `json.dumps(..., indent="\t", sort_keys=True)`) for one section's own body (no outer `"resume": { ... }` key wrapper -- the caller adds that) -- shared by every section a write ever produces; `myopts` keys alphabetical.

## fn read_info_mtimes
- `"path": 123` pairs (Portage `json.dumps` of `{str: int}`); a foreign-written float (`123.0`) is accepted and truncated -- what portuale writes back is always an integer.

## fn load_mtimedb
- Returns the unknown-key warnings for the entry point to print (one `MtimeDbLoad` per root per invocation, target then running). The warnings are sorted: Portage iterates `set(d.keys()) - _MTIMEDBKEYS`, whose order is hash-random per process, so byte-identical repeated runs need the deterministic choice. The normalized top level itself is not cached -- every op re-loads (see the module note).
- `old` *replaces* `updates` when both are present (Portage's `d["updates"] = d["old"]`, not a merge); `cur` is dropped silently. Non-object `info`/`updates` values read back as the default. `starttime` parses as `i64` (default `0`); `version` unescapes a JSON string (default `""`).

## fn commit
- Compares the post-mutation render against the pre-mutation render and skips the write when they match (Portage's `d != self._clean_data`, `util/mtimedb.py:94-101`); a write stamps `portuale-<CARGO_PKG_VERSION>` first. The file is never unlinked: Portage's `commit` only ever writes, so clearing the last section leaves the default keys behind.
- Empty flat sections render inline (`"info": {}`, Portage's own `json.dumps` of an empty dict); the whole document carries no trailing newline (`json.dumps` output written verbatim -- S0 (a) `t2/a-mtimedb` ends `...3.0.83"}`).

## fn read_updates_mtimes
- `mtimedb["updates"]` (backlog #562): updates-file path -> that file's `st_mtime` whole seconds, Portage `_global_updates`' `prev_mtimes` -- the record Portage's `emerge --package-moves` global updates reads and commits. Shares the flat-map reader with `info` (`read_flat_mtimes`/`parse_flat_mtime_map`).

## fn write_updates_mtimes
- Commits the whole normalized object with `updates` replaced (an unchanged map skips the write). Production recording since backlog #576 Task 11, so no longer test-only.

## fn read_raw_objects
- Backlog #575 Task 1: used to extract only the four known sections (`resume`, `resume_backup`, `info`, `updates`) and drop everything else on every write. Now a string-aware top-level walk returning `(key, raw value slice)` for every `"key": <value>` pair at depth 1, whatever the value shape (object, array, string, bare number/literal); the load step (`load_normalized`) parses the managed keys out of it, keeps the valid-but-unmanaged ones as verbatim raw text, and drops the rest with a warning. A `"` that does not open a pair (e.g. inside an array, or a malformed spot) is skipped as a plain string so the walk always terminates with the well-formed pairs intact. Slicing stays on ASCII structural bytes, so multibyte string contents (Portage writes `ensure_ascii=False`) are never split.
- Helpers: `string_end` (escape-aware closing quote; one-past), `skip_ws`, `raw_value_end` (brace-matched objects/arrays, quoted strings, bare tokens to the next delimiter). `extract_object` reuses `raw_value_end` for its brace matching and stays for the `resume`-section parse path (`read_section`), which needs one key's object rather than the whole key set.

## fn extract_object
- Only whitespace and a single `:` may separate the key from its own value -- JSON's own `"key": {...}` shape. Reuses `raw_value_end` for the brace matching (the value must still start with `{`); the one-key lookup stays for the `resume`-section parse path.
- Unlike a naive "split on the key, take up to the first `}`", stays correct once a second top-level section follows in the same file; and not "find the first `{` anywhere later", which would skip past an array-shaped value (e.g. `"favorites"`) into a sibling key's object.

## fn parse_section
- The version grammar is too broad for a clean regex group, so the cpv is split into `(cat, pkg, ver)` in Rust.

## fn write_resume_list
- Like Portage's own `_save_resume_list` (Scheduler.py:2398-2431) the assignment is unconditional: an all-noop plan (entries all `nomerge` / already-installed) still overwrites any stale list -- Portage reaches `Scheduler.merge()` for such a plan (no `--ask` early return, no `--pretend` early return) and commits the empty list. Callers on the *failure* path guard the empty case themselves (a failure with nothing unmerged leaves the per-merge shrink's own tail alone). Portage's own resume assignment only ever touches the `"resume"` key.
- Up-front save of an all-noop plan overwrites a stale list with empty (backlog #179): readers treat an empty list as nothing to resume (`parse_section` maps it to `None`), so behaviour matches Portage's "nothing left to merge" too.

## fn remove_merged_entry
- Committed at once, "so that --resume still works after being interrupted by reboot, sigkill or similar" (Scheduler.py:1595-1602). Removes the first entry equal to `entry` (Portage's own `list.remove` semantics); an already-installed no-op "merge" reports success without ever being saved. A non-empty remainder is re-committed with an existing `resume_backup` preserved untouched.

## fn rotate_resume_to_backup
- Replaces whatever backup existed before -- Portage's own unconditional `mtimedb["resume_backup"] = mtimedb["resume"]` -- and `resume` itself is cleared. A single-item mergelist, or no `resume` entry at all, is a no-op. Matches Portage's own `len(...) > 1` guard exactly (actions.py:664-672): a one-package resume list isn't worth preserving as a "you can still get this back" backup.

## fn read_resume_list
- Ports actions.py:220-225: promotion removes the backup from its own slot.

## fn split_cpv
- `cat/pkg-1.2.3-r1` -> `("cat/pkg", "1.2.3-r1")`. Splits at the last `-` whose following char is a digit (Portage `pkgsplit` heuristic).

## fn clear_resume_list
- Ports Scheduler.py:1599-1601's own `del mtimedb["resume"]` once the mergelist empties; a stale-by-then recovery point Portage doesn't clear here either.

## tests: info_mtimes_round_trip_without_disturbing_the_resume_list
- Ports `MtimeDB.commit()` semantics: one file, every key at once.

## tests: updates_mtimes_round_trip_without_disturbing_other_sections
- Backlog #562: the `updates` record shares the file with `resume` and `info`; every writer preserves the others, sections serialize in `sort_keys` order, and clearing the record leaves the other sections alone.

## tests: managed_top_level_keys_splice_back_verbatim_while_unknown_ones_drop
- Seeds a Portage-shaped file (odd-spaced `ldpath` object, `starttime` number, `version` string per `util/mtimedb.py::_MTIMEDBKEYS`, plus a stray `bogus`) and asserts an `info` write and a `resume` write each keep the managed keys byte-identical, drop the stray key, and stamp the `portuale-<version>` version.

## tests: info_mtimes_accepts_a_foreign_float_and_reads_empty_as_empty
- Absent file -> empty memo (Portage's own `setdefault("info", {})` shape).

## tests: a_mixed_ebuild_and_binary_mergelist_round_trips_its_own_kind_tags
- Portage's own resume mergelist holds `[type, root, cpv, action]` with `type` `"ebuild"` or `"binary"`; portuale's own used to only ever write `"ebuild"`.

## tests: up_front_save_of_an_all_noop_plan_overwrites_a_stale_list_with_empty
- #179: Portage `Scheduler._save_resume_list` (Scheduler.py:2398-2431) assigns `mtimedb["resume"]` unconditionally at merge start, so a run whose plan has nothing to merge still commits the fresh (empty) list -- no stale entry survives. The driver calls `write_resume_list` with exactly the up-front-filtered mergelist (empty for an all-noop plan), so the write itself must not no-op on empty.

## tests: rotate_to_backup_ignores_a_single_item_resume_list
- Portage's own `len(...) > 1` guard (actions.py:664-672).

## tests: driver_sequence_rotates_first_then_saves_up_front_and_shrinks_per_merge
- The whole backlog #168 driver lifecycle in one hermetic sequence.

## ResumeOpts.merge_wait_scope (#512)
- `--merge-wait-scope` persists in `mtimedb["resume"]["myopts"]` as `"--merge-wait-scope": "<deep|system|toolchain|none>"` (first key in `json.dumps(sort_keys=True)` order), so `emerge --resume` replays the scope the original run used; absent means Portage's `deep` default. The substring reader tolerates Portage-written files; old files without the key resume as `deep`.

## fn write_content (atomic)
- Portage `atomic_ofstream` (same-directory temporary renamed over the target, `util/__init__.py`) plus `apply_secpass_permissions(mode=0o644, gid=portage_gid)`: the file mode is forced to `0644` regardless of the writer's umask and the `portage` group is applied best-effort (a writer outside the group keeps its own gid, never failing), so a crash never leaves a half-written record behind and a stale temporary is removed on failure. Writes the render verbatim (no trailing newline added).

## tests: writes_replace_the_file_atomically_with_fixed_mode
- Pre-creates the file with mode `0600` and asserts the rewrite lands `0644` (proving the explicit chmod, umask-independent) with the new content and no `.*.tmp.*` leftover in the directory.

## tests: mtimedb_file_carries_the_portage_gid_when_permitted
- When the `portage` group resolves and the writer may chown (root, or a member), the rewritten file is group-owned by it; otherwise only the mode and the successful write are pinned.

## retired: write_sections/preserved_parts/render_raw_part
- The full-shape rewrite retired the splice-verbatim writers: `write_sections` took pre-rendered `\t"key": {...}` parts in a `BTreeMap` (hence `sort_keys` order) and removed the file when nothing was stored; `preserved_parts(root, fresh_key)` carried every other top-level value over verbatim except the caller's own `fresh_key`; `render_raw_part` wrapped one raw value back under its key (as `\t"resume": {...}`, also used for the `resume` -> `resume_backup` rotation). Every write now commits the whole normalized object from `render_full` instead; see `## fn commit`.

## tests: fresh_file_has_portage_default_keys
- A first commit on a fresh root renders every default key in Portage's serialization (sorted keys, tab indent, no trailing newline), with `version` as `portuale-<CARGO_PKG_VERSION>`; byte shape from S0 (a) `t2/a-mtimedb`, Portage `util/mtimedb.py:83-86, 105`.

## tests: stray_key_is_dropped_with_portage_warning
- A stray key loads with Portage's warning (`util/mtimedb.py:88-90`) and is gone after the next commit; matches S0 (b) (`t2/b-p.err`, `t2/b-mtimedb`).

## tests: old_key_becomes_updates_and_cur_is_dropped
- `old` reads back as `updates` (replacing it) and both `old`/`cur` vanish on commit (`util/mtimedb.py:77-81`); the S0 (b) root carries both shapes.

## tests: unchanged_state_is_not_rewritten
- Rewriting identical content leaves bytes and mtime alone (`commit` compare, `util/mtimedb.py:94-101`); matches S0 (c) (repeat run, mtime and md5 unchanged).

## tests: warning_printed_once_per_root_per_run
- First load per root reports the warning; later loads, and the `read_*`/`write_*` calls between them, stay silent -- including a read before any load (`lib/_emerge/actions.py:3380-3391`, one `MtimeDB` per root).
