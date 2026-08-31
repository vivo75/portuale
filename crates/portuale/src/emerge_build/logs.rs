//! Build logs.

use crate::emerge_build::*;

/// Also used by the binary-merge digest-failure path, which logs to the
/// same `PORTAGE_LOG_FILE`.
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
    // `PORTAGE_LOGDIR`/`PORTAGE_LOG_FILE_SEP`/`FEATURES=split-log` are read
    // here, where the log path is decided, and handed to the pure
    // `ensure_portage_logdir_symlink` so it stays unit-testable with no
    // env involvement.
    let logdir = std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let sep = std::env::var("PORTAGE_LOG_FILE_SEP").unwrap_or_else(|_| ":".to_string());
    let split_log = features.split_whitespace().any(|t| t == "split-log");
    // The `.gz` suffix applies to both the `${T}` path and the
    // `PORTAGE_LOG_FILE` below, so the symlink and its target agree.
    let compress = features
        .split_whitespace()
        .any(|t| t == "compress-build-logs");
    let path = builddir.join("temp").join(if compress {
        "build.log.gz"
    } else {
        "build.log"
    });
    ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path: &path,
        builddir: &builddir,
        category,
        pf: &format!("{package}-{version}"),
        logdir: logdir.as_deref(),
        sep: &sep,
        split_log,
        compress,
    });
    path
}

/// Arguments of [`ensure_portage_logdir_symlink`].
pub(crate) struct EnsurePortageLogdirSymlinkArgs<'a> {
    pub(crate) tmpdir_log_path: &'a Path,
    pub(crate) builddir: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) logdir: Option<&'a Path>,
    pub(crate) sep: &'a str,
    pub(crate) split_log: bool,
    pub(crate) compress: bool,
}

/// How `build_log_path` places the log when `PORTAGE_LOGDIR` is set: the
/// build log lives under `logdir` permanently and `tmpdir_log_path`
/// (`${T}/build.log`) becomes a symlink to it. A no-op when `logdir` is
/// `None` or cannot be created (then logging is disabled for the build).
///
/// `logid_time` is the `.logid` marker file's mtime, created on first use
/// and reused afterwards, so every phase of one build shares a timestamp.
// Portage: prepare_build_dirs.py, prepare_build_dirs
pub(crate) fn ensure_portage_logdir_symlink(args: EnsurePortageLogdirSymlinkArgs<'_>) {
    let EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path,
        builddir,
        category,
        pf,
        logdir,
        sep,
        split_log,
        compress,
    } = args;
    let Some(logdir) = logdir else {
        return;
    };
    if std::fs::create_dir_all(logdir).is_err() {
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

    // The `.gz` goes on the log file name itself, in both layouts.
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

    // Skip if the symlink already points at the right place.
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
/// A `.gz` path is gunzipped first.
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
