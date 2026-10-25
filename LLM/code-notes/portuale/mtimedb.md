# crates/portuale/src/mtimedb.rs
`/var/cache/edb/mtimedb` persistence for the `resume` mergelist, its `resume_backup` rotation, the `info` dir-mtime memo, and the `updates` per-file mtime record.

## module
- Ports Portage's `resume` support: `_emerge/Scheduler.py::_save_resume_list` + `_emerge/actions.py`'s `--resume` handling (backlog #168). The resolved plan's mergelist is saved to `mtimedb["resume"]` **before the first merge runs** (Portage `Scheduler.merge()`'s own `_save_resume_list` call), so a SIGKILL mid-merge still leaves a list for `emerge --resume`. Each successful merge then removes its own entry (`remove_merged_entry`, Scheduler.py:1595-1602), deleting the key once the list empties; a clean failure re-saves the still-unmerged tail. Each entry is `[type, root, cpv, operation]` along with the original atom args (`mtimedb["resume"]["favorites"]`). `emerge --resume` reads them back and merges them in order; `emerge --resume --skipfirst` drops the first (the one that failed) before continuing.
- Portuale writes a Portage-compatible `{"resume": {...}, "resume_backup": {...}, ...}` (hand-rolled, tab-indented like Portage `json.dumps(indent="\t", sort_keys=True)`); every writer carries the other top-level keys over verbatim (raw-value splice, never parse-and-reformat -- reformatting a `resume` section would restamp a foreign mergelist's stored root with the current one). Unknown top-level keys an existing file had (`ldpath`, `starttime`, `version`: object, number and string values alike) splice back untouched, so no write drops what portuale does not own (backlog #575 Task 1: `read_raw_objects` used to keep only the four known sections, deleting the rest on every write). Tolerant of a Portage-written file with the same shape.
- `resume_backup` rotation (`rotate_resume_to_backup`, actions.py:664-672): right before a fresh, non-`--resume` merge starts, an existing `resume` entry whose mergelist has more than one item is preserved as `resume_backup` (replacing whatever backup existed) rather than just being overwritten -- `emerge --resume` later promotes `resume_backup` back to `resume` when `resume` itself is absent (actions.py:222-224, `read_resume_list`'s own fallback), so an accidental fresh `emerge <atom>` doesn't destroy a still-recoverable resume point. `clear_resume_list` only ever removes `resume` itself (Scheduler.py:1599-1601's own `del mtimedb["resume"]` once the mergelist empties), leaving `resume_backup` untouched.
- `mtimedb["info"]`: Portage's `MtimeDB` (util/mtimedb.py) carries an `"info"` key mapping each absolute GNU-info directory to that directory's `st_mtime` (whole seconds) the last time Portage's `chk_updated_info_files` (util/_info_files.py) regenerated its `dir` index -- or skipped regenerating because the mtime already matched. `read_info_mtimes`/`write_info_mtimes` round-trip that map through the same file without disturbing `resume`/`resume_backup` (Portage `commit()` persists the whole dict at once).
- Two deliberate narrowings:
  - Portage's file always carries every `_MTIMEDBKEYS` key (`info`, `ldpath`, `resume`, `resume_backup`, `starttime`, `updates`, `version`); portuale only ever writes the sections it manages (`resume`, `resume_backup`, `info`, `updates` since backlog #562) and splices every other top-level key back verbatim (backlog #575 Task 1 ended the old drop-unknown-keys behaviour -- see the module note). `ldpath` is `--sync`/`env-update` state portuale never manages, `starttime`/`version` Portage bookkeeping with no reader here. An empty `info`/`updates` map is omitted outright (Portage would write `"info": {}`); the file is removed when nothing at all is stored, as before.
  - Values are whole-second `i64`s (Portage `os.stat(...)[stat.ST_MTIME]` is already an int). A foreign-written float (`123.0`) is accepted on read and truncated; what portuale writes back is always an integer, matching Portage's steady state.
- Binary-entry replay: each mergelist item carries Portage's own `type` tag (`"ebuild"` or `"binary"`), so `emerge --resume` can dispatch a resumed binary package through `emerge_getbinpkg::run_merge_plan` the same way `--getbinpkg` itself does, rather than only ever replaying source entries. This also closes the "build-time flags (`--usepkg` etc.)" gap `myopts` used to have no room for: Portage re-derives usepkg/getbinpkg preference from `myopts` and re-decides binary-vs-source at resume time, but portuale's own mergelist already records that *decision* directly (each entry's own `ResumeEntryKind`, fixed at the point the original run resolved it) -- arguably more robust than re-deciding from restored flags, so `myopts` itself still only carries `--oneshot`/`--onlydeps` (the two flags that govern *world*-recording, unrelated to which entries are binary). Cut: a resumed binary entry always resolves from the local `$PKGDIR` (`ResumeEntryKind::Binary` maps to `GraphEntry::remote_binary: false`) -- Portage re-fetches from a remote binhost too if the original run would have; portuale doesn't try to re-derive "was this fetched remotely" from the failed run's own state.

## enum ResumeEntryKind
- Portage's own `mergelist` entry `type` tag (`["ebuild"|"binary", <root>, <cpv>, <action>]`) -- which kind of merge `emerge --resume` replays this entry as.

## fn format_section_body
- Portage `json.dumps(..., sort_keys=True)` shape (tab-indented, `json.dumps(..., indent="\t", sort_keys=True)`) for one section's own body (no outer `"resume": { ... }` key wrapper -- the caller adds that) -- shared by every section a write ever produces; `myopts` keys alphabetical.

## fn read_info_mtimes
- `"path": 123` pairs (Portage `json.dumps` of `{str: int}`); a foreign-written float (`123.0`) is accepted and truncated -- what portuale writes back is always an integer.

## fn write_sections
- An empty object has nothing Portage or portuale itself would ever read back, hence file removal.
- Takes pre-rendered parts in a `BTreeMap<String, String>` (keys serialize in `sort_keys` order, managed and foreign alike); preserved values splice back as raw text under `render_raw_part` (see the module note on the restamp hazard). The `resume` -> `resume_backup` rotation re-keys the same raw value rather than re-rendering it.

## fn read_updates_mtimes
- `mtimedb["updates"]` (backlog #562): updates-file path -> that file's `st_mtime` whole seconds, Portage `_global_updates`' `prev_mtimes` -- the record Portage's `emerge --package-moves` global updates reads and commits. Shares the flat-map reader with `info` (`read_flat_mtimes`/`parse_flat_mtime_map`).

## fn write_updates_mtimes
- Replaces only the `updates` section, preserving the rest verbatim; an empty map stores nothing for `updates` itself. Test-only while the global-updates check is read-only (backlog #575 Task 1 removed the `check_root_updates` call that swallowed its errors, matching Portage's `MtimeDB.__write_to_disk` `except OSError`; backlog #576 Task 11 restores production recording and must drop the `#[cfg_attr(not(test), allow(dead_code))`).

## fn read_raw_objects
- Backlog #575 Task 1: used to extract only the four known sections (`resume`, `resume_backup`, `info`, `updates`) and drop everything else on every write. Now a string-aware top-level walk returning `(key, raw value slice)` for every `"key": <value>` pair at depth 1, whatever the value shape (object, array, string, bare number/literal); keys portuale writes are replaced by the caller, all others splice back verbatim. A `"` that does not open a pair (e.g. inside an array, or a malformed spot) is skipped as a plain string so the walk always terminates with the well-formed pairs intact. Slicing stays on ASCII structural bytes, so multibyte string contents (Portage writes `ensure_ascii=False`) are never split.
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

## tests: foreign_top_level_keys_survive_every_write
- Backlog #575 Task 1: seeds a Portage-shaped file (`ldpath` object, `starttime` number, `version` string per `util/mtimedb.py::_MTIMEDBKEYS`) and asserts an `info` write and a `resume` write each splice all three back verbatim.

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
