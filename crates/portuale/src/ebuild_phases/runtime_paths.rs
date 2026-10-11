//! Where the phase runtime (bin/, pym/) and the repository live; the embedded runtime.

use crate::ebuild_phases::*;

/// The tree this binary was compiled in (two levels above the manifest dir),
/// canonicalized when it still exists. Never panics: a relocated binary degrades
/// like a missing checkout; the consumer that cannot degrade reports via `bin_dir`.
pub(crate) fn repo_root() -> PathBuf {
    // portuale/src/ebuild_phases.rs -> portuale -> rust -> repo root
    let lexical = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or(Path::new("/"));
    lexical
        .canonicalize()
        .unwrap_or_else(|_| lexical.to_path_buf())
}

/// The working checkout of upstream Portage (`3rdparty/portage/`, overridable via
/// `$PORTUALE_PORTAGE_CHECKOUT`). Runtime needs it only for the oracle overlay,
/// `package_sets_conf`'s first choice, and the re-sync source; absent when nobody
/// cloned it. See `3rdparty/repos.toml` for the pinned ref.
pub(crate) fn portage_checkout() -> PathBuf {
    if let Some(p) = std::env::var_os("PORTUALE_PORTAGE_CHECKOUT") {
        return PathBuf::from(p);
    }
    repo_root().join("3rdparty/portage")
}

/// `<checkout>/lib` when it holds `portage`, else `/`. Never empty: shells `cd`
/// into the value unconditionally, and `/` is always a valid cwd.
/// Portage: const.py, config.py, phase-functions.sh, bin/ebuild.sh
pub(crate) fn pym_path_value(checkout: &Path) -> String {
    let lib = checkout.join("lib");
    if lib.join("portage").is_dir() {
        lib.display().to_string()
    } else {
        "/".to_string()
    }
}

/// `Some("<checkout>/lib")` under the same condition, else nothing (no entry is
/// pushed; native helpers need none). Portage prefers it over `PORTAGE_PYM_PATH`.
/// Portage: misc-functions.sh
pub(crate) fn pythonpath_value(checkout: &Path) -> Option<String> {
    let lib = checkout.join("lib");
    if lib.join("portage").is_dir() {
        Some(lib.display().to_string())
    } else {
        None
    }
}

/// Portage's `cnf/sets/portage.conf`: the checkout's file when present, else the
/// compiled-in copy, so a relocated binary lists the same sets instead of none.
pub(crate) fn package_sets_conf() -> Option<String> {
    std::fs::read_to_string(portage_checkout().join("cnf/sets/portage.conf"))
        .ok()
        .or_else(|| {
            crate::embedded_runtime::file("cnf/sets/portage.conf")
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        })
}

/// The dir `PORTAGE_BIN_PATH` points at: the tracked vendored `bin/` (unmodified
/// upstream scripts plus portuale-owned shims), or — in oracle mode only — a symlink
/// overlay with vendored entries winning over `<checkout>/bin/`. `ebuild.sh` uses it
/// as a literal string prefix, never `realpath`s it. Resolved once per process.
/// See `bin/README.md` for the portuale-owned list (a re-sync must not overwrite them).
pub(crate) fn bin_dir() -> Result<&'static Path, String> {
    use std::sync::OnceLock;
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let vendored = match resolve_bin_dir(
            std::env::var(BIN_DIR_VAR).ok().as_deref(),
            &repo_root(),
            true,
        )? {
            BinSource::Dir(dir) => dir,
            BinSource::Embedded => extract_embedded_runtime()?,
        };
        let checkout = portage_checkout().join("bin");
        // The checkout overlay exists only in oracle mode (the differential-tests
        // handle); native mode (unset or `native`) uses the vendored dir directly.
        let real_helpers = std::env::var("PORTUALE_PYTHON_HELPERS").as_deref() == Ok("real");
        if !real_helpers || !checkout.is_dir() {
            return Ok(vendored);
        }
        let overlay = exec_scratch_base().join(format!("portuale-bin.{}", std::process::id()));
        match build_bin_overlay(&overlay, &checkout, &vendored) {
            Ok(()) => {
                register_cleanup(overlay.clone());
                Ok(overlay)
            }
            Err(e) => {
                eprintln!(
                    "portuale: bin/ overlay setup failed ({e}); \
                     falling back to {} (vendored bin/ changes not applied)",
                    checkout.display()
                );
                Ok(checkout)
            }
        }
    })
    .as_ref()
    .map(PathBuf::as_path)
    .map_err(Clone::clone)
}

/// Env var naming a dir holding the vendored runtime, for a binary run outside
/// the tree it was built in.
pub(crate) const BIN_DIR_VAR: &str = "PORTUALE_BIN_DIR";

/// Where [`resolve_bin_dir`] found the phase runtime.
#[derive(Debug, PartialEq)]
pub(crate) enum BinSource {
    /// A directory on disk.
    Dir(PathBuf),
    /// Nowhere on disk: the copy compiled into this binary.
    Embedded,
}

