# crates/portuale/src/ebuild_merge/tests/run_merge.rs
Full `run_merge`/`run_qmerge` merge tests: preinst/postinst ordering, vdb content, collisions, protect-owned, preserve-libs, blockers.

## fn real_merge_lands_files_and_a_symlink_and_writes_a_real_vdb_entry
- Portage merges via an atomic dbtmpdir-then-rename (`dblink.merge`); no `-MERGING-` temp directory survives a successful merge.
- The `FEATURES=noclean` choice keeps build state the way Portage's `FEATURES=noclean` does, so the hook markers under `${T}` survive for the ordering assertions below.

## fn computed_slot_is_evaluated_in_the_vdb_and_a_reinstall_does_not_abort_under_protect_owned
- Task #149. Expected value from Portage: the mpdecimal probe on the container test bed (`4.0.1` -> `SLOT=4` in the Portage vdb, and `build-info/SLOT` = `4` from the install phase writing the evaluated `${SLOT}`); for this fixture `ver_cut` of `1.0` evaluates to `1`.
- Before the fix, `merge_after_install` derived `full_slot` by regex-parsing the raw ebuild text (`parse_slot`), so the literal `$(ver_cut 1)` landed in the vdb, and every slot-keyed consumer (`installed_instance_pf`/`find_collisions`/`blocked_installed_packages`/`unmerge_replaced_same_slot`, all fed `full_slot.split('/')`) keyed off the same corrupt value. On an image whose `dev-libs/mpdecimal-4.0.1` was installed by Portage (evaluated `SLOT=4`), that mismatch made `find_collisions` refuse to credit the installed instance, so a reinstall under `FEATURES=protect-owned` aborted with `NOT merged due to file collisions`, the exact #149 symptom.
- A *computed* `SLOT="$(ver_cut 1)"` is read back as its **evaluated** value -- the value Portage's own metadata generation and `dblink.treewalk` both use.

## fn real_merge_writes_the_full_build_info_into_the_vdb_entry
- Portage's `treewalk()` copies every `build-info` file into the vdb, and `ebuild_phases::write_post_install_metadata` writes the dependency-string metadata files there, so the merged vdb entry carries `RDEPEND`/`EAPI`/`KEYWORDS`/..., not just the former `CATEGORY`/`SLOT`/`repository`/`CONTENTS`/`COUNTER`.
- An empty md5-cache value (`DEPEND=`) is not written at all: Portage unlinks it, and `bin/phase-functions.sh` never wrote it.

## fn run_qmerge_merges_without_rerunning_the_install_phase
- Portage's `bin/phase-functions.sh` `__dyn_install` leaves the `.installed` marker behind; running only the `install` phase directly reproduces what a prior `ebuild <file> install` run leaves for qmerge to find.

## fn upgrade_publishes_the_new_entry_after_unmerging_the_old
- Backlog #183: the position matches Portage's `treewalk()` `_movefile(self.dbtmpdir, self.dbpkgdir)`, after the replace loop.
- The in-between invisibility itself is what the `l32` C4 candidate run checks live: the kill lands while the new entry is still `-MERGING-*`.

## fn re_merging_a_sub_slotted_package_does_not_collide_with_its_own_files
- L3 smoke finding: passing the ebuild's full `slot/sub_slot` to `find_collisions`/`installed_instance_pf` (which compare against the vdb `SLOT` file's main slot) made every file look foreign, so a reinstall aborted under the default `protect-owned`.

## fn run_merge_post_cleans_the_builddir_unless_noclean
- Backlog #42: Portage's `dblink.merge()` tail (`vartree.py:6183-6198`) runs the `clean` phase after a merge unless `FEATURES=noclean`. Leaving `${PORTAGE_BUILDDIR}` behind is what stopped a later `-B` from repackaging an already-`instprep`ped image (#38 S4).
- The `noclean` half is bug #704866's sibling gate: Portage keeps the builddir and `.installed` survives for a later `qmerge`/inspection.
- The `emerge` paths that reach `run_merge` therefore leave no `${PORTAGE_BUILDDIR}` behind; with `noclean` the build state survives.

## fn ordinary_collision_aborts_by_portage_default_via_protect_owned
- Complements `protect_owned_alone_aborts_when_an_owner_is_identified`, which proves the same logic via an explicit `protect_owned: true` rather than relying on the default.

## fn ordinary_collision_aborts_the_merge_and_names_the_owner_when_collision_protect_is_on
- Ports Portage's `dblink._collision_protect` abort; `collisionpkg-a` is named as the owning package via `find_owners`.
- `FEATURES=collision-protect` on: Portage `dblink._collision_protect`'s own abort -- `collisionpkg-c` would overwrite `collisionpkg-a`'s own, different-package `shared.txt`, so the merge aborts *before* writing anything (the file is left exactly as `collisionpkg-a` installed it) and the error names `collisionpkg-a` as the owning package (`find_owners`).

