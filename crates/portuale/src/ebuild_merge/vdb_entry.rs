//! Writing and publishing the VDB entry; env.d updates.

use crate::ebuild_merge::*;

/// Where `pkg_postinst`'s `PORTAGE_UPDATE_ENV` points: the live entry's own
/// `environment.bz2` (Portage: vartree.py, dblink.treewalk). `files`:
/// rewritten in place, `scratch` is `None`; a database backend copies the
/// stored file into `scratch_dir` for the phase to rewrite, and
/// [`absorb_update_env`] stores it back.
pub(crate) struct UpdateEnvTarget {
    pub(crate) path: PathBuf,
    pub(crate) scratch: Option<PathBuf>,
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
