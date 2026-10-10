//! The installed-package database ("VDB") behind one interface
//! (feat#157, backlog #305).
//!
//! Design: `LLM/feat-157-authoritative-vdb-database.md` (authoritative)
//! and `docs/vdb_to_db.md` §6–§11. Plan: `LLM/02.305-vdb-backends.opus.md`.
//! The method list comes from the S0 inventory
//! (`LLM/evidence/305-s0-path-inventory.md` §4, needs N1–N16 in §5) and
//! the write order from `LLM/evidence/305-s0-vartree-write-order.md`.
//!
//! - [`InstalledDb`] is the read side plus [`InstalledDb::begin_write`];
//!   [`WriteTxn`] is the write side.
//! - [`register`] / [`for_root`] is the process-wide registry
//!   (`root → Arc<dyn InstalledDb>`). An unregistered root gets a cached
//!   [`FilesDb`], so `emerge` (which never registers) stays on the
//!   historic `var/db/pkg` layout.
//! - [`FilesDb`] is that layout. S1.2 moved the per-entry and `aux` reads
//!   behind it (`generation`, `category_generation`, `entries`,
//!   `category_entries`, `has_entry`, `aux_get`, `file_meta`,
//!   `list_files`, `read_file`); S1.4 moved the merge's writes
//!   (item 20); the other methods still return [`Error::Unsupported`]
//!   until S2+ move today's code.
//!
//! This crate sits **below** `portage-repo` and depends only on
//! `portage-util`. Atom
//! parsing, `USE` reduction, version splitting (`split_pf`) and the
//! package-move remapping (`installed_cp_sources`) stay above it.
//!
//! # Changes from the feat#157 §6.1 first cut
//!
//! Every deviation from `vdb_to_db.md` §6.1, with its reason. Later steps
//! build on this list.
//!
//! 1. **`Cpv` is [`EntryKey`] `{ category, pf }`**, the stored directory
//!    name, not a split `cat/pkg-ver`. Splitting needs `ververify`
//!    (`portage-versions`/`portage-repo`), and several sites only hold the
//!    `pf` (`owns_path_pf`, `find_owners`). The key is always the stored
//!    (pre-`move`) name; the move fallback stays in `portage-repo` and
//!    calls the trait with each candidate name (N13).
//! 2. **`begin_entry(image) -> EntryId` is split into steps that keep
//!    today's write order** (S0.1 rows 3–16):
//!    [`WriteTxn::begin_entry`] creates the empty pending entry (it runs
//!    before `pkg_preinst`: the `l32` C4 killed-mid-merge invariant, #183);
//!    [`WriteTxn::copy_entry_file`] / [`WriteTxn::put_entry_file`] add
//!    files (N3); [`WriteTxn::next_counter`] ticks the counter (N1);
//!    [`WriteTxn::seal_entry`] writes the consolidated `metadata` file,
//!    stamp last; [`WriteTxn::finish_entry`] publishes. N1 asked for
//!    `begin_entry` to return the counter, but the counter is ticked
//!    after the build-info copy (`populate_vdb_tmp`), which is after
//!    `pkg_preinst`, so one call cannot do both without moving a write.
//!    `next_counter` returns the value before any payload lands, which
//!    is what N1 needs.
//! 3. **The pending entry is named by its [`EntryKey`], not an
//!    `EntryId`.** It outlives a transaction: the database backends
//!    commit the `merging` state first and finish in a later transaction
//!    (design §9), and `files` has at most one `-MERGING-<pf>` per key.
//! 4. **Replace order is delete, then publish** (§0.7, `vartree.py:
//!    5219-5238`): the merge calls [`WriteTxn::delete_entry`] for the old
//!    instance before [`WriteTxn::finish_entry`] for the new one.
//! 5. **`snapshot()` is not the hot read path.** A whole snapshot on
//!    `files` would open about 23 files per entry for the 1,500 of 2,130
//!    host entries that have no `metadata` file. The resolver reads
//!    through [`InstalledDb::entries`] (R3), [`InstalledDb::category_entries`]
//!    (R1), [`InstalledDb::aux_get`] (R2, lazy, one field) and
//!    [`InstalledDb::has_entry`] (N13), which keep today's `openat` and
//!    `statx` counts (N8b). [`InstalledDb::snapshot`] stays for the
//!    generation-keyed caches of the database backends (S3.2).
//! 6. **Two cache keys** (N8a): [`InstalledDb::generation`] (files: the
//!    `vdb_fingerprint` of `all_installed_packages`) and
//!    [`InstalledDb::category_generation`] (files: one category
//!    directory's mtime, the `installed_candidates` key, 1–2 `statx` per
//!    call instead of about 30).
//! 7. **Extra reads:** [`InstalledDb::read_pending_file`] (N2; the caller
//!    composes "live, then pending"), [`InstalledDb::file_meta`] (the
//!    exists-tests of `unmerge_one_installed` and `entries_not_merged`
//!    are one `stat`, not a read), [`InstalledDb::read_file_all`] (N11;
//!    it returns every live entry, with `None` where the file is
//!    missing, so `read_all_needed_entries` keeps its empty rows without
//!    a second walk).
//! 8. **`reverse_dependents` returns raw records** (§0.7, N15 option b):
//!    [`DepRecord`] holds the entry's normalised `USE` and the requested
//!    `*DEPEND` strings. The caller (`portage-repo`) reduces and matches
//!    them. The result is a superset of the real dependents: `files`
//!    returns every entry, an indexed backend only the entries whose
//!    reduced atoms name `cp`. The classes are a parameter because
//!    today's scan reads four keys, not five (no `IDEPEND`).
//! 9. **`world()` / `set_world` are split** into `world` + `world_sets`
//!    (N5): each file is read and written alone, and a transaction that
//!    only sets one of them is legal (N4).
//! 10. **[`PreservedLibs`] carries `loaded`** (N6): `set_preserved_libs`
//!     writes nothing when `entries == loaded`.
//! 11. **Counter store access for converters:** [`InstalledDb::counter`]
//!     and [`WriteTxn::set_counter`]. [`Counter`] is `i64`: a missing or
//!     corrupt `counter` file reads as `-1` today, so the first merge
//!     gets `0` (`next_counter`). On `files` the tick is real's
//!     `counter_tick_core` since #306: under the VDB lock, max of the file
//!     and every entry's `COUNTER`, plus one, written atomically.
//! 12. **Whole-entry copy for converters:** [`InstalledDb::entry_image`]
//!     and [`WriteTxn::insert_entry`] with [`EntryImage`], which carries
//!     exact bytes, modes, mtimes and the `metadata` stamp state
//!     ([`MetadataStamp`]). A stale stamp stays stale and no `metadata`
//!     file is ever added (S0.3 corpus).
//! 13. **Paths for the `files` backend:** [`InstalledDb::vdb_dir`] and
//!     [`InstalledDb::entry_path`] (N9: `PORTAGE_UPDATE_ENV`, saved-env
//!     phases; N10: `emerge -C /var/db/pkg/cat/pf`). Both are `None` on
//!     the database backends, and callers then use a scratch copy plus
//!     [`WriteTxn::replace_file`].
//! 14. **[`FileMeta`] keeps the §6.1 meaning** (one stored file: name,
//!     size, mode, mtime) for `list_files`, convert and FUSE. N16's
//!     `CONTENTS`-line record is not a trait type (see "Rejected").
//! 15. **[`aux_get`](InstalledDb::aux_get) serves only the 23
//!     [`METADATA_FILE_FIELDS`]**, normalised like real `_aux_get`
//!     (`" ".join(v.split())`, invalid `SLOT` → `"0"`). Any other key
//!     (`CONTENTS`, `NEEDED.*`) is [`InstalledDb::read_file`], raw bytes.
//!     `None` means "no such entry", so the move fallback above can tell
//!     it from an empty field.
//! 16. **Errors** are one hand-written [`Error`] enum (`#[non_exhaustive]`,
//!     no `thiserror`, which is only a transitive dependency of the
//!     workspace). [`Error::Io`] displays as `"<path>: <io error>"`, the
//!     format every caller prints today.
//! 17. **One copy of the `metadata` format.** [`METADATA_FILE_FIELDS`],
//!     [`METADATA_FILE_FORMAT_VERSION`] and [`in_metadata_file`] are
//!     defined here. `portage-repo` re-exports them (S1.2; the field set
//!     is part of the format and two copies must not drift).
//! 18. **How `FilesDb` realises the S1.2 reads (no interface change).**
//!     The per-entry memo of `aux_get` is thread-local and keyed by
//!     `(vdb dir, category, pf)`, validated by the entry directory's
//!     `st_mtime_ns` on every call, exactly the memo `portage-repo` had
//!     (the registry hands out one `FilesDb` per root, but the memo is
//!     per thread, as before). `aux_get` costs one `statx` of the entry
//!     directory (`Ok(None)` when it is not a directory) and nothing
//!     else on a memo hit. [`InstalledDb::has_entry`] is one `statx`.
//!     [`InstalledDb::read_file`] is one `open` with no existence `stat`:
//!     a missing entry directory and a missing file are both `Ok(None)`
//!     (`NotFound`/`NotADirectory`); callers that need "entry exists"
//!     call `has_entry` first, as the pre-`move` fallback in
//!     `portage-repo` does.
//!     [`InstalledDb::category_generation`] is one `statx` of the
//!     category directory. [`InstalledDb::snapshot`] stays unimplemented
//!     until S3.2 (nothing on `files` uses it).
//! 19. **S1.3: `FilesDb` does not implement `reverse_dependents`; the
//!     scan stays in `portage-repo` on top of the S1.2 reads.** Today's
//!     `installed_reverse_dependents` is not a directory scan of its own:
//!     it walks the memoised `all_installed_packages` set and reads `USE`
//!     and the four `*DEPEND` keys of each entry through `vdb_aux_get`,
//!     which is already [`InstalledDb::aux_get`] (memo, `metadata`
//!     snapshot, package-move fallback). Reproducing that inside `FilesDb`
//!     would need the move-remapped, memoised installed set (which lives
//!     above this crate) or an uncached [`InstalledDb::entries`] walk (extra
//!     `statx`). So on `files` the reverse-dependent scan keeps its exact
//!     syscall pattern by calling `aux_get`; `FilesDb::reverse_dependents`
//!     stays [`Error::Unsupported`] until a database backend needs it
//!     (S2). The raw-record shape of item 8 is unchanged. The same step
//!     moved: `FilesDb::entries` now serves `all_installed_packages`'
//!     uncached walk; `has_entry` serves the entry-exists tests and the
//!     move-fallback resolution (`resolve_vdb_entry`); `read_file` serves
//!     `installed_contents_files` (whole-file UTF-8 check stays with the
//!     caller) and `read_vdb_env_vars`.
//! 20. **S1.4: how `FilesDb` realises the merge's writes (no interface
//!     change).** `ebuild_merge`'s five VDB steps call the trait in
//!     today's order, each step opening and committing its own
//!     transaction (the pending entry outlives a transaction, item 3):
//!     `create_vdb_tmp` → [`WriteTxn::begin_entry`] (`stat`,
//!     `remove_dir_all` of a stale `-MERGING-<pf>`, `create_dir_all`);
//!     `populate_vdb_tmp` → [`WriteTxn::copy_entry_file`] per build-info
//!     file (`std::fs::copy`), [`WriteTxn::next_counter`], then
//!     [`WriteTxn::put_entry_file`] for `CATEGORY`, `SLOT`, `repository`
//!     and `COUNTER`; `write_vdb_tmp_contents` → `put_entry_file`
//!     (`CONTENTS`) then [`WriteTxn::seal_entry`] (the former
//!     `write_consolidated_metadata_file`: body, `stat`, stamp appended
//!     last); `publish_vdb_tmp` → [`WriteTxn::finish_entry`] (`stat`,
//!     `remove_dir_all` of a live same-`pf` entry, `rename`). The
//!     replaced same-slot entries are still deleted by
//!     `ebuild_unmerge::delete_vdb_dir` between the last two steps (S1.5
//!     routes it to [`WriteTxn::delete_entry`]), so the replace order of
//!     item 4 holds. **`files` makes no write atomic and takes no lock
//!     (the counter tick alone does, #306): every call is applied when it is made, [`WriteTxn::commit`] is a
//!     no-op ordering point, and a failure leaves what was written,
//!     exactly as before S1.4.** [`InstalledDb::begin_write`] does no I/O.
//!     The counter tick is real's (item 11, #306): VDB lock, max over the
//!     entries' `COUNTER`s, atomic write.
//!     The preserved-libs registry and the config memory are read and
//!     written through [`InstalledDb::preserved_libs`] /
//!     [`WriteTxn::set_preserved_libs`] and
//!     [`InstalledDb::config_memory`] / [`WriteTxn::set_config_memory`]
//!     by every caller of `ebuild_merge`'s `read_plib_registry`,
//!     `write_plib_registry`, `read_cfgfiledict` and `write_cfgfiledict`
//!     (unmerge included, since the helpers are shared); the `files`
//!     formats (real `json.dumps` registry, `"path md5"` lines) moved
//!     here as [`parse_preserved_libs`] / [`format_preserved_libs`] and
//!     the private config-memory reader/writer. `pruneNonExisting`
//!     stays with the caller (N6). [`InstalledDb::read_pending_file`]
//!     serves the merge's reads of the `-MERGING-` entry
//!     (`replacement_needed_entries`, the `read_contents_pf` fallback),
//!     and [`InstalledDb::entry_path`] gives `PORTAGE_UPDATE_ENV` its
//!     `<entry>/environment.bz2` (N9). [`InstalledDb::counter`] reads the
//!     store without ticking. `populate_vdb_tmp` rejects a build-info
//!     file whose name is not UTF-8 (entry file names are `String`, see
//!     "Text and bytes"); before S1.4 it was copied. No such name exists:
//!     build-info names are fixed ASCII keys and `<PF>.ebuild`.
//!
//! 21. **S1.5: unmerge, the W4 rewrites and the scans (one interface
//!     addition).** [`WriteTxn::delete_entry`] is the old
//!     `delete_vdb_dir`: `remove_dir_all` of the entry (its error is
//!     returned), then a best-effort `remove_dir` of the category
//!     directory (N14). [`WriteTxn::replace_file`] is the two
//!     `std::fs::write` calls of `remove_from_contents` (`CONTENTS`, then
//!     `NEEDED.ELF.2` only when something was removed and the file was
//!     readable): in place, no temporary file, so the entry directory's
//!     mtime and the `metadata` stamp are untouched. The reads of the
//!     unmerge (`CONTENTS`, `COUNTER`, `DEFINED_PHASES`, the
//!     `environment.bz2` and `<pf>.ebuild` exists-tests) are
//!     [`InstalledDb::read_file`] / [`InstalledDb::file_meta`]; the saved
//!     environment and ebuild still go to bash by path
//!     ([`InstalledDb::entry_path`], N9). [`InstalledDb::read_file_all`]
//!     is the walk of `read_all_needed_entries`, moved unchanged: every
//!     category is listed and tested first, then each category's entries
//!     are listed, tested and read in turn; a file that cannot be read for
//!     any reason is `None`. [`InstalledDb::owners`] is the walk of
//!     `find_owners` plus the per-entry `CONTENTS` read of
//!     `installed_contents_files`; the `<package>-<version>` split and the
//!     package-move fallback stay above the crate (`find_owners` drops the
//!     claims of an entry name that does not split, which the old loop
//!     skipped before reading its `CONTENTS`). **New:**
//!     [`InstalledDb::categories`] lists the category directories, so a
//!     caller that scanned category by category (`blockers_from_flat_deps`)
//!     keeps its order (list a category, read each entry's `SLOT`, next
//!     category) with [`InstalledDb::category_entries`], which also drops
//!     the per-entry `is_dir` `stat` (the entry still has to be a
//!     directory, by `d_type` or `stat`). `unmerge_replaced_same_slot`
//!     lists its category the same way, so a stray non-directory named
//!     like a version is no longer taken for an installed instance.
//!
//! 22. **S1.6: `world` / `world_sets` / `set_world` / `set_world_sets`
//!     on `files` (no interface change).** The readers are
//!     `pretend::read_world_atoms` / `read_world_sets` moved unchanged:
//!     one `read_to_string`, `NotFound` (and only it) is an empty store,
//!     any other failure is [`Error::Io`] (the caller adds its own
//!     `reading ` prefix), lines are trimmed and blank / `#` lines are
//!     dropped (`world` also drops `@` lines, `world_sets` keeps only
//!     them and strips every leading `@`). The writers are the three
//!     rewrites of `pretend` (`update_world_file`, `deselect_from_world`,
//!     `run_deselect`) moved unchanged: the caller sorts and
//!     de-duplicates; `create_dir_all` of `var/lib/portage`, the lines
//!     joined by `\n` plus a trailing `\n` (an empty list writes an empty
//!     file), a plain `std::fs::write`: no temporary file, no lock, not
//!     atomic. One transaction creates `var/lib/portage` once: a second
//!     world write of the same transaction (`--deselect` rewrites both
//!     files) skips the `create_dir_all`, as the old single `create_dir_all`
//!     did. A transaction holding only `set_world` (or only
//!     `set_world_sets`) is legal. The "Recording ... in world favorites
//!     file" lines stay in `pretend`. On a bare VDB directory the reads
//!     are empty and the writes are [`Error::Unsupported`].
//!
//! 23. **S4.1: the merge on a database backend (two interface
//!     additions).** [`WriteTxn::finish_entry_replacing`] deletes the
//!     replaced same-slot entries and publishes the pending one in one
//!     transaction (default: `delete_entry` each, then `finish_entry`, the
//!     order of item 4); [`InstalledDb::replace_in_publish`] tells the
//!     merge whether to use it (`files`: `false`, database backends:
//!     `true`). **`files` is unchanged**: the replace loop still removes
//!     each old directory after its `pkg_postrm`, and `publish_vdb_tmp` /
//!     the preserved-libs registration are the S1.4 calls. On a database
//!     backend one merge with a same-slot replace commits, in order:
//!     (1) `begin_entry` (the `merging` row, before `pkg_preinst`);
//!     (2) the build-info files, `CATEGORY`/`SLOT`/`repository`, and
//!     `next_counter` + `COUNTER` (the high-water mark is consumed here,
//!     so a crash never hands a published value out again); (3) the config
//!     memory of the payload merge; (4) `CONTENTS` + `seal_entry`; then
//!     the D4 writes the replace loop makes for each old instance (W4
//!     `replace_file` on preserved-lib owners, the registry, the stale
//!     config memory), each its own commit, **without** deleting the old
//!     row; then (F) **one commit**: `finish_entry_replacing(new, olds)`
//!     (old rows gone, new row `installed`, derived tables,
//!     `counter_hwm = max(hwm, COUNTER)`) plus the merge's preserved-libs
//!     registration (`set_preserved_libs`, counter read from the pending
//!     entry). After it: the `pkg_postinst` `environment.bz2` rewrite
//!     (one `replace_file` commit) and the post-merge preserved-libs
//!     prune. Commits 1–4 touch only the invisible `merging` row (plus
//!     the counter and D4 stores), so until (F) every reader, `has_version`
//!     included, sees the old instance; a crash anywhere before (F)
//!     leaves the old instance installed and a `merging` orphan (design
//!     §9.1). Commits 2 and 4 cannot join (1) or (F): `pkg_preinst` runs
//!     between (1) and (2), and the replace loop reads the pending
//!     `CONTENTS`/`NEEDED.ELF.2` that (4) and (2) store. The D4 writes
//!     before (F) stay separate because the next step of the merge reads
//!     them back (the loop re-reads the registry and the config memory).
//!     `world` is written by `pretend` at the end of the run (§0.7).
//!     The replace loop tells a later old instance's unmerge which
//!     earlier ones it already unmerged (their rows are still live), so
//!     `others_in_slot` matches `files`. **Paths for bash and copiers
//!     (N9)**: [`materialize_files`] / [`materialize_entry`] write the
//!     needed files of a live entry into a scratch directory
//!     (`PORTAGE_UPDATE_ENV` → `<builddir>/vdb-update-env/`, the saved-env
//!     phases → `<scratch>/vdb-entry/<cat>/<pf>/`, `quickpkg` → the whole
//!     entry there) and [`absorb_file`] stores a rewritten file back
//!     with one `replace_file` commit when its bytes changed. `files`
//!     keeps [`InstalledDb::entry_path`].
//!
//! 24. **S4.2: a standalone unmerge on a database backend (no interface
//!     change).** `mrg -C` / `--depclean` (`unmerge_one_installed`) and
//!     `ebuild <file> unmerge` (`run_unmerge`) use
//!     [`InstalledDb::replace_in_publish`] (true off `files`) as the switch
//!     again: the unmerge collects its D4 writes in memory and commits them
//!     with the row's deletion in **one transaction** after `pkg_postrm`
//!     (`ebuild_unmerge::retire_entry`): the W4 `replace_file`s of the
//!     *other* installed entries whose preserved libraries the prune removed,
//!     [`WriteTxn::set_preserved_libs`] (the registry as the unmerge left it;
//!     `loaded` is the stored registry, so an unchanged one writes nothing),
//!     [`WriteTxn::set_config_memory`] (the `stale_confmem` prune) and
//!     [`WriteTxn::delete_entry`]. Exactly one generation step per unmerge.
//!     Until that commit every reader still sees the entry (its payload
//!     files are already gone, as on `files` before `delete_vdb_dir`), and
//!     `prerm`/`postrm` read the scratch copy of item 23. The steps that read
//!     what an earlier step wrote (the prune reads the registry
//!     `preserve_libs_on_unmerge` just updated) read the in-memory copy, not
//!     the store. What stays separate: the world files (`deselect_from_world`
//!     in `pretend.rs`, a second transaction after the unmerge, as on
//!     `files`; `--deselect` has no unmerge); the replace loop of a merge
//!     (S4.1, item 23: its D4 writes precede the publishing commit and are
//!     read back by the next step); and a failed unmerge, which leaves the
//!     registry, config memory and rows as they were (on `files` the registry
//!     write of `preserve_libs_on_unmerge` would already have landed).
//!     **`files` is unchanged**: no collector exists there, and the same
//!     calls run in the same order as in S1.5.
//! 25. **S4.3: sweeping orphans (one interface addition).** A merge killed
//!     between its two commits leaves a `merging` row (`files`: a
//!     `-MERGING-<pf>` directory) that [`InstalledDb::pending_entries`]
//!     lists. Nothing removes it automatically: `mrg` warns at startup on a
//!     database backend (once, before its own merge, whose pending entry is
//!     legitimate), `portuale vdb status` lists it and `portuale vdb sweep`
//!     calls [`WriteTxn::discard_pending`], which deletes only a pending
//!     entry (an installed one of the same key is untouched; a key that is
//!     not pending is [`Error::Invalid`]).
//! 26. **S7.2: the read-only FUSE view (one interface addition).**
//!     [`InstalledDb::entry_stat`] is [`InstalledDb::entry_image`] without
//!     the bytes ([`EntryStat`]): the view needs every entry's directory
//!     mtime for the category and root mtimes, and must not read every
//!     blob to get it. Every backend overrides the default (sqlite and
//!     redb read the metadata rows only; `files` lists the directory and
//!     reads just the small `metadata` file to judge its stamp).
//!     `FilesDb::read_file_at` is implemented (`pread`), so a mount of
//!     `files` is a pass-through. The view itself is `portuale`'s
//!     `vdb_view.rs`; `fuser` is not a dependency of this crate.
//!
//! # SqliteDb (feature `vdb-sqlite`)
//!
//! S2.3 adds the schema and `open` / `open_readonly`; S2.4 the read side;
//! S2.5 the write side (below). Every [`WriteTxn`] and [`InstalledDb`]
//! method is implemented; nothing is [`Error::Unsupported`].
//!
//! - **Reads (S2.4)** return what `FilesDb` returns for the same logical
//!   content (unit tests in `sqlite.rs` seed both and compare every read).
//!   Only `state = 'installed'` rows are live; `read_pending_file` reads
//!   the `merging` row. Lists are ordered by `(category, pf)` / name,
//!   byte order, like `FilesDb`'s sorted directory reads. `aux_get`
//!   follows the stored `metadata_stamp`: `valid` serves the stored
//!   `metadata` file as a complete snapshot (so a field the sealing
//!   dropped, e.g. non-UTF-8, is `""`); `absent`/`stale` read the field
//!   file, lossy and whitespace-joined; invalid `SLOT` is `"0"`. The
//!   extracted `slot`/`subslot`/`repo` columns are not used by reads.
//!   `owners` reads the `owner` table (S8.2: an indexed lookup of each
//!   distinct path, spelled `/p` and `p`, with `claim_paths`' rule, the
//!   first matching input path, `(category, pf)` then `CONTENTS` order).
//!   `category_generation` is the global `meta.generation`.
//!   `counter` is `meta.counter_hwm`, `-1` reading as `None`.
//!   `read_file_at` is `substr` on the blob. `snapshot` is built from the
//!   field files under the `aux_get` rules in one read transaction.
//!   `reverse_dependents` reads `dep_atom` (S8.1): the live entries with a
//!   row for `cp` or for the unsure marker `""` in a requested class.
//!   Known difference: `categories` lists only categories with a live entry.
//!   (`world`, `world_sets` and an empty-path preserved-libs entry round-trip
//!   as `files` does since S2.5.)
//!
//! - **Write transactions (S2.5).** The callers (S1.4 wrappers) open a
//!   transaction, make a few calls and commit, and one merge spans several
//!   transactions (`begin_entry` and the file puts in one, `finish_entry` in
//!   a later one), so a pending entry is a **committed** `state = 'merging'`
//!   row and `merging` rows survive a crash (design §9.1). Each
//!   `SqliteTxn` is one SQLite transaction on **its own connection**,
//!   started with `BEGIN IMMEDIATE` in [`InstalledDb::begin_write`] (a second
//!   writer waits up to the 5 s `busy_timeout`, then [`Error::Backend`]),
//!   committed by [`WriteTxn::commit`] and rolled back when dropped without
//!   a commit. Because it owns a connection, reads on the [`SqliteDb`] during
//!   an open transaction do not block (WAL) and see the committed state.
//!   A read-only handle refuses `begin_write` with [`Error::Invalid`].
//! - **Generation policy.** `meta.generation` goes up by exactly 1 in every
//!   committed transaction that wrote anything (a transaction with no
//!   effective write, or an unchanged `set_preserved_libs`, does not
//!   bump), D4-only ones included. `files` has no counter (its key is a
//!   directory-mtime fingerprint that D4 writes do not move); the portable
//!   contract only needs a change after entry-level commits, and bumping
//!   more can only invalidate caches more often.
//! - **Entry writes.** `begin_entry` drops a stale `merging` row and inserts
//!   a fresh one (an `installed` row of the same key stays: the schema is
//!   `UNIQUE (category, pf, state)`). `put_entry_file` / `copy_entry_file`
//!   store a regular file (`0100644`, or the source's mode) with the mtime
//!   of the call; a file with a *new name* added to an entry whose stamp is
//!   `valid` makes it `stale`, as a new name moves a directory mtime on
//!   `files`. `seal_entry` stores the `metadata` bytes `FilesTxn` writes
//!   (the `#dir_mtime=` line carries `entry.dir_mtime_ns`) and sets the
//!   stamp `valid`. `finish_entry`, in one transaction, deletes the
//!   `installed` row of the same key (cascade), flips the `merging` row,
//!   fills the extracted columns and the derived tables and raises
//!   `meta.counter_hwm` to the entry's `COUNTER` when larger.
//!   `insert_entry` stores the image as given (files, modes, mtimes,
//!   directory mode and mtime, stamp state; never a `metadata` file it did
//!   not get), replaces a live row of the same key, fills the derived
//!   tables and leaves `counter_hwm` alone (the converter calls
//!   `set_counter`). `delete_entry` removes the `installed` row only (a
//!   pending replacement survives) and a missing entry is
//!   [`Error::Invalid`], as are puts and `finish_entry` without a pending
//!   entry. `replace_file` keeps the file's mode and every other file, the
//!   stamp state and the directory mtime, and refreshes what derives from
//!   that file. `next_counter` is `counter_hwm + 1` stored in the same
//!   transaction (atomic, unlike `files`; a dropped transaction does not
//!   consume a value); `set_counter` is a plain store.
//! - **Derived tables** exist only for `installed` rows and are refilled
//!   from the stored files, so they can be rebuilt. `owner`: the
//!   `claim_paths` line rule (kind in `obj sym dir dev fif bin`, path as
//!   written, `obj` md5 and mtime, `sym` target and mtime, `seq` in
//!   `CONTENTS` order; a non-UTF-8 `CONTENTS` gives no rows, like `files`).
//!   `needed`: `NEEDED.ELF.2` lines with at least five `;` fields (`arch`,
//!   `obj` bytes, `soname`, `rpath`, `needed`; the no-rpath sentinel is
//!   `""`). `dep_atom`: a **prefilter** index (no `portage-dep`, item 8):
//!   one row per distinct `(class, cp, token)` for every token of the five
//!   `*DEPEND` fields that is clearly `category/package`, found by a small
//!   string routine (`dep_cp`): skip `( ) || ^^ ??` and `flag?`; strip `!`
//!   or `!!` and one operator (`<= >= < > = ~`); cut at `[` and `:`; require
//!   a `[A-Za-z0-9+_.-]` category, a `[A-Za-z0-9+_-]` name, and, after an
//!   operator, a stripped `-<version>[-rN][*]` (an unrecognisable version
//!   skips the token), or, with no operator, a name that does not look
//!   versioned. A superset of the real atoms is the goal; the caller of
//!   `reverse_dependents` still reduces `USE` and matches. **S8.1:** every
//!   token that is neither structure (`( ) || ^^ ??`, `flag?`) nor read by
//!   `dep_cp` is stored with `cp = ""` (the unsure marker), and
//!   `reverse_dependents(cp)` returns the entries with a row for `cp` **or**
//!   for `""`, so an entry whose deps could not be fully indexed is never
//!   lost. The index is derived from the stored `*DEPEND` files, so it
//!   assumes a `valid` `metadata` snapshot agrees with them (what the stamp
//!   asserts); a database written before S8.1 has no markers and needs its
//!   index rebuilt (S8.3). The columns `slot`, `subslot` (the slot when there is no `/`),
//!   `repo` and `counter` come from `SLOT` (as `aux_get` serves it),
//!   `repository` and `COUNTER`.
//! - **Schema edits in S2.5** (nothing is released, so `SCHEMA_VERSION`
//!   stays 1): `UNIQUE (category, pf, state)`; `preserved_lib.path`
//!   nullable (an entry with no paths is one NULL row); `world` and
//!   `world_sets` keyed by `pos` so the written order is kept.
//!
//! - **Schema version policy**: `meta.schema_version` (1,
//!   [`SCHEMA_VERSION`]) is independent of [`METADATA_FILE_FORMAT_VERSION`] (plan §1, Q4).
//!   A database with another value, no `meta` table, or that is not a
//!   SQLite file is refused with [`Error::Corrupt`]; there is no silent
//!   migration. Engine failures are [`Error::Backend`]; a missing file
//!   for `open_readonly` is [`Error::Io`].
//! - **Pragmas** (`open`): `journal_mode=WAL` (refused with
//!   [`Error::Invalid`] if the filesystem will not take it),
//!   `synchronous=FULL`, `busy_timeout=5000`, `foreign_keys=ON`. Local
//!   filesystems only; a network filesystem is not detected.
//! - **Tables**: `meta`, `entry` (keyed by `(category, pf)`, `state`
//!   `merging`|`installed`, `metadata_stamp`, dir mode and mtime),
//!   `entry_file` (the truth, bytes), and the rebuildable `owner`,
//!   `dep_atom`, `needed`, plus `preserved_lib`, `world`, `world_sets`,
//!   `config_memory`. `meta` holds `schema_version`, `generation`,
//!   `counter_hwm` (`-1` = none yet), `created_at`, and optionally
//!   `imported_files_generation` and `imported_files_source` (the
//!   source's `generation()` and root path when converted from a files
//!   backend, used to detect stale conversions).
//! - **Import mark** When `copy_all` converts from a files backend to a
//!   database backend, it records the source's `generation()` value and
//!   source path as `meta` keys `imported_files_generation` and
//!   `imported_files_source`. This allows tools to warn when the source
//!   files VDB has changed since the database was created and a re-conversion
//!   is needed.
//!
//! # RedbDb (feature `vdb-redb`)
//!
//! S5.2 adds the tables, `open` / `open_readonly` and the read side; S5.3
//! the write side (every [`WriteTxn`] method; `begin_write` is
//! [`Error::Invalid`] on a read-only handle). Details are in the module doc
//! of `redb_db.rs` and `redb_db/write.rs`.
//!
//! - **One process only.** redb locks the file for a read-write handle, so
//!   a second open (read-write or read-only, any process, also a second
//!   handle in the same process) fails with [`Error::Busy`], which names the
//!   file and says redb allows one process at a time. Several read-only
//!   handles may coexist. S5.5 surfaces this in `convert` and `vdb status`.
//! - **State model as sqlite**: `entry` is keyed by `(state, category, pf)`
//!   (`0` merging, `1` installed), so a pending entry is a committed row
//!   that outlives a transaction, beside a live row of the same key. File
//!   bytes are stored in chunks of 64 KiB (`entry_file_chunk`, with a
//!   `(len, mode, mtime)` record in `entry_file_meta`), and `read_file_at`
//!   loads only the chunks it needs. Values are fixed little-endian binary
//!   records, no serde.
//! - **Writes follow `SqliteTxn` method by method** (same semantics,
//!   errors, generation policy, counter rule, stamp handling and derived
//!   rows). One [`WriteTxn`] is one `redb::WriteTransaction`, committed by
//!   `commit` and aborted when dropped. redb allows one write transaction
//!   per `Database` and its `begin_write` **blocks** until the open one
//!   ends: another thread simply waits, but a second `begin_write` on the
//!   thread that already holds one would wait for itself, so it returns
//!   [`Error::Invalid`]. Callers hold one transaction at a time (the merge
//!   already does: each transaction is short). Reads during a transaction
//!   do not block and see the committed state. `owner`, `dep_atom` (the
//!   shared `dep_cp` prefilter) and `needed` are filled in the same
//!   transaction (row encodings: `redb_db/write.rs`); `meta.generation` is
//!   bumped on commit when anything was written.
//! - **Reads equal sqlite's** (and `FilesDb`'s): same stamp rule, same
//!   `owners` rule, `category_generation` = `generation`, `categories` lists
//!   only categories with a live entry. The listing and the 23-field
//!   snapshot are cached in process and validated against `meta.generation`
//!   (read in the same read transaction) on every call; redb being
//!   single-process, only this handle's own commits can change the file.
//! - **Schema version** [`REDB_SCHEMA_VERSION`] (1) in `meta`; another
//!   value, a redb file without the tables or a non-redb file is
//!   [`Error::Corrupt`]. `meta` also holds `generation` (0 at creation),
//!   `counter_hwm` (-1) and `created_at`.
//!
//! **Rejected from N1–N16:** N10 `cpv_for_path` (the caller prints the
//! canonical VDB directory in its errors, so it needs
//! [`InstalledDb::vdb_dir`], not a key); N16's `CONTENTS`-shaped
//! `FileMeta` (today's `CONTENTS` readers differ: token rules, the six
//! kinds of `installed_contents_files`, whole-file UTF-8 failure in
//! `read_contents_pf`, so a shared parsed type would change one of them;
//! they keep parsing [`InstalledDb::read_file`] bytes, and the indexed
//! backends parse `CONTENTS` internally for [`InstalledDb::owners`]);
//! N8b's `entry(cpv)` returning all 23 fields (replaced by per-field
//! `aux_get`, item 5). N1 and N2 are taken in a different shape (items 2
//! and 7). N4, N5, N6, N7, N9, N11, N12, N13, N14 and N15 are taken
//! (N14 is internal to `FilesDb::delete_entry` / `finish_entry`).
//!
//! # Text and bytes
//!
//! File contents, `CONTENTS` paths ([`InstalledDb::owners`]) and
//! [`EntryImage`] data are bytes (design §7.3). Entry keys, file names
//! inside an entry, `aux_get` values, world atoms, preserved-libs records
//! and config memory are `String`, because today's readers decode them
//! (lossy for `aux_get`, like real's `errors="replace"`; `read_to_string`
//! for the others). An entry file whose name is not UTF-8 is reported as
//! [`Error::Invalid`] by `list_files` / `entry_image`; none exists in the
//! S0.3 corpus.

