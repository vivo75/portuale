//! Where the phase runtime (bin/, pym/) and the repository live; the embedded runtime.

use crate::ebuild_phases::*;

/// The tree this binary was compiled in (two levels above the manifest
/// directory), canonical when it still exists, else the same path as
/// compiled in.
///
/// Never panics (backlog #322): a binary copied out of its build tree
/// (a container, a minimal host) has no such directory, and every
/// consumer below only ever *joins* a path onto this and checks the
/// result exists, so a missing tree degrades exactly like a missing
/// `3rdparty/portage` always has. The consumer that cannot degrade --
/// the vendored `bin/` -- reports it through [`bin_dir`].
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

/// The gitignored working checkout of upstream Portage (`3rdparty/portage/`
/// by default, overridable with `$PORTUALE_PORTAGE_CHECKOUT`).
///
/// Since #326 S2–S7 nothing at runtime needs it: every helper is native
/// (`helpers/`, reached through `bin/portuale-python`), and
/// `has_version`/`best_version` go through the vendored
/// `portageq-wrapper` shim (the native `portuale portageq`). What still
/// reads this path: the D2 oracle overlay (`bin_dir()` overlays `bin/`
/// on `<checkout>/bin/` only with `PORTUALE_PYTHON_HELPERS=real`),
/// `package_sets_conf()`'s first choice (falling back to the copy
/// compiled into the binary), and the re-sync / fixture-generator
/// source of record. Absent when nobody cloned it -- callers degrade the
/// same way a missing binary already does. See `3rdparty/repos.toml` for
/// the pinned ref.
pub(crate) fn portage_checkout() -> PathBuf {
    if let Some(p) = std::env::var_os("PORTUALE_PORTAGE_CHECKOUT") {
        return PathBuf::from(p);
    }
    repo_root().join("3rdparty/portage")
}

/// Value for `PORTAGE_PYM_PATH` given a Portage checkout dir: `<checkout>/lib`
/// when `<checkout>/lib/portage` is a dir, else `/`.
///
/// Real Portage always sets the variable (never empty): it is the realpath of
/// its own `lib/` (`const.py:102`,
/// `PORTAGE_PYM_PATH = os.path.realpath(os.path.join(__file__, "../.."))`),
/// exported through the config as a constant key
/// (`package/ebuild/config.py:3157`, `if mykey == "PORTAGE_PYM_PATH": return
/// portage._pym_path`). The shell dies on a non-existent value: real
/// `__dyn_clean` (`bin/phase-functions.sh:316`, `cd "${PORTAGE_PYM_PATH}" ||
/// die "PORTAGE_PYM_PATH does not exist: ..."` at `:331-332`) runs that `cd`
/// unconditionally (a cwd outside the build dir, since the dir is removed),
/// and `bin/ebuild.sh:211-212` has the same `die`. `/` is always a valid cwd.
/// Without a checkout the value is `/`; no helper needs the real path any
/// more (all native, #326 S7) -- the variable only preserves real's
/// invariant that it is never empty.
pub(crate) fn pym_path_value(checkout: &Path) -> String {
    let lib = checkout.join("lib");
    if lib.join("portage").is_dir() {
        lib.display().to_string()
    } else {
        "/".to_string()
    }
}

/// Value for `PORTAGE_PYTHONPATH` given a Portage checkout dir:
/// `Some("<checkout>/lib")` under the same condition as [`pym_path_value`],
/// else `None` (the caller pushes nothing).
///
/// Real `bin/misc-functions.sh:600,621` prefers `PORTAGE_PYTHONPATH` over
/// `PORTAGE_PYM_PATH` (`PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}`)
/// for the `gpkg-helper.py`/`xpak-helper.py` invocation, so with a checkout
/// the real helper imports `portage` from this checkout. Without a checkout
/// no entry is pushed at all -- and none is needed, since every helper is
/// native (#326 S7).
pub(crate) fn pythonpath_value(checkout: &Path) -> Option<String> {
    let lib = checkout.join("lib");
    if lib.join("portage").is_dir() {
        Some(lib.display().to_string())
    } else {
        None
    }
}

