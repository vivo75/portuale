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

pub(crate) fn compute_environment(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
) -> Result<Environment, String> {
    let ebuild_abs = ebuild_path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", ebuild_path.display()))?;
    let pkg_dir = ebuild_abs
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", ebuild_abs.display()))?
        .to_path_buf();
    // The category is the grandparent dir name (<category>/<package>/<pf>.ebuild).
    // Portage: doebuild.py, doebuild_environment
    let category = pkg_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            format!(
                "{}: cannot determine CATEGORY from path",
                ebuild_abs.display()
            )
        })?
        .to_string();

    let package_dir_name = pkg_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: cannot determine package name", pkg_dir.display()))?;

    let ebuild_text = std::fs::read_to_string(&ebuild_abs)
        .map_err(|e| format!("{}: {e}", ebuild_abs.display()))?;
    let eapi = parse_eapi(&ebuild_text);
    let split = split_package(&ebuild_abs, package_dir_name)?;

    let portage_builddir = portage_tmpdir
        .join("portage")
        .join(&category)
        .join(&split.pf);

    // `INHERITED` is the md5-cache `_eclasses_` names in order (stored tab-separated
    // with md5s); older caches carry a plain space list. Absent outside a repo.
    let inherited = repo_root_for(&pkg_dir)
        .and_then(|repo_root| {
            portage_repo::repo_aux_metadata(&repo_root, &category, &split.pf).ok()
        })
        .and_then(|md| {
            if let Some(eclasses) = md.get("_eclasses_") {
                let names: Vec<&str> = eclasses.split('\t').step_by(2).collect();
                (!names.is_empty()).then(|| names.join(" "))
            } else {
                md.get("INHERITED")
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|s| !s.is_empty())
            }
        });

    // `${PORTAGE_BUILDDIR}/files` is always a symlink to the repo `files/` dir,
    // even dangling: consumers fail loudly either way, and `clean`'s `rm -f`
    // needs a link, not a directory. Skipped for a binary merge (self-loop).
    // Portage: prepare_build_dirs.py, _prepare_fake_filesdir
    let real_filesdir = pkg_dir.join("files");
    if pkg_dir != portage_builddir {
        ensure_fake_filesdir_link(&real_filesdir, &portage_builddir.join("files"));
    }

    Ok(Environment {
        ebuild_abs,
        pkg_dir,
        category,
        split,
        eapi,
        portage_tmpdir: portage_tmpdir.to_path_buf(),
        portage_builddir,
        inherited,
    })
}

impl Environment {
    /// `${PORTAGE_BUILDDIR}` (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`): callers hang
    /// their own temp subdirectories off it.
    pub(crate) fn portage_builddir(&self) -> &Path {
        &self.portage_builddir
    }
    /// `${PORTAGE_TMPDIR}`: must be an exported var when `FEATURES=sandbox` is on,
    /// for `bin/ebuild.sh`'s `addwrite`.
    pub(crate) fn portage_tmpdir(&self) -> &Path {
        &self.portage_tmpdir
    }
    pub(crate) fn d(&self) -> PathBuf {
        self.portage_builddir.join("image")
    }
    pub(crate) fn workdir(&self) -> PathBuf {
        self.portage_builddir.join("work")
    }
    /// `${T}`: also read by the per-package `elog` flush before the post-merge clean.
    pub(crate) fn t(&self) -> PathBuf {
        self.portage_builddir.join("temp")
    }
    pub(crate) fn s(&self) -> PathBuf {
        self.workdir().join(&self.split.p)
    }
    pub(crate) fn home(&self) -> PathBuf {
        self.portage_builddir.join("homedir")
    }
    pub(crate) fn filesdir(&self) -> PathBuf {
        self.portage_builddir.join("files")
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

pub(crate) fn create_directories(env: &Environment) -> Result<(), String> {
    // `S` is deliberately not created: only `unpack` extracting an archive or the
    // ebuild itself may bring it to life, so a source-less ebuild still fails the `[[ -d ${S} ]]` check.
    for dir in [
        env.t(),
        env.d(),
        env.home(),
        env.filesdir(),
        // `bin/ebuild.sh` sources into `${PORTAGE_BUILDDIR}/empty` when it exists
        // (its "safe cwd" branch); always pre-created so the fallback never triggers.
        env.portage_builddir.join("empty"),
        // `${T}/logging`: without it `__elog_base` silently drops all elog output.
        env.t().join("logging"),
    ] {
        // `files` is a (possibly dangling) symlink, never a dir: don't follow it.
        if dir == env.filesdir()
            && std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}
