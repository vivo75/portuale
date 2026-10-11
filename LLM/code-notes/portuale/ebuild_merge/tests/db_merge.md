# crates/portuale/src/ebuild_merge/tests/db_merge.rs
Database-backend equivalence coverage: binpkg/source merges, upgrades, crash recovery, unmerges, and index-vs-scan agreement on sqlite/redb vs files.

## fn binpkg
- No moved context.

## fn use_sqlite
- No moved context.

## fn use_db
- No moved context.

## fn entry_files
- No moved context.

## fn payload_tree
- No moved context.

## fn payload_bytes
- No moved context.

## fn install_hook
- No moved context.

## fn clear_hook
- No moved context.

## fn outcome
- No moved context.

## fn binpkg_merge_matches_files
- Covers design points (a) + (c): see the test bodies for the entry/payload/config-memory comparison and the `PORTAGE_UPDATE_ENV` rewrite observation.

## fn a_binpkg_merged_on_sqlite_matches_the_files_merge
- No moved context.

## fn a_binpkg_merged_on_redb_matches_the_files_merge
- feat#157 S5.4 (c). The fixture ebuilds (`binpkgrmpkg`) run no phase that calls `has_version` / `best_version` (grep of the fixture repo finds none), so this needs no parent pipe (S6.3); a phase that did would reach the held redb file only through that pipe.

## fn a_same_slot_upgrade_on_sqlite_replaces_in_the_publishing_commit
- No moved context.

## mod HookGuard
- No moved context.

## fn crash_before_commit
- No moved context.

## fn vdb_cli
- No moved context.

## fn a_crash_before_the_publishing_commit_leaves_the_old_instance_and_a_sweepable_orphan
- feat#157 S4.5 (design 9.1): the process dies right before the publishing commit of a same-slot upgrade. After a restart (fresh handle) the old instance is still installed with its files, the new one is a pending orphan that `status` reports and `sweep` removes, the generation was not advanced by the aborted commit and the counter is not reused.
- The new payload files landed on disk before the crash and stay there (as in Portage); only documented, not asserted.

## fn a_crash_before_the_retire_commit_leaves_the_entry_installed
- feat#157 S4.5. The payload files were already removed before the crash; not asserted.

## fn a_source_merge_on_sqlite_matches_the_files_merge
- No moved context.

## fn unmerge_standalone
- No moved context.

## fn a_standalone_unmerge_on_sqlite_retires_the_row_in_one_commit
- feat#157 S4.2 (a): `mrg -C` of an installed package on sqlite. The payload goes as on `files`, prerm/postrm ran from the scratch copy of the stored environment, and the row goes in ONE commit: right before it the entry is still installed (and its payload already gone); the generation moved by exactly one.

## fn a_standalone_unmerge_prunes_preserved_libs_in_its_retire_commit
- feat#157 S4.2 (b): the soname-bump scenario of S1.5. After `sonamebumplib` 1.0 -> 2.0 (the old soname is preserved under 2.0), unmerging the last consumer prunes the preserved library: it is removed, the registry is cleared, and the W4 rewrites of `sonamebumplib-2.0`'s `CONTENTS` / `NEEDED.ELF.2` happen. On sqlite they all land in the commit that deletes the consumer's row (one generation step).
- The `/usr/lib64`-in-linker-path seed follows the S1.5 soname-bump test.

## mod Snap
- No moved context.

## fn stable_bytes
- No moved context.

## fn snapshot
- No moved context.