/// The text of Portage's `cnf/sets/portage.conf` (the built-in package-set
/// definitions behind `--list-sets` and set-name search): the checkout's
/// file when there is one (a developer tree, byte-identical to before), else
/// the copy compiled into the binary (backlog #322), so a relocated binary
/// lists the same sets instead of silently listing none.
pub(crate) fn package_sets_conf() -> Option<String> {
    std::fs::read_to_string(portage_checkout().join("cnf/sets/portage.conf"))
        .ok()
        .or_else(|| {
            crate::embedded_runtime::file("cnf/sets/portage.conf")
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        })
}

/// The directory `PORTAGE_BIN_PATH` points at for real phase execution.
///
/// `bin/` (repo root) is a tracked, vendored copy of upstream Portage's
/// own `bin/` runtime -- all the `.sh` (`ebuild.sh` and its whole source
/// closure), every `ebuild-helpers/` script, `estrip`/`ecompress`,
/// `ecompress-file`, the `*-qa-check.d/` sets and the stdlib-only
/// `filter-bash-environment.py` -- so `emerge` runs on a host with no
/// Portage installed. Portuale-owned shims live next to them:
/// `portuale-python` (`PORTAGE_PYTHON` in native mode, #326 D1),
/// `chmod-lite` (the native helper, or the upstream script in `real`
/// mode, #326 D2), `ebuild-ipc` (always 127, #326 D6) and
/// `portageq-wrapper` (the native `portuale portageq`, feat#157 S6).
/// See `bin/README.md`'s portuale-owned list: a re-sync must not
/// overwrite them.
///
/// The `.py` helpers that `import portage` (`doins.py`, `xpak-helper.py`,
/// `gpkg-helper.py`, `dohtml.py`, `xattr-helper.py`, `install.py`) are
/// not vendored. In native mode `portuale-python` answers them all
/// natively (`helpers/`, #326 S3-S7) except `dohtml.py`, which is never
/// in scope (`dohtml` dies first for EAPI >= 7) and exits 127 like any
/// unknown helper; the D1 transition table is gone (S7).
///
/// So: in native mode (unset or `native` `PORTUALE_PYTHON_HELPERS`, the
/// default) `PORTAGE_BIN_PATH` is the vendored `bin/` directly and no
/// overlay is built at all. Only with
/// `PORTUALE_PYTHON_HELPERS=real` (the D2 oracle handle for differential
/// tests, which needs an installed Portage) is it a symlink overlay --
/// vendored `bin/` entries win, the not-vendored `.py` helpers fall
/// through to `<checkout>/bin/`.
///
/// `portageq-wrapper` is deliberately vendored too (feat#157 S6, #151): the
/// tracked `bin/portageq-wrapper` shim execs the native `portuale portageq`
/// (via `$PORTUALE_BIN`), and because vendored entries are linked over the
/// checkout's, the shim wins even when `3rdparty/portage` is present --
/// `has_version`/`best_version` never need Python or the checkout.
///
/// Resolved once per process. `bin/ebuild.sh` only ever uses
/// `${PORTAGE_BIN_PATH}` as a literal string prefix for `source`, never
/// `realpath`s it, so a symlinked entry resolves to the vendored file.
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
        // #326 D2: the checkout overlay exists only in `real` mode (the
        // oracle handle for differential tests). In native mode (unset or
        // `native`) the vendored dir is the whole runtime.
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

/// Environment variable naming a directory that holds the vendored phase
/// runtime (a copy of the repository's `bin/`), for a binary run outside
/// the tree it was built in (backlog #322).
pub(crate) const BIN_DIR_VAR: &str = "PORTUALE_BIN_DIR";

/// Where [`resolve_bin_dir`] found the phase runtime.
#[derive(Debug, PartialEq)]
pub(crate) enum BinSource {
    /// A directory on disk that already holds it.
    Dir(PathBuf),
    /// Nowhere on disk: use the copy compiled into this binary.
    Embedded,
}