mod convert;
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod dep_cp;
mod error;
mod files;
mod files_write;
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod loaded;
#[cfg(feature = "vdb-redb")]
mod redb_db;
mod registry;
mod scratch;
#[cfg(feature = "vdb-sqlite")]
mod sqlite;
mod types;

pub use convert::{CopyReport, VerifyReport, copy_all, verify};
pub use error::{Error, Result};
pub use files::FilesDb;
pub use files_write::{format_preserved_libs, parse_preserved_libs};
#[cfg(feature = "vdb-redb")]
pub use redb_db::{RedbDb, SCHEMA_VERSION as REDB_SCHEMA_VERSION};
pub use registry::{for_root, register, reset};
pub use scratch::{absorb_file, materialize_entry, materialize_files};
#[cfg(feature = "vdb-sqlite")]
pub use sqlite::{SCHEMA_VERSION, SqliteDb};
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// feat#157 (#305) S2.1: prove that rusqlite links in the vdb-sqlite feature.
/// Returns the rusqlite version string.
#[cfg(feature = "vdb-sqlite")]
pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

/// feat#157 (#305) S5.1: prove that redb links in the vdb-redb feature.
/// Creates a redb Database at path and writes one key in a table.
#[cfg(feature = "vdb-redb")]
pub fn redb_smoke(path: &std::path::Path) -> Result<()> {
    let db = redb::Database::create(path).map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    let write_txn = db
        .begin_write()
        .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    {
        let table_def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("test");
        let mut table = write_txn
            .open_table(table_def)
            .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
        table
            .insert(b"key" as &[u8], b"value" as &[u8])
            .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    }
    write_txn
        .commit()
        .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    Ok(())
}

