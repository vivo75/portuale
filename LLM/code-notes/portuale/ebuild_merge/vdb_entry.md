# crates/portuale/src/ebuild_merge/vdb_entry.rs
Writing and publishing the VDB entry; env.d updates. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn create_vdb_tmp
- Ports `dblink.treewalk()`'s `self.dbdir = self.dbtmpdir; self.delete(); ensure_dirs(self.dbtmpdir)` step (lib/portage/dbapi/vartree.py), right after the collision-protect abort gate and before `pkg_preinst` runs, long before a single file is copied to `${ROOT}`. The wiped sibling is the `MERGING_IDENTIFIER`-prefixed temporary entry for this exact `category/pf`.
- Both merge paths call this before `pkg_preinst`, so a SIGKILL any time from here on leaves `-MERGING-<pf>` behind exactly like Portage (the `l32` C4 killed-mid-merge invariant) -- and every portuale vdb reader skips such names (see `portage_util::is_merging_vdb_entry`).
- History: the entry used to be written with direct `stat` / `remove_dir_all` / `create_dir_all` calls; since feat#157 S1.4 it goes through the root's `portage_vdb::WriteTxn` (`begin_entry`). The pending entry outlives a transaction (that crate's doc item 3), and on `files` every call is applied at once, so the write order is the call order in the merge.

## fn populate_vdb_tmp
- Ports `dblink.treewalk()`'s info-file + `COUNTER` step (vartree.py), after `pkg_preinst`, before `_merge_contents` copies a single file to `${ROOT}`: the `for x in os.listdir(inforoot): self.copyfile(...)` copy of *every* regular file directly under `${PORTAGE_BUILDDIR}/build-info` -- the `CATEGORY`/`SLOT`/`KEYWORDS`/`IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/… `bin/phase-functions.sh __dyn_install` writes, the `DEPEND`/`RDEPEND`/`LICENSE`/… `ebuild_phases::write_post_install_metadata` adds, `NEEDED.ELF.2` from the `scanelf` QA step, `environment.bz2`, the `<PF>.ebuild` copy -- then the merge-generated files never in `build-info`.
- `CATEGORY`/`SLOT`/`repository` are re-asserted explicitly: a standalone `ebuild <file> install` outside a repo checkout has no `build-info/repository`, and `SLOT` here is the caller-resolved full slot.
- `COUNTER` is ticked here so the replace loop records the new value from the temporary entry. It is `WriteTxn::next_counter`: on `files`, Portage's `counter_tick_core` since #306 -- under the VDB lock, the max of the `counter` file and every installed package's own `COUNTER`, plus one, written atomically. `COUNTER` here is Portage's `cpv_counter`.

## fn write_vdb_tmp_contents
- Ports `dblink.treewalk()`'s `CONTENTS` + metadata-consolidation tail (vartree.py): `CONTENTS` is written into `dbtmpdir` by `_merge_contents` while the image is copied to `${ROOT}`, and `_consolidate_to_metadata_file(self.dbtmpdir)` is the last write into `dbtmpdir` before the rename below.
- Consolidate-after-unmerge (Portage) and consolidate-before-unmerge (here) are the same bytes: neither touches the temporary directory.
- `seal_entry` is that consolidation (moved into `portage-vdb` in S1.4): body, `stat`, `#dir_mtime=` appended last. The `#dir_mtime=` trailer records the dir's `st_mtime_ns` and Portage's reader rejects the file if the dir changed afterwards, so it must come after CONTENTS and be *appended* (a plain write, no new dir entry) after the body is on disk.

## fn publish_vdb_tmp
- Ports `dblink.treewalk()`'s move into place (vartree.py): after the replace loop unmerged every same-slot version, `self.dbdir = self.dbpkgdir; self.delete(); _movefile(self.dbtmpdir, self.dbpkgdir)`. Same-device move, the guarantee Portage's `_movefile()` relies on.
- Readers skip the stale leftover; the next merge's `create_vdb_tmp` wipes it. Shares Portage's own delete-then-move exposure on a same-pf reinstall exactly.
- History: `WriteTxn::finish_entry` since S1.4; on `files` the same `stat`, `remove_dir_all` and `rename` as before.

## fn publish_merged_entry
- The preserved-libs registration is Portage's `vartree.py:5266-5272`.
- On `files`, the replace loop already deleted each entry of `replaced` (`unmerge_one_installed`), so it is unused there.
- Database backends: one transaction (design §9, feat#157 S4.1) via `portage_vdb::WriteTxn::finish_entry_replacing`, with the registry update riding along through `set_preserved_libs`. The counter it records is read from the pending entry, the bytes `populate_vdb_tmp` stored, the same value the published entry carries.

## fn update_env_target
- Ports vartree.py:5334-5337. On `files` the path is `<entry>/environment.bz2`.
- History: the database-backend scratch-copy shape since feat#157 S4.1, N9.

## fn absorb_update_env
- Portage rewrites the file after the entry is published too, so this is a separate commit there as well.