/// Where the vendored phase runtime (`bin/ebuild.sh` and friends) is,
/// first hit wins: `$PORTUALE_BIN_DIR` when set and non-empty; the `bin/`
/// of the tree this binary was built in when it still has one (every
/// developer and bed flow, unchanged); the copy compiled into the binary.
/// A set override that is wrong is an error and never falls through: a
/// silent fallback would hide a typo. With no embedded copy and nothing on
/// disk the error says what was looked for and how to fix it. The message
/// carries no program prefix; every caller prints it behind its own
/// (`emerge: `, `ebuild: `).
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

/// True when `dir` is on a filesystem mounted `noexec` (or cannot be
/// inspected, which is treated the same: do not run helpers from it).
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

/// First of `candidates` that `usable` accepts, else the first one (the
/// caller then fails with its own, more specific message).
pub(crate) fn pick_scratch_base(candidates: &[PathBuf], usable: impl Fn(&Path) -> bool) -> PathBuf {
    candidates
        .iter()
        .find(|d| usable(d))
        .or_else(|| candidates.first())
        .cloned()
        .unwrap_or_else(std::env::temp_dir)
}

/// Where the per-process runtime directories (extracted `bin/`, the #88
/// overlay) go. The helpers in them are executed by name from `PATH`, so a
/// `noexec` `$TMPDIR` (hardened hosts mount `/tmp` that way) would make every
/// `doins`/`ecompress`/`estrip` fail with "Permission denied" (backlog #324):
/// prefer `$TMPDIR`, then fall back to directories that allow execution.
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

/// Writes the embedded `bin/` to `<tmp>/portuale-rt.<pid>/bin` and returns
/// that directory; the parent is removed at process exit. Per process, so
/// concurrent runs never share (or race on) a tree.
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

/// Directories this process created under `$TMPDIR` (the `bin/` overlay of
/// backlog #88, the extracted runtime of #322), removed at process exit.
/// Statics never run destructors and `bin_dir()` has call sites in three
/// files with independent exit paths, so per-site removal would sprawl and
/// rot: one `atexit` registration, made on the first directory created,
/// covers every command (and the test harness's subprocess runs) on all
/// normal exits; signal kills still leak, like any tmp dir.
pub(crate) fn register_cleanup(dir: PathBuf) {
    use std::sync::{Mutex, Once};
    static DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static REGISTER: Once = Once::new();
    // `extern "C"`, no closure capture: the handler reads `DIRS` itself. A
    // failed registration keeps the old leak rather than breaking the run.
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

/// Fails with [`bin_dir`]'s message when the phase runtime cannot be
/// found. Called at the top of a real run, before anything is shipped,
/// fetched or written, so a missing runtime is reported up front and
/// never half-way through a plan (backlog #322).
pub(crate) fn require_phase_runtime() -> Result<(), String> {
    bin_dir().map(|_| ())
}

/// Removes a `bin_dir()` overlay directory, ignoring every failure
/// (a partially-removed or already-gone overlay is not an error worth
/// failing a phase -- or process exit -- over). Called by the `atexit`
/// handler; factored out so the idempotency contract is unit-pinned.
pub(crate) fn remove_bin_overlay_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Populates `overlay` with a symlink to every entry of `checkout`, then
/// symlinks every entry of `vendored` over the top (dropping the
/// checkout link first). Rebuilt from scratch each call.
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

/// Real portage's own mechanism for locating a repo from one of its own
/// ebuild files: walks up from `pkg_dir` looking for a `profiles/
/// repo_name` file, returning the *ancestor directory itself* (the real
/// repo root, suitable for `portage_repo::read_md5_cache`) -- `None` for
/// a standalone ebuild file outside any repo checkout. Shared by
/// `fetch_sources` below and `ebuild_package.rs`'s own `Packages`-index
/// metadata lookup (moved here rather than duplicated, since both need
/// exactly the same walk).
pub(crate) fn repo_root_for(pkg_dir: &Path) -> Option<PathBuf> {
    for ancestor in pkg_dir.ancestors() {
        if ancestor.join("profiles").join("repo_name").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