/// Real `_METADATA_FILE_FIELDS` (`vartree.py:78-104`): the 23 single-line
/// fields of the consolidated `metadata` file, sorted. `CONTENTS` and
/// `NEEDED*` are line-oriented and excluded. The set is part of the
/// format: change it only together with [`METADATA_FILE_FORMAT_VERSION`].
pub const METADATA_FILE_FIELDS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "COUNTER",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "REQUIRES",
    "RESTRICT",
    "SLOT",
    "USE",
    "repository",
];

// [`EntryFields`] stores exactly this many values.
const _: () = assert!(METADATA_FILE_FIELDS.len() == 23);

/// Real `_METADATA_FILE_FORMAT_VERSION` (`vartree.py:105`).
pub const METADATA_FILE_FORMAT_VERSION: u32 = 1;

/// Real `_in_metadata_file(fname)`: whether `name` is one of the
/// [`METADATA_FILE_FIELDS`].
pub fn in_metadata_file(name: &str) -> bool {
    METADATA_FILE_FIELDS.contains(&name)
}

/// The read side of one installed-package database. One instance serves
/// one root (or, for [`FilesDb::open_vdb_dir`], one VDB directory).
///
/// Entries in the `merging` state (`files`: `-MERGING-<pf>`) are never
/// returned by the listing and per-entry methods; only
/// [`InstalledDb::read_pending_file`] sees them. Listing order is the
/// backend's stable order (`files`: `portage_util::read_dir_entries`,
/// sorted by name, as today).
pub trait InstalledDb: Send + Sync {
    /// Which backend this is.
    fn kind(&self) -> BackendKind;