/// First hit wins: `$PORTUALE_BIN_DIR` when set and non-empty; the build tree's
/// `bin/` when it still has one; the embedded copy. A wrong override errors and
/// never falls through (a silent fallback would hide a typo). Messages carry no
/// program prefix; each caller prints its own.
///
/// Pure (all inputs are parameters) so the order is unit-testable.
pub(crate) fn resolve_bin_dir(
    override_dir: Option<&str>,
    build_root: &Path,
    has_embedded: bool,
) -> Result<BinSource, String> {
    if let Some(dir) = override_dir.filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        return if dir.join("ebuild.sh").is_file() {
            Ok(BinSource::Dir(dir))
        } else {
            Err(format!(
                "{BIN_DIR_VAR}={} has no ebuild.sh: it must name a copy of the \
                 repository's bin/ directory (the vendored ebuild phase runtime)",
                dir.display()
            ))
        };
    }
    let tree = build_root.join("bin");
    if tree.join("ebuild.sh").is_file() {
        return Ok(BinSource::Dir(tree));
    }
    if has_embedded {
        return Ok(BinSource::Embedded);
    }
    Err(format!(
        "cannot find the ebuild phase runtime (bin/ebuild.sh): {BIN_DIR_VAR} is not set \
         and the tree this binary was built in ({}) is not there. Run from that tree, \
         or set {BIN_DIR_VAR} to a copy of the repository's bin/ directory",
        build_root.display()
    ))
}

/// True when `dir` is on a `noexec` mount (or can't be inspected: same treatment).
pub(crate) fn is_noexec(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return true;
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return true;
    }
    st.f_flag & libc::ST_NOEXEC != 0
}

/// First of `candidates` that `usable` accepts, else the first one (the caller
/// then fails with its own, more specific message).
pub(crate) fn pick_scratch_base(candidates: &[PathBuf], usable: impl Fn(&Path) -> bool) -> PathBuf {
    candidates
        .iter()
        .find(|d| usable(d))
        .or_else(|| candidates.first())
        .cloned()
        .unwrap_or_else(std::env::temp_dir)
}

/// Where per-process runtime dirs go: `$TMPDIR` first, then execution-allowing
/// fallbacks — a `noexec` `$TMPDIR` would make every helper fail "Permission denied".
pub(crate) fn exec_scratch_base() -> PathBuf {
    let mut candidates = vec![
        std::env::temp_dir(),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/dev/shm"),
    ];
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR") {
        candidates.push(PathBuf::from(run));
    }
    pick_scratch_base(&candidates, |d| d.is_dir() && !is_noexec(d))
}

/// Writes the embedded `bin/` to `<tmp>/portuale-rt.<pid>/bin`; per process, so
/// concurrent runs never share. The parent is removed at process exit.
pub(crate) fn extract_embedded_runtime() -> Result<PathBuf, String> {
    let root = exec_scratch_base().join(format!("portuale-rt.{}", std::process::id()));
    let bin = root.join("bin");
    let _ = std::fs::remove_dir_all(&root);
    register_cleanup(root.clone());
    crate::embedded_runtime::extract("bin", &bin).map_err(|e| {
        format!(
            "could not extract the embedded ebuild phase runtime to {}: {e} \
             (set {BIN_DIR_VAR} to a copy of the repository's bin/ directory, \
             or point TMPDIR at a writable directory)",
            bin.display()
        )
    })?;
    Ok(bin)
}

/// Dirs created under `$TMPDIR`, removed at process exit via one `atexit`
/// registration (statics never run destructors; signal kills still leak).
pub(crate) fn register_cleanup(dir: PathBuf) {
    use std::sync::{Mutex, Once};
    static DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static REGISTER: Once = Once::new();
    // `extern "C"`, no closure capture: the handler reads `DIRS` itself.
    extern "C" fn cleanup_created_dirs() {
        if let Ok(dirs) = DIRS.lock() {
            for dir in dirs.iter() {
                remove_bin_overlay_dir(dir);
            }
        }
    }
    if let Ok(mut dirs) = DIRS.lock() {
        dirs.push(dir);
    }
    REGISTER.call_once(|| {
        let _ = unsafe { libc::atexit(cleanup_created_dirs) };
    });
}

/// Fails with [`bin_dir`]'s message when the runtime is missing — before anything
/// is shipped, fetched or written, never half-way through a plan.
pub(crate) fn require_phase_runtime() -> Result<(), String> {
    bin_dir().map(|_| ())
}

/// Removes an overlay dir, ignoring every failure (never worth failing a phase or
/// process exit over). Factored out so the idempotency contract is unit-pinned.
pub(crate) fn remove_bin_overlay_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Symlinks every checkout entry into `overlay`, then every vendored entry over
/// the top. Rebuilt from scratch each call.
pub(crate) fn build_bin_overlay(
    overlay: &Path,
    checkout: &Path,
    vendored: &Path,
) -> std::io::Result<()> {
    use std::os::unix::fs::symlink;
    let _ = std::fs::remove_dir_all(overlay);
    std::fs::create_dir_all(overlay)?;
    for entry in portage_util::read_dir_entries(checkout)? {
        symlink(entry.path(), overlay.join(entry.file_name()))?;
    }
    for entry in portage_util::read_dir_entries(vendored)? {
        let dest = overlay.join(entry.file_name());
        let _ = std::fs::remove_file(&dest);
        symlink(entry.path(), dest)?;
    }
    Ok(())
}

/// Locates a repo from an ebuild file by walking up for `profiles/repo_name`;
/// `None` for standalone files. Shared by `fetch_sources` and `ebuild_package`'s
/// metadata lookup.
/// Portage: portage's own repo-locating mechanism
pub(crate) fn repo_root_for(pkg_dir: &Path) -> Option<PathBuf> {
    for ancestor in pkg_dir.ancestors() {
        if ancestor.join("profiles").join("repo_name").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
