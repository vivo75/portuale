//! The build environment of one package: paths, directories, fake DISTDIR.

use crate::ebuild_phases::*;

/// The directory layout `doebuild_environment()` computes, `PORTAGE_BUILDDIR`-relative.
/// Portage: doebuild.py, doebuild_environment
pub(crate) struct Environment {
    pub(crate) ebuild_abs: PathBuf,
    pub(crate) pkg_dir: PathBuf,
    pub(crate) category: String,
    pub(crate) split: PackageSplit,
    pub(crate) eapi: String,
    pub(crate) portage_tmpdir: PathBuf,
    pub(crate) portage_builddir: PathBuf,
    /// Space-joined eclass names for `INHERITED`, from the ebuild's md5-cache entry
    /// (`_eclasses_`, else the older `INHERITED` list); unset outside a repo.
    /// Exported into every phase env (see `phase_env_vars`).
    pub(crate) inherited: Option<String>,
}

impl Environment {
    /// `${PORTAGE_TMPDIR}`: must be an exported var when `FEATURES=sandbox` is on,
    /// for `bin/ebuild.sh`'s `addwrite`.
    pub(crate) fn portage_tmpdir(&self) -> &Path {
        &self.portage_tmpdir
    }
    /// `${PORTAGE_BUILDDIR}/distdir`: the per-package fake `DISTDIR`.
    /// Portage: config.py
    pub(crate) fn fake_distdir(&self) -> PathBuf {
        self.portage_builddir.join("distdir")
    }
    /// `${PORTAGE_BUILDDIR}/build-info`: pre-exists by packaging time — Portage's
    /// `__dyn_install` already created it and copied the ebuild file in.
    pub(crate) fn build_info(&self) -> PathBuf {
        self.portage_builddir.join("build-info")
    }
    /// `${PORTAGE_BUILDDIR}/.installed`: Portage's `__dyn_install` creates it on a
    /// successful `src_install`; `run_qmerge` skips the install phase when present.
    /// Portage: phase-functions.sh, doebuild.py
    pub(crate) fn installed_marker(&self) -> PathBuf {
        self.portage_builddir.join(".installed")
    }
}

/// Links `${PORTAGE_BUILDDIR}/files` to the repo package dir's `files/`,
/// unconditionally — dangling included, like Portage.
///
/// Removal safety (the target may live anywhere, including the repo itself):
/// only the link entry (or a stray file) is ever `remove_file`d; a plain
/// directory is removed with `remove_dir_all`, strictly bounded to this one
/// known in-builddir path (never followed, never a user path). The parent
/// builddir is created first so a first-ever build has somewhere for the link.
/// Portage: prepare_build_dirs.py, _prepare_fake_filesdir
pub(crate) fn ensure_fake_filesdir_link(target: &Path, link_path: &Path) {
    if link_path
        .parent()
        .is_some_and(|parent| std::fs::create_dir_all(parent).is_err())
    {
        return;
    }
    match std::fs::read_link(link_path) {
        Ok(current) if current == target => {}
        _ => {
            match std::fs::symlink_metadata(link_path) {
                Ok(meta) if meta.file_type().is_symlink() || meta.file_type().is_file() => {
                    let _ = std::fs::remove_file(link_path);
                }
                Ok(_) => {
                    let _ = std::fs::remove_dir_all(link_path);
                }
                Err(_) => {}
            }
            let _ = std::os::unix::fs::symlink(target, link_path);
        }
    }
}

/// `${PORTAGE_BUILDDIR}/distdir` holds one symlink per file of the package's `A`,
/// each pointing at the actual dir, so a phase sees only its declared distfiles.
/// Anything else in it is removed; a wrong-target symlink is recreated. The
/// portage-gid chown is left out (like every builddir directory).
/// Portage: prepare_build_dirs.py, _prepare_fake_distdir
pub(crate) fn prepare_fake_distdir(
    env: &Environment,
    actual: &Path,
    alist: &[String],
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fake = env.fake_distdir();
    let io = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    if !std::fs::symlink_metadata(&fake).is_ok_and(|m| m.is_dir()) {
        // A non-directory squatter must go before the dir is (re)created.
        if std::fs::symlink_metadata(&fake).is_ok() {
            std::fs::remove_file(&fake).map_err(|e| io(&fake, e))?;
        }
        std::fs::create_dir_all(&fake).map_err(|e| io(&fake, e))?;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| io(&fake, e))?;
    }
    for entry in std::fs::read_dir(&fake).map_err(|e| io(&fake, e))? {
        let entry = entry.map_err(|e| io(&fake, e))?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        let keep = meta.file_type().is_symlink()
            && alist
                .iter()
                .any(|x| std::ffi::OsStr::new(x) == entry.file_name());
        if keep {
            continue;
        }
        if meta.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| io(&path, e))?;
        } else {
            std::fs::remove_file(&path).map_err(|e| io(&path, e))?;
        }
    }
    for x in alist {
        let link = fake.join(x);
        let target = actual.join(x);
        match std::fs::read_link(&link) {
            Ok(current) if current == target => continue,
            Ok(_) => std::fs::remove_file(&link).map_err(|e| io(&link, e))?,
            Err(_) => {}
        }
        std::os::unix::fs::symlink(&target, &link).map_err(|e| io(&link, e))?;
    }
    Ok(())
}