    /// `files` only: the VDB directory (`<root>/var/db/pkg`, not
    /// canonicalised). `None` on the database backends (N10, N12).
    fn vdb_dir(&self) -> Option<PathBuf>;

    /// `files` only: the directory of a live entry, for code that must
    /// hand a path to bash or a copier (N9). Pure path arithmetic, no
    /// I/O; the entry need not exist.
    fn entry_path(&self, key: &EntryKey) -> Option<PathBuf> {
        self.vdb_dir()
            .map(|vdb| vdb.join(&key.category).join(&key.pf))
    }

    /// Cache key for "anything in the database changed". Compare for
    /// equality only. `files`: the `vdb_fingerprint` (VDB dir mtime plus
    /// every category dir mtime, folded), `0` when the VDB is missing.
    /// Database backends: `meta.generation`, bumped by every commit.
    fn generation(&self) -> Result<u64>;

    /// Cheaper cache key for one category (N8a). `files`: that category
    /// directory's mtime in nanoseconds, `0` when missing (the
    /// `installed_candidates` key). Database backends may return
    /// [`InstalledDb::generation`].
    fn category_generation(&self, category: &str) -> Result<u64>;

    /// Every live entry (R3), in listing order.
    fn entries(&self) -> Result<Vec<EntryKey>>;

