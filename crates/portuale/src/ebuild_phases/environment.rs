//! The build environment of one package: paths, directories, fake DISTDIR.

use crate::ebuild_phases::*;

/// The real directory layout `doebuild_environment()` computes (all
/// paths, `PORTAGE_BUILDDIR`-relative, matching real
/// `lib/portage/package/ebuild/doebuild.py:499-524` exactly): `D`/`ED`
/// get a trailing separator, matching real portage's own convention
/// (every real helper script's own path-joining assumes it).
pub(crate) struct Environment {
    pub(crate) ebuild_abs: PathBuf,
    pub(crate) pkg_dir: PathBuf,
    pub(crate) category: String,
    pub(crate) split: PackageSplit,
    pub(crate) eapi: String,
    pub(crate) portage_tmpdir: PathBuf,
    pub(crate) portage_builddir: PathBuf,
    /// Space-joined eclass names for `INHERITED` (real
    /// `porttree.py:872`'s `" ".join(mydata["_eclasses_"])`), from the
    /// ebuild's own repo `metadata/md5-cache` entry. `None` for a
    /// standalone ebuild outside any repo, or one that inherits nothing.
    /// Exported into every phase env so `bin/ebuild.sh`'s own
    /// `__INHERITED_QA_CACHE=${INHERITED}` snapshot suppresses the
    /// spurious `Eclass '…' inherited illegally` QA notice when a
    /// non-`depend` phase re-sources the ebuild -- see `phase_env_vars`.
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
    // Real doebuild_environment(): `cat = os.path.basename(os.path.
    // dirname(pkg_dir))` -- the category is the ebuild's own
    // grandparent directory name (<category>/<package>/<pf>.ebuild).
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

    // Real `porttree.py:872`: `INHERITED = " ".join(_eclasses_)` -- the
    // eclass names from this ebuild's own repo `metadata/md5-cache`
    // entry, in order. Modern md5-cache stores `_eclasses_=<name>\t<md5>
    // \t<name>\t<md5>…`; older/fixture caches store a plain `INHERITED=
    // <space list>`. Absent for a standalone ebuild outside any repo.
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

    // Real `prepare_build_dirs._prepare_fake_filesdir`
    // (`lib/portage/package/ebuild/prepare_build_dirs.py:504-515`):
    // `${PORTAGE_BUILDDIR}/files` is always a symlink to the ebuild's own
    // `O/files` (the repo package dir's `files/`), even when that target
    // does not exist -- real links unconditionally (a dangling link is
    // fine: `FILESDIR` consumers fail loudly on a missing path either
    // way, and crucially the `clean` phase's `rm -f .../files` only
    // works on a link). The phase env's `FILESDIR` is
    // `Environment::filesdir()` = builddir/files (matching real
    // `doebuild.py:527`), so without this link every `eapply`/
    // `FILESDIR` reference in a real ebuild (app-misc/jq's
    // `jq-1.6-r3-never-bundle-oniguruma.patch` is what surfaced it) dies
    // on a missing path -- L2 S5 finding `l2-filesdir-symlink-missing`.
    // A binary merge's ebuild is copied *into* the builddir (pkg_dir ==
    // portage_builddir), where the link would be a self-loop.
    //
    // Backlog #94 (host `emerge -1 sys-libs/timezone-data` residue):
    // portuale used to link only when the repo `files/` was a real dir
    // and otherwise let `create_directories` make a plain directory --
    // whose `rm -f` in `__dyn_clean` then fails with "Is a directory"
    // and, under the phase's `set -e`, aborts the rest of the cleanup,
    // leaving `.../files` (+ parents) behind in `/var/tmp/portage`.
    // Link unconditionally now; `create_directories` skips the symlink
    // (it must not `create_dir_all` through a dangling link).
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
    /// Real `${PORTAGE_BUILDDIR}` (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`)
    /// -- a per-merge scratch root the caller can hang its own temp
    /// subdirectories off (e.g. `ebuild_merge`'s replace-loop
    /// extracted-from-vdb ebuilds).
    pub(crate) fn portage_builddir(&self) -> &Path {
        &self.portage_builddir
    }
    /// Real `${PORTAGE_TMPDIR}` -- the root `bin/ebuild.sh`'s own
    /// `addwrite "${PORTAGE_TMPDIR}/portage"` opens to the sandbox, so
    /// it must be a real env var whenever `FEATURES=sandbox` is active.
    pub(crate) fn portage_tmpdir(&self) -> &Path {
        &self.portage_tmpdir
    }
    pub(crate) fn d(&self) -> PathBuf {
        self.portage_builddir.join("image")
    }
    pub(crate) fn workdir(&self) -> PathBuf {
        self.portage_builddir.join("work")
    }
    /// Real `${T}` -- also read by `ebuild_merge::process_merge_elog`
    /// (the per-package `elog` flush before the post-merge clean, #42).
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
    /// Real `${PORTAGE_BUILDDIR}/distdir` (`config.py:3408`): the
    /// per-package fake `DISTDIR`.
    pub(crate) fn fake_distdir(&self) -> PathBuf {
        self.portage_builddir.join("distdir")
    }
    /// Real `${PORTAGE_BUILDDIR}/build-info`: created as a side effect of
    /// every real `unpack|prepare|configure|compile|test|clean|install`
    /// phase already run by the time `ebuild_package::run_package`'s own
    /// `install` chain completes (`bin/phase-functions.sh`'s own
    /// unconditional `mkdir build-info` in that case branch) -- so by
    /// the time packaging needs it, it already exists and already has a
    /// real copy of the ebuild file in it (`build-info/${PF}.ebuild`).
    pub(crate) fn build_info(&self) -> PathBuf {
        self.portage_builddir.join("build-info")
    }
    /// Real `${PORTAGE_BUILDDIR}/.installed`: real, unmodified
    /// `bin/phase-functions.sh`'s own `__dyn_install` already creates
    /// this unconditionally on a successful `src_install`
    /// (`phase-functions.sh:653`, no `FEATURES` gate at all) -- this
    /// portuale writes nothing new for it, real phase execution already
    /// leaves it behind as a side effect (confirmed empirically: a real
    /// `ebuild <file> install` run via portuale's own binary leaves
    /// `.installed` in place). `ebuild_merge::run_qmerge` is the one
    /// caller: real `doebuild()`'s own `mydo == "qmerge"` branch checks
    /// for exactly this marker before skipping the install phase.
    pub(crate) fn installed_marker(&self) -> PathBuf {
        self.portage_builddir.join(".installed")
    }
}

