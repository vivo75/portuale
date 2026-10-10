//! Build logs.

use crate::emerge_build::*;

/// `pub(crate)` for the binary-merge digest-failure path
/// (`emerge_getbinpkg`): real logs `BinpkgVerifier`'s digest block to
/// this same `PORTAGE_LOG_FILE`.
pub(crate) fn build_log_path(
    portage_tmpdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    features: &str,
) -> PathBuf {
    let builddir = portage_tmpdir
        .join("portage")
        .join(category)
        .join(format!("{package}-{version}"));
    // Real `PORTAGE_LOGDIR`/`PORTAGE_LOG_FILE_SEP`/`FEATURES=split-log`
    // -- the same "env var, not full config resolution" shortcut this
    // whole CLI boundary already uses elsewhere (`DISTDIR`/`FEATURES`
    // tokens/...); read once here, right where the log path itself is
    // decided, and handed to the pure logic in
    // `ensure_portage_logdir_symlink` so that function stays directly
    // unit-testable with no env-var involvement at all.
    let logdir = std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let sep = std::env::var("PORTAGE_LOG_FILE_SEP").unwrap_or_else(|_| ":".to_string());
    let split_log = features.split_whitespace().any(|t| t == "split-log");
    // Real `compress_log_ext` (`prepare_build_dirs.py:397-399`): the
    // `.gz` suffix applies to BOTH the `${T}` path and the
    // `PORTAGE_LOG_FILE` below, so the symlink and its target agree.
    let compress = features
        .split_whitespace()
        .any(|t| t == "compress-build-logs");
    let path = builddir.join("temp").join(if compress {
        "build.log.gz"
    } else {
        "build.log"
    });
    ensure_portage_logdir_symlink(
        &path,
        &builddir,
        category,
        &format!("{package}-{version}"),
        logdir.as_deref(),
        &sep,
        split_log,
        compress,
    );
    path
}

/// Real `prepare_build_dirs()`'s own `PORTAGE_LOGDIR`/`FEATURES=
/// split-log` handling (`prepare_build_dirs.py:368-468`): when
/// `logdir` is given (`PORTAGE_LOGDIR` set, `build_log_path`'s own
/// doc comment), the real build log lives there permanently --
/// `<logdir>/<CATEGORY><sep><PF><sep><logid_time>.log`, or under
/// `split_log`, `<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log`
/// -- and `tmpdir_log_path` (`${T}/build.log`) becomes a symlink to it
/// rather than the real file. With `compress`
/// (`FEATURES=compress-build-logs`, `prepare_build_dirs.py:397-399`'s
/// own `compress_log_ext = ".gz"`), both names gain the `.gz` suffix
/// and phase output is gzip-encoded into the file
/// (`ebuild_phases::open_log_file`'s pump thread -- real
/// `EbuildPhase._open_log`'s `gzip.GzipFile(mode="ab")`). Everything
/// downstream that opens `tmpdir_log_path` still just opens the path
/// `build_log_path` returned, symlinks transparently followed by
/// `std::fs`, so this is the only place that needs to know about
/// `PORTAGE_LOGDIR` at all. A no-op when `logdir` is `None`
/// (`PORTAGE_LOGDIR` unset or empty -- real's own `if
/// mysettings.get("PORTAGE_LOGDIR", "") == "": del it`) or when it
/// can't be created (real's own "Permission issues... Disabling
/// logging").
///
/// `logid_time`: a `.logid` marker file's own mtime (real's own
/// `os.stat(logid_path).st_mtime`), created on first use and reused
/// afterward -- so every phase of the same build (each its own fresh
/// shell, `ebuild_phases::run_one_phase`'s own doc comment) and a
/// resumed one all share one timestamp, matching real exactly.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn ensure_portage_logdir_symlink(
    tmpdir_log_path: &Path,
    builddir: &Path,
    category: &str,
    pf: &str,
    logdir: Option<&Path>,
    sep: &str,
    split_log: bool,
    compress: bool,
) {
    let Some(logdir) = logdir else {
        return;
    };
    if std::fs::create_dir_all(logdir).is_err() {
        // Real: permission issues here disable logging for this build
        // entirely (`tmpdir_log_path` stays the real file).
        return;
    }

    let logid_path = builddir.join(".logid");
    let logid_time = std::fs::metadata(&logid_path)
        .and_then(|m| m.modified())
        .or_else(|_| {
            std::fs::create_dir_all(builddir)?;
            std::fs::write(&logid_path, [])?;
            std::fs::metadata(&logid_path)?.modified()
        })
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let stamp = crate::elog::utc_stamp_at(logid_time);

    // Real `compress_log_ext`: the `.gz` goes on the real log file
    // name itself (both `split-log` and flat layouts).
    let ext = if compress { ".log.gz" } else { ".log" };
    let (log_subdir, real_log) = if split_log {
        let subdir = logdir.join("build").join(category);
        let file = subdir.join(format!("{pf}{sep}{stamp}{ext}"));
        (subdir, file)
    } else {
        let file = logdir.join(format!("{category}{sep}{pf}{sep}{stamp}{ext}"));
        (logdir.to_path_buf(), file)
    };
    if std::fs::create_dir_all(&log_subdir).is_err() {
        return;
    }

    // Real's own idempotent re-symlink check (`make_new_symlink`):
    // skip if it already points at the right place.
    let needs_new = std::fs::read_link(tmpdir_log_path)
        .map(|target| target != real_log)
        .unwrap_or(true);
    if needs_new {
        if let Some(parent) = tmpdir_log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(tmpdir_log_path);
        let _ = std::os::unix::fs::symlink(&real_log, tmpdir_log_path);
    }
}

/// Last `n` lines of `path`, or a short "(build log unavailable)" note.
/// A `.gz` path (`FEATURES=compress-build-logs`) is gunzipped first --
/// real's own failure display (`Scheduler.py`) and QA scan
/// (`doebuild.py`) both wrap a `.gz`-suffixed log in
/// `gzip.GzipFile(mode="rb")` before reading.
pub(crate) fn tail_of(path: &Path, n: usize) -> String {
    let is_gz = path.extension().is_some_and(|ext| ext == "gz");
    let text: Option<String> = if is_gz {
        std::fs::File::open(path).ok().and_then(|f| {
            use std::io::Read;
            let mut decoder = flate2::read::MultiGzDecoder::new(f);
            let mut s = String::new();
            decoder.read_to_string(&mut s).ok().map(|_| s)
        })
    } else {
        std::fs::read_to_string(path).ok()
    };
    match text {
        Some(s) => {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        None => "(build log unavailable)".to_string(),
    }
}