    /// Every category that exists (`files`: each directory under the VDB),
    /// in listing order. The caller lists each with
    /// [`InstalledDb::category_entries`] (module doc, item 21).
    fn categories(&self) -> Result<Vec<String>>;

    /// The `pf` of every live entry in `category` (R1), in listing order;
    /// empty when the category does not exist. The caller applies its
    /// own `<package>-<version>` prefix rule.
    fn category_entries(&self, category: &str) -> Result<Vec<String>>;

    /// Whether `key` is a live entry (N13).
    fn has_entry(&self, key: &EntryKey) -> Result<bool>;

    /// Entries that are mid-merge (`-MERGING-<pf>` directories on `files`,
    /// `merging` rows on a database), by the key they will be published
    /// under, sorted. Not live: absent from every other read. Converters
    /// report them and never copy them. Default: none.
    fn pending_entries(&self) -> Result<Vec<EntryKey>> {
        Ok(Vec::new())
    }

    /// One of the [`METADATA_FILE_FIELDS`] of a live entry, normalised
    /// like real `_aux_get`: whitespace runs collapsed to one space,
    /// invalid UTF-8 decoded lossy, a present but invalid `SLOT` served
    /// as `"0"`, a missing field as `""`. `Ok(None)` when the entry does
    /// not exist. A key outside the set is [`Error::Invalid`].
    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>>;