## fn symlink_over_directory_always_aborts_regardless_of_collision_protect
- Dangling/file-target symlinks are the ones Portage's `os.walk` lists in `files` (hence in `linklist`); directory-target symlinks are in `dirs` and never checked.
- PMS 13.4's own symlink-over-directory ban: unconditional, regardless of `FEATURES` -- `collisionpkg-b` installs a file-target (dangling `nowhere`) symlink exactly where `collisionpkg-a` already installed a real directory (`adir`), which aborts the merge even with `collision_protect: false` (`MergeOptions::default()`).

## fn protect_owned_alone_does_not_abort_an_unclaimed_stray_file
- Portage's "None of the installed packages claim the file(s)" case: the distinguishing behaviour from `collision-protect`, which aborts unconditionally on any collision.

## fn preservepkg_new_collides_ordinarily_without_a_registry_entry
- Sanity baseline under portuale's "fixtures must actually distinguish the new behavior" rule: proves the fixture pair is a genuine collision before the registry-exclusion behaviour is shown.

## fn real_merge_creates_a_real_fifo_and_records_a_fif_contents_line
- The FIFO node is created via `mkfifo(1)`, needing no special privilege unlike a device node.
- The `fif` CONTENTS line carries no digest/mtime/target field at all (Portage's `_format_contents_line(node_type="fif", abs_path=myrealdest)`).
- Re-merging over an existing node is a no-op (Portage's `if mydmode is None:` only creates when nothing is there yet); unmerging leaves the node in place, matching Portage's `_unmerge_pkgfiles()` `"fif"`/`"dev"` branches never calling `unlink()` (see `ebuild_unmerge::remove_contents`). The vdb entry itself is still removed as normal either way.
- End-to-end proof of `merge_tree`'s own new `fif` branch: merging a FIFO node (created via `mkfifo(1)`) creates a FIFO at the destination and records a `fif` `CONTENTS` line.

## fn real_merge_regenerates_env_update_outputs_from_its_own_env_d_entry
- The fixture installs its own env.d entry, not a separately-merged package's.
- Portage `merge()`'s own ordering: `env_update()` runs after `postinst`, so it sees the merge's own just-installed `/etc/env.d/50-envupdatetest` and regenerates `/etc/profile.env`/`/etc/csh.env`/`/etc/environment.d/10-gentoo-env.conf`/`/etc/ld.so.conf` from it.

## fn real_merge_invokes_a_real_root_scoped_ldconfig_when_one_is_present
- See `env_update.rs`'s module doc comment for the ROOT-scoped lookup. The "prove it with a marker file" style matches the `pkg_preinst`/`pkg_postinst` ordering tests in this file.

## fn real_merge_copies_a_real_needed_elf2_into_the_vdb
- `NEEDED.ELF.2` is Portage's unmodified `bin/misc-functions.sh install_qa_check` `scanelf`-driven output, generated by the post-install misc-functions step (`ebuild_phases::run_commands_async`) and copied out of `build-info` by Portage's `dblink.merge()` `treewalk()` (`vartree.py:4912-4913`); see `populate_vdb_tmp`'s doc comment for why portuale copies only this one build-info file, not the whole directory.
- The test installs a dynamically-linked ELF binary (`/bin/true`, whatever the host machine has) so `scanelf` has something genuine to report on, typically `libc.so.6`.
- #39: the install-phase `_post_src_install_soname_symlinks` rewrite adds Portage's 6th multilib-category field, so every rewritten vdb line carries six `;`-separated fields.

## fn real_unmerge_preserves_a_still_needed_shared_library
- Wires portuale's `preserve_libs_on_unmerge` (see its doc comment) into `ebuild_unmerge::run_unmerge`. The consumer is genuinely linked against the library (`DT_NEEDED: libpreservetest.so.1`, baked in by `gcc` at fixture build time; see the two fixture ebuilds' comments for why each independently rebuilds a throwaway same-sonamed copy to link against).
- Portage's `_find_libs_to_preserve` finds the still-installed consumer's `NEEDED.ELF.2` entry needing this soname; the library file is filtered out of `CONTENTS` before `remove_contents`'s per-file loop ever sees it (see `ebuild_unmerge::remove_contents`'s `preserved_paths` doc comment), and the on-disk registry records it under the package's `category/pn:slot` key.
- Portage's `_prune_plib_registry()` tail: once the last consumer is gone too, the preserved library is orphaned and the next merge/unmerge deletes it and clears the registry (here the consumer's own unmerge does it, `unmerge_no_replacement=True`).

## fn mergeblockerpkg_collides_ordinarily_without_config_resolution
- Sanity baseline under the fixtures-must-distinguish rule: with `MergeOptions::default()`'s deliberately-inert `config_root` sentinel (see that field's doc comment), config/USE resolution never runs, so `blocked_installed_packages` degrades to an empty set.

## fn mergeblockerpkg_excludes_the_collision_via_a_real_blocker_atom
- Resolution is rooted at `config_root` (the `fixtures` tree, which has its own `repos.conf`).
