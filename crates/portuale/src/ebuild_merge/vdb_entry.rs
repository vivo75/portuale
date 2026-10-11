//! Writing and publishing the VDB entry; env.d updates.

use crate::ebuild_merge::*;

/// Wipes a stale `-MERGING-<pf>` sibling left by a killed previous merge,
/// then recreates it empty (Portage: vartree.py, dblink.treewalk). Runs
/// before `pkg_preinst`; every portuale vdb reader skips such names.
/// Through the root's [`portage_vdb::WriteTxn`]; each VDB step below opens
/// and commits its own transaction.
pub(crate) fn create_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.begin_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Arguments of [`populate_vdb_tmp`].
pub(crate) struct PopulateVdbTmpArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) build_info_dir: &'a Path,
    pub(crate) slot: &'a str,
    pub(crate) repository: &'a str,
}

/// Copies every regular file directly under `build_info_dir` into the
/// temporary vdb entry, then re-asserts `CATEGORY`/`SLOT`/`repository` and
/// ticks `COUNTER` via `WriteTxn::next_counter` (Portage: vartree.py,
/// dblink.treewalk). The directory must already exist (see [`create_vdb_tmp`]).
pub(crate) fn populate_vdb_tmp(args: PopulateVdbTmpArgs<'_>) -> Result<(), String> {
    let PopulateVdbTmpArgs {
        root,
        category,
        pf,
        build_info_dir,
        slot,
        repository,
    } = args;
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

/// Records `CONTENTS` in the temporary entry, then folds every per-field
/// file into the consolidated `metadata` file (`seal_entry`; Portage:
/// vartree.py, dblink.treewalk). Runs after the file copy; the rename into
/// place waits for the replace loop (see [`publish_vdb_tmp`]).
pub(crate) fn write_vdb_tmp_contents(
    root: &Path,
    category: &str,
    pf: &str,
    contents: &str,
) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    // The consolidation: body, `stat`, `#dir_mtime=` appended last. The
    // rename below does not touch the dir's own mtime, so the value
    // survives the move.
    txn.put_entry_file(&key, "CONTENTS", contents.as_bytes())
        .map_err(|e| e.to_string())?;
    txn.seal_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Drops the old live entry, then atomically renames the temporary sibling
/// into place (same directory, so `rename` alone is atomic; Portage:
/// vartree.py, dblink.treewalk). A crash before this leaves the stale
/// `MERGING_IDENTIFIER` leftover, never a half-written final entry.
pub(crate) fn publish_vdb_tmp(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.finish_entry(&key).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// Arguments of [`publish_merged_entry`].
pub(crate) struct PublishMergedEntryArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) preserve_paths: &'a BTreeSet<String>,
    pub(crate) replaced: &'a [String],
}

/// The merge's publish step: the new entry goes live, the replaced
/// same-slot entries go away, and the preserved-libs registration is stored
/// (Portage: vartree.py, dblink.treewalk).
///
/// - `files`: [`publish_vdb_tmp`] then [`register_merge_preserved_libs`].
/// - database backends: one transaction deleting `replaced`'s rows and
///   publishing the pending row (raising the counter high-water mark to its
///   `COUNTER`), with the registry update riding along. Until this commit
///   readers still see the old instances; a crash before it leaves them
///   installed and the new row `merging`.
pub(crate) fn publish_merged_entry(args: PublishMergedEntryArgs<'_>) -> Result<(), String> {
    let PublishMergedEntryArgs {
        root,
        category,
        pn,
        pf,
        main_slot,
        preserve_paths,
        replaced,
    } = args;
    let db = portage_vdb::for_root(root);
    if !db.replace_in_publish() {
        publish_vdb_tmp(root, category, pf)?;
        return register_merge_preserved_libs(RegisterMergePreservedLibsArgs {
            root,
            category,
            pn,
            new_pf: pf,
            main_slot,
            preserve_paths,
        });
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
        plib_store(&merge_plib_registration(MergePlibRegistrationArgs {
            root,
            category,
            pn,
            new_pf: pf,
            main_slot,
            preserve_paths,
            new_counter: &counter,
        }))
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

/// Where `pkg_postinst`'s `PORTAGE_UPDATE_ENV` points: the live entry's own
/// `environment.bz2` (Portage: vartree.py, dblink.treewalk). `files`:
/// rewritten in place, `scratch` is `None`; a database backend copies the
/// stored file into `scratch_dir` for the phase to rewrite, and
/// [`absorb_update_env`] stores it back.
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
/// commit, only when the bytes changed). No-op on `files`.
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