    /// Every live entry with all 23 normalised fields. Expensive on
    /// `files` (see the module doc, item 5).
    fn snapshot(&self) -> Result<Arc<Snapshot>>;

    /// The files of a live entry, `None` when the entry does not exist.
    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>>;

    /// Size, mode and mtime of one file of a live entry, `None` when the
    /// entry or the file does not exist (`files`: one `stat`).
    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>>;

    /// The exact bytes of one file of a live entry (R5), `None` when the
    /// entry or the file does not exist.
    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// Up to `len` bytes of one file from offset `off` (FUSE). Short at
    /// end of file; `None` when the entry or the file does not exist.
    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>>;

    /// One file of the **pending** entry for `key` (`files`:
    /// `-MERGING-<pf>`), `None` when there is no pending entry or no
    /// such file (N2). Callers that want "live, then pending" call
    /// [`InstalledDb::read_file`] first.
    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// One named file of every live entry, in listing order (N11). Every
    /// live entry is listed; the value is `None` where the file is
    /// missing.
    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>>;

    /// A whole live entry for a converter: every file with its bytes,
    /// mode and mtime, the directory's mode and mtime, and the
    /// `metadata` stamp state. `None` when the entry does not exist.
    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>>;

    /// [`InstalledDb::entry_image`] without the file bytes: names, sizes,
    /// modes, mtimes, the directory's mode and mtime and the `metadata`
    /// stamp state (S7: the read-only FUSE view stats every entry for the
    /// category and root mtimes and must not read their blobs). `None`
    /// when the entry does not exist. The default derives it from the
    /// image; every backend overrides it with a read that skips the bytes.
    fn entry_stat(&self, key: &EntryKey) -> Result<Option<EntryStat>> {
        Ok(self.entry_image(key)?.map(EntryStat::from))
    }