## fn sequence_equivalence
- feat#157 S4.4. Sequence: (1) merge binpkgrmpkg-1.0 (binary), (2) merge mergepkg (source), (3) same-slot upgrade binpkgrmpkg 1.0 -> 2.0, (4) the soname bump sonamebumplib 1.0 -> consumesonamebump -> sonamebumplib 2.0 (preserves the old soname), (5) unmerge consumesonamebump (prunes the preserved library, W4), (6) unmerge mergepkg.
- Both runs use the same ROOT and PORTAGE_TMPDIR paths one after the other, so `environment.bz2` (which records paths) is comparable byte for byte. The sqlite result is also converted to a temporary `files` root with `copy_all` and compared as well; `verify` runs on that pair too.
- Compared per live entry: the file set, every file's bytes and permission bits (group/other write masked) and the `metadata` stamp state; plus the same live-entry set, `world`, `world_sets`, `preserved_libs`, `config_memory`, counter, and the payload trees under ROOT (paths, bytes, symlink targets; no modes).
- Ignored, because run-dependent by nature (and `verify` compares them, so its reported differences are only checked to be among these, the modes only by the umask bits): all file mtimes and the entry directory mtime (each run builds and writes its files at another instant); the `#dir_mtime=` stamp line of `metadata` (derived from the entry directory mtime; its Valid/Stale/Absent state is compared); the mtime field of `obj` / `sym` lines of `CONTENTS` (the payload files' mtimes); the `BUILD_TIME` file and the `BUILD_TIME=` line of `metadata` (a source merge records the time of its build); the entry directory's mode, down to its umask bits (`files` records the merge's `mkdir` under the process umask, 0o755, or 0o775 under another umask; a sqlite merge stores the schema default 0o755; both are permission bits since #307); the group and other write bits of entry file modes (a file written under another umask: 0o664 vs 0o644); the modes of the payload tree (the same umask race, as `payload_bytes` notes). Nothing else is dropped: not `environment.bz2`, not `COUNTER`, not the rest of `metadata`.
- The `/usr/lib64`-in-linker-path seed follows the S1.5 soname-bump test. The end-state sanity assert covers 1 + 2 + upgrade + soname bump - consumer - mergepkg.
- Of a `verify` mode difference (`mode <a> != <b>` / `directory mode <a> != <b>`, octal, permission bits both since #307), only the umask bits may differ.

## fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files
- No moved context.

## fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_redb_and_files
- feat#157 S5.4 (see [`sequence_equivalence`] for what is compared and ignored).

## fn db_kinds
- No moved context.

## fn a_later_replaced_instance_does_not_see_the_earlier_ones_needed_on_a_database
- #310: with two replaced same-slot instances (a broken VDB). On `files` the earlier instance's directory is already gone; on a database it stays installed until the publishing commit. Both old instances consume `libkeep.so.1`, so the preserved `keepA.so` survives the first prune (the second still needs it) and must be pruned by the second, whose own and whose predecessor's consumers are both gone; before the fix a database kept it alive through the first instance's lines.
- Seed detail: the pending entry's `CONTENTS` is `also_keep`'s ownership source, so the file stays on disk through both unmerges.

## fn collision_protect_names_the_owner_from_the_index_on_every_backend
- feat#157 S8.2: `collisionpkg-c` would overwrite `collisionpkg-a`'s `shared.txt`, the merge aborts under `collision-protect` and names `collisionpkg-a` as the owner, and `find_owners` agrees with the files answer.

## fn write_vdb
- No moved context.

## fn reverse_dependents_and_owners_from_the_index_equal_the_scan
- feat#157 S8.1 / S8.2: on a hand-built VDB whose dependency strings carry every kind of token (USE conditional, `||` group, slot operator, blocker, version bounds, a wildcard and bare versioned names the index cannot classify), plus `CONTENTS` lines with odd path spellings, `installed_reverse_dependents` and `find_owners` give the same answer on `files` (the scan) and on every database backend (the index), and the answer is the right one.
- `use_db` converts each alias root's files VDB (the same tree). Another spelling of the same root per backend, so the registry keeps `root` on `files`.

## fn installed_reverse_dependents_for
- No moved context.

## fn fixture_reverse_dependents_are_identical_on_every_backend
- feat#157 S8.1: each database backend converted from the fixture root.
