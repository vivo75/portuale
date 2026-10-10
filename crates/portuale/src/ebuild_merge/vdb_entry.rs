//! Writing and publishing the VDB entry; env.d updates.

use crate::ebuild_merge::*;

/// Real `dblink.treewalk()`'s own `self.dbdir = self.dbtmpdir;
/// self.delete(); ensure_dirs(self.dbtmpdir)` step (`vartree.py`, right
/// after the collision-protect abort gate and before `pkg_preinst` runs,
/// long before a single file is copied to `${ROOT}`): wipe a stale
/// `MERGING_IDENTIFIER`-prefixed temporary sibling left by a killed
/// previous merge of this exact `category/pf`, then (re)create it empty.
/// Both merge paths call this before `pkg_preinst`, so a SIGKILL any time
/// from here on leaves `-MERGING-<pf>` behind exactly like real (the
/// `l32` C4 killed-mid-merge invariant) -- and every portuale vdb reader
/// skips such names (see [`portage_util::is_merging_vdb_entry`]).
///
/// feat#157 S1.4: the entry is written through the root's
/// [`portage_vdb::WriteTxn`] (`begin_entry`; on `files` the same
/// `stat` / `remove_dir_all` / `create_dir_all` as before). Each of the
/// merge's VDB steps opens and commits its own transaction: the pending
/// entry outlives a transaction (crate doc item 3), and on `files` every
/// call is applied at once, so the write order is the call order below.
pub(crate) fn create_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.begin_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own info-file + `COUNTER` step (`vartree.py`,
/// after `pkg_preinst`, before `_merge_contents` copies a single file to
/// `${ROOT}`): copy *every* regular file directly under `build_info_dir`
/// (`${PORTAGE_BUILDDIR}/build-info`) into the temporary vdb entry real's
/// `for x in os.listdir(inforoot): self.copyfile(...)` copies -- the
/// `CATEGORY`/`SLOT`/`KEYWORDS`/`IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/…
/// `bin/phase-functions.sh __dyn_install` writes, the
/// `DEPEND`/`RDEPEND`/`LICENSE`/… `ebuild_phases::write_post_install_
/// metadata` adds, `NEEDED.ELF.2` from the real `scanelf` QA step,
/// `environment.bz2`, the `<PF>.ebuild` copy -- then the merge-generated
/// files that were never in `build-info`: `CATEGORY`/`SLOT`/`repository`
/// re-asserted explicitly (a standalone `ebuild <file> install` outside a
/// repo checkout has no `build-info/repository`, and `SLOT` here is the
/// caller-resolved full slot) and `COUNTER` (real `cpv_counter`,
/// portuale's own `next_counter`, ticked here so the replace loop below
/// records the new value from the temporary entry). The directory itself
/// must already exist (see [`create_vdb_tmp`]).
///
/// The counter is `WriteTxn::next_counter` (on `files` real's
/// `counter_tick_core` since #306: under the VDB lock, the max of the
/// `counter` file and every installed package's own `COUNTER`, plus one,
/// written atomically).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn populate_vdb_tmp(
    root: &Path,
    category: &str,
    pf: &str,
    build_info_dir: &Path,
    slot: &str,
    repository: &str,
) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    if let Ok(entries) = portage_util::read_dir_entries(build_info_dir) {
        for entry in entries {
            let src = entry.path();
            if src.is_file()
                && let Some(name) = src.file_name()
            {
                // Entry file names are UTF-8 in the interface; every
                // build-info name is a fixed ASCII key or `<PF>.ebuild`.
                let name = name
                    .to_str()
                    .ok_or_else(|| format!("{}: file name is not UTF-8", src.display()))?;
                txn.copy_entry_file(&key, name, &src)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    let counter = txn.next_counter().map_err(|e| e.to_string())?;
    for (name, value) in [
        ("CATEGORY", category),
        ("SLOT", slot),
        ("repository", repository),
    ] {
        txn.put_entry_file(&key, name, format!("{value}\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }
    txn.put_entry_file(&key, "COUNTER", counter.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own `CONTENTS` + metadata-consolidation tail
/// (`vartree.py`: `CONTENTS` is written into `dbtmpdir` by
/// `_merge_contents` while the image is copied to `${ROOT}`, and
/// `_consolidate_to_metadata_file(self.dbtmpdir)` is the last write into
/// `dbtmpdir` before the rename below): record the merge-generated file
/// list, then fold every per-field file into the consolidated `metadata`
/// file. Runs after the file copy, before the old instances are unmerged
/// (neither touches the temporary directory, so real's
/// consolidate-after-unmerge and this consolidate-before-unmerge are the
/// same bytes); the rename into place still waits for the replace loop
/// (see [`publish_vdb_tmp`]).
pub(crate) fn write_vdb_tmp_contents(
    root: &Path,
    category: &str,
    pf: &str,
    contents: &str,
) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    // Real `_consolidate_to_metadata_file(self.dbtmpdir)` (`vartree.py`):
    // the last write into the temp vdb dir before the rename -- fold
    // every per-field file real's `_in_metadata_file()` accepts into one
    // `metadata` file. Its `#dir_mtime=` trailer records the dir's
    // `st_mtime_ns` and real's reader rejects the file if the dir changed
    // afterwards, so it must come after CONTENTS above and the
    // `#dir_mtime=` line must be *appended* (a plain write, no new dir
    // entry) after the body is on disk. The rename below does not touch
    // the dir's own mtime, so the value survives the move.
    // `seal_entry` is that consolidation (moved into `portage-vdb`,
    // S1.4): body, `stat`, `#dir_mtime=` appended last.
    txn.put_entry_file(&key, "CONTENTS", contents.as_bytes())
        .map_err(|e| e.to_string())?;
    txn.seal_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Real `dblink.treewalk()`'s own move into place (`vartree.py`: after
/// the replace loop unmerged every same-slot version,
/// `self.dbdir = self.dbpkgdir; self.delete(); _movefile(self.dbtmpdir,
/// self.dbpkgdir)`): drop the old live entry, then atomically rename the
/// temporary sibling into place. Both sit under the same `<category>`
/// directory, hence guaranteed the same filesystem, so `std::fs::rename`
/// alone is already atomic here -- the same guarantee real `_movefile()`
/// relies on for a same-device move. A crash before this leaves the stale
/// `MERGING_IDENTIFIER` leftover (readers skip it; the next merge's
/// [`create_vdb_tmp`] wipes it) -- never a half-written *final* entry,
/// except real's own delete-then-move exposure on a same-pf reinstall,
/// which this shares exactly.
///
/// `WriteTxn::finish_entry` (S1.4; on `files` the same `stat`,
/// `remove_dir_all` and `rename` as before).
pub(crate) fn publish_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.finish_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// The merge's publish step: the new entry goes live, the replaced
/// same-slot entries go away, and the merge's preserved-libs registration
/// (real `vartree.py:5266-5272`) is stored.
///
/// - `files` ([`portage_vdb::InstalledDb::replace_in_publish`] is
///   `false`): exactly the S1.4 calls, [`publish_vdb_tmp`] then
///   [`register_merge_preserved_libs`]. The replace loop already deleted
///   each entry of `replaced` (`unmerge_one_installed`), so it is unused.
/// - database backends: **one transaction** (design §9, feat#157 S4.1):
///   [`portage_vdb::WriteTxn::finish_entry_replacing`] deletes the rows of
///   `replaced` and publishes the pending row (which also raises the
///   counter high-water mark to its `COUNTER`), and the registry update
///   rides along through `set_preserved_libs`. The counter it records is
///   read from the pending entry, the bytes `populate_vdb_tmp` stored, the
///   same value the published entry carries. Until this commit every
///   reader (`has_version` included) still sees the old instances; a
///   crash before it leaves them installed and the new row `merging`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn publish_merged_entry(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    main_slot: &str,
    preserve_paths: &BTreeSet<String>,
    replaced: &[String],
) -> Result<(), String> {
    let db = portage_vdb::for_root(root);
    if !db.replace_in_publish() {
        publish_vdb_tmp(root, category, pf)?;
        return register_merge_preserved_libs(root, category, pn, pf, main_slot, preserve_paths);
    }
    let key = portage_vdb::EntryKey::new(category, pf);
    let libs = if preserve_paths.is_empty() {
        None
    } else {
        let counter = db
            .read_pending_file(&key, "COUNTER")
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| "0".to_string());
        plib_store(&merge_plib_registration(
            root,
            category,
            pn,
            pf,
            main_slot,
            preserve_paths,
            &counter,
        ))
    };
    let old: Vec<portage_vdb::EntryKey> = replaced
        .iter()
        .map(|old_pf| portage_vdb::EntryKey::new(category, old_pf.as_str()))
        .collect();
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.finish_entry_replacing(&key, &old)
        .map_err(|e| e.to_string())?;
    if let Some(libs) = &libs {
        txn.set_preserved_libs(libs).map_err(|e| e.to_string())?;
    }
    #[cfg(test)]
    tests::publish_hook(root);
    txn.commit().map_err(|e| e.to_string())
}

/// Where `pkg_postinst`'s `PORTAGE_UPDATE_ENV` points (real
/// `vartree.py:5334-5337`): the live entry's own `environment.bz2`.
/// `files`: `<entry>/environment.bz2`, rewritten in place by bash, and
/// `scratch` is `None`. A database backend has no entry directory, so the
/// stored file is copied into `scratch_dir` and the phase rewrites that
/// copy; [`absorb_update_env`] stores it back (feat#157 S4.1, N9).
pub(crate) struct UpdateEnvTarget {
    pub(crate) path: PathBuf,
    pub(crate) scratch: Option<PathBuf>,
}

pub(crate) fn update_env_target(
    root: &Path,
    category: &str,
    pf: &str,
    scratch_dir: &Path,
) -> Result<UpdateEnvTarget, String> {
    let db = portage_vdb::for_root(root);
    let key = portage_vdb::EntryKey::new(category, pf);
    if let Some(entry) = db.entry_path(&key) {
        return Ok(UpdateEnvTarget {
            path: entry.join("environment.bz2"),
            scratch: None,
        });
    }
    portage_vdb::materialize_files(db.as_ref(), &key, &["environment.bz2"], scratch_dir)
        .map_err(|e| e.to_string())?;
    Ok(UpdateEnvTarget {
        path: scratch_dir.join("environment.bz2"),
        scratch: Some(scratch_dir.to_path_buf()),
    })
}

/// After `pkg_postinst`: on a database backend, store the rewritten
/// scratch `environment.bz2` into the live entry (one `replace_file`
/// commit, only when the bytes changed). Real rewrites the file after the
/// entry is published too, so this is a separate commit there as well.
/// No-op on `files`.
pub(crate) fn absorb_update_env(
    root: &Path,
    category: &str,
    pf: &str,
    target: &UpdateEnvTarget,
) -> Result<(), String> {
    let Some(dir) = &target.scratch else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    portage_vdb::absorb_file(
        db.as_ref(),
        &portage_vdb::EntryKey::new(category, pf),
        dir,
        "environment.bz2",
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