    /// Candidate reverse dependents of `cp` (R4): one [`DepRecord`] per
    /// entry with its `USE` and the `classes` asked for, in that order.
    /// A superset; the caller reduces and matches (module doc, item 8).
    fn reverse_dependents(&self, cp: &str, classes: &[DepClass]) -> Result<Vec<DepRecord>>;

    /// File owners (R6): every `(path, entry)` pair where a live entry's
    /// `CONTENTS` records one of `paths` (absolute, as `CONTENTS` writes
    /// them). Entries in listing order; within an entry, `CONTENTS`
    /// order.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>>;

    /// The `world` file's atoms (N5). Missing store: empty.
    fn world(&self) -> Result<World>;

    /// The `world_sets` file's set names, without the `@` (N5). Missing
    /// store: empty.
    fn world_sets(&self) -> Result<WorldSets>;

    /// The preserved-libs registry, with `loaded == entries` (N6).
    /// Missing or unparsable store: empty.
    fn preserved_libs(&self) -> Result<PreservedLibs>;

    /// The config-protect memory (N7). Missing store: empty.
    fn config_memory(&self) -> Result<ConfigMemory>;

    /// The counter store (W5), `None` when missing or unparsable. Not
    /// ticked; [`WriteTxn::next_counter`] ticks.
    fn counter(&self) -> Result<Option<Counter>>;

    /// Start a write transaction. Database backends take the write lock
    /// here (sqlite `BEGIN IMMEDIATE`).
    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>>;

    /// Whether a merge deletes the entries of the same-slot instances it
    /// replaces in the publishing transaction
    /// ([`WriteTxn::finish_entry_replacing`]) instead of one by one in
    /// the replace loop (module doc, item 23). `files`: `false`, the
    /// S1.4 order (each old directory is removed right after its
    /// `pkg_postrm`, then the new one is renamed in). Database backends:
    /// `true`, so the old instance stays installed until the one commit
    /// that publishes the new one.
    fn replace_in_publish(&self) -> bool {
        self.kind() != BackendKind::Files
    }

