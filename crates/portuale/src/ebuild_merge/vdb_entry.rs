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