/// Real `bin/*.sh` needs every directory it writes into (via helpers, or
/// bare bash redirection) to already exist -- `__dyn_unpack` creates
/// `WORKDIR` itself, but `T`/`D`/`HOME` are relied on existing already
/// (real `doebuild()` creates them via `prepare_build_dirs()` before
/// spawning the phase at all).
///
/// `S` is deliberately NOT created here (backlog #190): real `S` only
/// ever exists because `src_unpack`'s own `unpack ${A}` call creates it
/// as a side effect of extracting a real, fetched `SRC_URI` archive, or
/// because the ebuild sets `S` itself (e.g. `S="${WORKDIR}"`). Real
/// `bin/phase-functions.sh`'s `__dyn_prepare`/`__dyn_configure`/
/// `__dyn_compile`/`__dyn_test`/`__dyn_install` die with `The source
/// directory '${S}' doesn't exist` when `S` is not a directory
/// (`phase-functions.sh:444,480,512,546,638`; default
/// `S=${WORKDIR}/${P}` from `bin/ebuild.sh:500`; the `${WORKDIR}`
/// fallback is EAPI 0-3 only, `bin/eapi.sh:8`; the empty-`A` fallback
/// fires only when no phase up to the current one is defined).
/// Pre-creating `${S}` would make that `[[ -d ${S} ]]` check always
/// pass, so a source-less ebuild with no `S` would build under
/// portuale while real dies -- exactly the #190 divergence. Real
/// `_prepare_fake_filesdir` (`prepare_build_dirs.py:504-515`) as a
/// standalone step: `link_path` (`${PORTAGE_BUILDDIR}/files`) becomes a
/// symlink to `target` (the repo package dir's `files/`), unconditionally
/// -- dangling included, exactly like real.
///
/// Removal safety (the link target may live anywhere, including the repo
/// itself): `symlink_metadata` never follows the link, so only the link
/// entry (or a plain file) is ever `remove_file`d; a real directory --
/// the pre-fix leftover shape, or anything a phase dropped -- is removed
/// with `remove_dir_all`, but strictly bounded to this one known
/// inside-the-builddir path (never a user path, never followed). The
/// parent builddir is `create_dir_all`ed first so a first-ever build has
/// somewhere to put the link.
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

/// Real `prepare_build_dirs._prepare_fake_distdir`
/// (`lib/portage/package/ebuild/prepare_build_dirs.py:518-549`):
/// `${PORTAGE_BUILDDIR}/distdir` holds exactly one symlink per file of the
/// package's own `A`, each pointing at `<actual>/<file>` -- so a phase
/// sees only the distfiles it declared. Anything else in it (a stale
/// symlink, a stray file, a directory) is removed; a symlink with the wrong
/// target is recreated. Real also chowns the directory to the portage gid
/// when privileged; portuale does not model that for any builddir
/// directory (see `create_directories`), so it is left out here too.
pub(crate) fn prepare_fake_distdir(
    env: &Environment,
    actual: &Path,
    alist: &[String],
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fake = env.fake_distdir();
    let io = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    if !std::fs::symlink_metadata(&fake).is_ok_and(|m| m.is_dir()) {
        // A non-directory squatter (real's `ensure_dirs` would fail on it).
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
    for dir in [
        env.t(),
        env.d(),
        env.home(),
        env.filesdir(),
        // Real `bin/ebuild.sh`'s own top-level code (run unconditionally
        // as soon as it's sourced, EAPI 8's own comment: "requires us to
        // use an empty directory here"): `cd`s into `${PORTAGE_BUILDDIR}/
        // empty` if it exists, falling back to `${PORTAGE_PYM_PATH}`
        // (always set in portuale, see `pym_path_value`) otherwise,
        // `die`-ing if neither works. Always pre-created here
        // so that fallback path is never reached at all.
        env.portage_builddir.join("empty"),
        // Real `prepare_build_dirs` creates `${T}/logging`;
        // `bin/isolated-functions.sh::__elog_base` silently drops every
        // `elog`/`ewarn`/`eerror` message if the dir doesn't exist.
        env.t().join("logging"),
    ] {
        // `${PORTAGE_BUILDDIR}/files` is a symlink (possibly dangling --
        // see `ensure_fake_filesdir_link`), never a directory to create:
        // `create_dir_all` would follow a dangling link and fail (or
        // worse, materialise the target). `symlink_metadata` does not
        // follow. Anything else in this list is always a real dir.
        if dir == env.filesdir()
            && std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}