    /// Get the import mark: the generation value and source path at the time
    /// the database was converted from a files backend. `None` if the
    /// database has never been converted from files, or it's a files backend.
    /// Default: none.
    fn import_mark(&self) -> Result<Option<(u64, String)>> {
        Ok(None)
    }
}

/// One write transaction.
///
/// **`files` applies every call at once, in call order, and `commit` does
/// nothing** (N4): nothing is atomic, a failed merge leaves what was
/// written, exactly as today. The database backends apply everything at
/// `commit`, and dropping the transaction without committing rolls it
/// back. Callers therefore make the calls in today's write order
/// (S0.1) and must not rely on rollback.
///
/// A transaction may hold any subset of calls; one with only
/// [`WriteTxn::set_world`] is legal (N4).
pub trait WriteTxn {
    /// Create the empty pending entry for `key` (`files`: remove a stale
    /// `-MERGING-<pf>`, then create it). Readers do not see it.
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Write one file of the pending entry for `key` (N3).
    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Copy the regular file `src` into the pending entry for `key` as
    /// `name` (N3). `files` copies with `std::fs::copy`, as
    /// `populate_vdb_tmp` does (the mode comes with it).
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()>;

    /// Tick the counter store and return the new value (W5, N1). `files`:
    /// under the VDB lock, take the larger of the `counter` file (`-1`
    /// when missing or unparsable) and every entry's `COUNTER`, add one,
    /// write it back atomically (real `counter_tick_core`). The caller writes the entry's
    /// `COUNTER` file with [`WriteTxn::put_entry_file`].
    fn next_counter(&mut self) -> Result<Counter>;

    /// Write the consolidated `metadata` file of the pending entry: body,
    /// then `stat` of the directory, then the `#dir_mtime=` stamp
    /// appended last (`vartree.py:188-229`). No-op when the entry holds
    /// none of the [`METADATA_FILE_FIELDS`]. No file may be added to the
    /// pending entry afterwards.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Publish the pending entry for `key` (`files`: remove a live entry
    /// with the same `pf`, then rename `-MERGING-<pf>` into place).
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Delete the live entries `replaced` (the same-slot instances this
    /// merge replaces), then publish the pending entry for `key`, in this
    /// transaction (design §9, module doc item 23). The default is
    /// [`WriteTxn::delete_entry`] for each, in order, then
    /// [`WriteTxn::finish_entry`]: the replace order of item 4. On a
    /// database backend the whole call commits at once, so no reader sees
    /// both instances or neither. The merge passes an empty `replaced` on
    /// `files`, which already deleted them in the replace loop
    /// ([`InstalledDb::replace_in_publish`]).
    fn finish_entry_replacing(&mut self, key: &EntryKey, replaced: &[EntryKey]) -> Result<()> {
        for old in replaced {
            self.delete_entry(old)?;
        }
        self.finish_entry(key)
    }

    /// Delete the pending entry for `key` (a `-MERGING-<pf>` directory on
    /// `files`, the `merging` row and its files on a database): the
    /// sweep of an orphan a crashed merge left (design §9.1). A live
    /// entry of the same key is never touched. [`Error::Invalid`] when
    /// `key` is not pending.
    fn discard_pending(&mut self, key: &EntryKey) -> Result<()>;

    /// Insert a whole live entry from an image (converters, W1 in one
    /// call). Counters are kept as they are. `files`: writes the files,
    /// applies modes and mtimes, then writes the `metadata` stamp last as
    /// [`EntryImage::metadata_stamp`] says (S2.6).
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()>;

    /// Delete a live entry (W2). `files`: remove the directory, then the
    /// category directory if it is now empty, best effort (N14).
    fn delete_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Rewrite one file of a live entry (W4: `CONTENTS`, `NEEDED.ELF.2`,
    /// `environment.bz2` after `pkg_postinst`). `files`: in place, so the
    /// entry directory's mtime and its `metadata` stamp stay valid.
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Replace the `world` store (N5). `files`: `create_dir_all` of the
    /// parent, then the atoms one per line, each followed by `\n`; an
    /// empty list writes an empty file.
    fn set_world(&mut self, world: &World) -> Result<()>;

    /// Replace the `world_sets` store (N5), same rules as
    /// [`WriteTxn::set_world`]; `files` writes each name as `@<name>`.
    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()>;

    /// Replace the preserved-libs registry. Writes nothing when
    /// `libs.entries == libs.loaded` (N6).
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()>;

    /// Replace the config-protect memory (N7), unconditionally.
    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()>;

    /// Set the counter store (converters; a merge uses
    /// [`WriteTxn::next_counter`]).
    fn set_counter(&mut self, counter: Counter) -> Result<()>;

    /// Record the import mark when converting from a files backend: the
    /// generation value of the source and the source root path. A no-op on
    /// `files`; on database backends, stores `meta` keys `imported_files_generation`
    /// and `imported_files_source`. Default: no-op.
    fn set_import_mark(&mut self, _generation: u64, _source: &str) -> Result<()> {
        Ok(())
    }

    /// Throw away the derived index (`owner`, `dep_atom`, `needed`) and
    /// recompute it from the stored files of every live entry, in this
    /// transaction (S8.3, `portuale vdb rebuild-index`). Repairs rows a
    /// bug or an older build wrote wrong or left out (residue R18: the
    /// unsure `dep_atom` marker). The entry columns and the counter are
    /// not touched. `files` has no derived index: [`Error::Unsupported`].
    fn rebuild_index(&mut self) -> Result<IndexCounts> {
        Err(Error::Unsupported(
            "rebuild_index: this backend has no derived index (S8.3)".into(),
        ))
    }

    /// Make the transaction durable. A no-op on `files`.
    fn commit(self: Box<Self>) -> Result<()>;
}

#[cfg(all(test, feature = "vdb-sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_version() {
        let version = sqlite_version();
        // rusqlite::version() returns a version string like "3.x.y"
        assert!(!version.is_empty());
        // The version should contain at least one digit
        assert!(version.chars().any(|c| c.is_ascii_digit()));
    }
}

#[cfg(all(test, feature = "vdb-redb"))]
mod redb_tests {
    use super::*;

    #[test]
    fn test_redb_smoke() {
        let temp_dir = std::env::temp_dir().join("redb-smoke-test");
        let db_path = temp_dir.join("test.db");

        // Clean up any previous test database
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        // Run the smoke test
        let result = redb_smoke(&db_path);
        assert!(result.is_ok(), "redb_smoke failed: {:?}", result);

        // Clean up
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
