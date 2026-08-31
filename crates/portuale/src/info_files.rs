//! Regenerate stale GNU info directory indexes (`<infodir>/dir`) after a
//! vdb-changing merge, by running the host `/usr/bin/install-info` over
//! each info file in them. Without the host binary the whole run is a
//! silent no-op.
// Portage: _info_files.py, chk_updated_info_files (called from post_emerge.py, post_emerge)

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::color::Colorizer;

/// The host binary shelled out to: a host path, never `ROOT`-relative.
const INSTALL_INFO: &str = "/usr/bin/install-info";

/// The `dir_extensions` tuple, in order.
const DIR_EXTENSIONS: &[&str] = &["", ".gz", ".bz2", ".xz", ".lz", ".lz4", ".zst", ".lzma"];

/// An `already installed` file is not an error.
const ALREADY_EXISTS: &str = "already exists, for file `";

/// The (harmless) no-DIR-header warning prefix: `install-info: warning:
/// no info dir entry in ` (all 44 chars).
const NO_DIR_ENTRY_PREFIX: &str = "install-info: warning: no info dir entry in ";

/// What one `chk_updated_info_files` run did (for tests and the
/// mtimedb-commit gate: `prev_mtimes` changed exactly when `regenerated`
/// is non-empty).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InfoReport {
    /// `false` when the host has no `/usr/bin/install-info`: the whole
    /// function is a no-op (nothing printed, nothing recorded).
    pub ran: bool,
    /// The inforoot dirs that were regenerated, in infodirs order.
    pub regenerated: Vec<String>,
    /// Every processed info file, errors included.
    pub icount: usize,
    /// Processed files whose install-info output was neither silent nor one
    /// of the two harmless shapes.
    pub badcount: usize,
}

/// `portage.util.normalize_path`: `normpath`, except a leading `//`
/// collapses to `/` (the `root + z` join reaches here as
/// `"//usr/share/info"`).
fn normalize_path(mypath: &str) -> String {
    let absolute = mypath.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for comp in mypath.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// Writability gate via `libc::access(W_OK)` (same syscall and UID semantics
/// as Portage's `os.access(inforoot, os.W_OK)` default).
fn is_writable(path: &Path) -> bool {
    let Ok(bytes) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `bytes` is a NUL-terminated path buffer; `access(2)` only
    // reads it.
    unsafe { libc::access(bytes.as_ptr(), libc::W_OK) == 0 }
}

/// Dot-files, subdirectories, and the `dir{,ext}{,.old}` names are never
/// info files.
fn skippable_name(name: &std::ffi::OsStr, inforoot: &Path) -> bool {
    let lossy = name.to_string_lossy();
    if lossy.starts_with('.') || inforoot.join(name).is_dir() {
        return true;
    }
    if lossy.starts_with("dir") {
        for ext in DIR_EXTENSIONS {
            if lossy == format!("dir{ext}") || lossy == format!("dir{ext}.old") {
                return true;
            }
        }
    }
    false
}

/// How one install-info run's merged output classifies: silent, `already
/// exists`, or the no-entry warning are all harmless (`None`); anything
/// else is collected error text (`Some`).
fn classify_output(myso: &str) -> Option<&str> {
    if myso.is_empty() {
        return None;
    }
    if myso.contains(ALREADY_EXISTS) {
        return None;
    }
    if myso.starts_with(NO_DIR_ENTRY_PREFIX) {
        return None;
    }
    Some(myso)
}

/// Reusable `install-info` spawn for tests: `None` when the binary is
/// missing at `binary` or fails to start.
fn run_install_info(binary: &str, inforoot: &str, name: &std::ffi::OsStr) -> Option<String> {
    let out = std::process::Command::new(binary)
        .arg(format!("--dir-file={inforoot}/dir"))
        .arg(Path::new(inforoot).join(name))
        .env("LANG", "C")
        .env("LANGUAGE", "C")
        .output()
        .ok()?;
    // Captured separately and concatenated stdout-then-stderr; only a
    // substring test and a prefix test ever read the text.
    let mut merged = out.stdout;
    merged.extend_from_slice(&out.stderr);
    // Portage decodes lossy and strips trailing newlines the same way.
    Some(
        String::from_utf8_lossy(&merged)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// Human error context for filesystem operations the caller does not
/// tolerate.
fn fs_err(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Arguments of [`chk_updated_info_files`].
pub struct ChkUpdatedInfoFilesArgs<'a> {
    pub root: &'a Path,
    pub infodirs: &'a [String],
    pub prev_mtimes: &'a mut BTreeMap<String, i64>,
    pub quiet: bool,
    pub color: &'a Colorizer,
    pub install_info: &'a str,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// `chk_updated_info_files(root, infodirs, prev_mtimes)`: `prev_mtimes` is
/// `mtimedb["info"]` (mutated in place; the caller commits it), `quiet` is
/// `--quiet`. Output goes to the two sinks (production passes locked
/// stdout/stderr; tests pass buffers so the suppression itself is pinned).
pub fn chk_updated_info_files(args: ChkUpdatedInfoFilesArgs<'_>) -> Result<InfoReport, String> {
    let ChkUpdatedInfoFilesArgs {
        root,
        infodirs,
        prev_mtimes,
        quiet,
        color,
        install_info,
        out,
        err,
    } = args;
    let mut report = InfoReport::default();
    // Without the host binary the whole function is a silent no-op.
    if !Path::new(install_info).exists() {
        return Ok(report);
    }
    report.ran = true;

    let root_str = root.display().to_string();
    let mut regen_infodirs: Vec<String> = Vec::new();
    for z in infodirs {
        if z.is_empty() {
            continue;
        }
        let inforoot = normalize_path(&format!("{root_str}{z}"));
        let inforoot_path = Path::new(&inforoot);
        if !inforoot_path.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(inforoot_path).map_err(|e| fs_err(inforoot_path, e))?;
        let mut keep = false;
        for entry in entries {
            let entry = entry.map_err(|e| fs_err(inforoot_path, e))?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".keepinfodir")
            {
                keep = true;
                break;
            }
        }
        if keep {
            continue;
        }
        let infomtime = std::fs::metadata(inforoot_path)
            .map_err(|e| fs_err(inforoot_path, e))?
            .mtime();
        if prev_mtimes.get(&inforoot) != Some(&infomtime) {
            regen_infodirs.push(inforoot);
        }
    }

    if regen_infodirs.is_empty() {
        // Up-to-date branch: a bare `\n` plus the einfo, both suppressed
        // under `--quiet`.
        if !quiet {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "{}GNU info directory index is up-to-date.",
                color.c("INFO", " * ")
            );
        }
        return Ok(report);
    }

    if !quiet {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{}Regenerating GNU info directory index...",
            color.c("INFO", " * ")
        );
    }

    let mut icount: usize = 0;
    let mut badcount: usize = 0;
    let mut errmsg = String::new();
    for inforoot in &regen_infodirs {
        // The `""` guard is dead (`normalize_path` never returns it) but
        // kept for fidelity.
        if inforoot.is_empty() {
            continue;
        }
        let inforoot_path = Path::new(inforoot);
        if !inforoot_path.is_dir() || !is_writable(inforoot_path) {
            continue;
        }

        let mut file_list: Vec<std::ffi::OsString> = std::fs::read_dir(inforoot_path)
            .map_err(|e| fs_err(inforoot_path, e))?
            .filter_map(|e| e.ok().map(|entry| entry.file_name()))
            .collect();
        file_list.sort();
        let dir_file = inforoot_path.join("dir");
        let mut moved_old_dir = false;
        let mut processed_count: usize = 0;
        for x in &file_list {
            if skippable_name(x, inforoot_path) {
                continue;
            }
            if processed_count == 0 {
                // Rename every `dir<ext>` aside, tolerating only `ENOENT`.
                for ext in DIR_EXTENSIONS {
                    let from = PathBuf::from(format!("{}{}", dir_file.display(), ext));
                    let to = PathBuf::from(format!("{}{}.old", dir_file.display(), ext));
                    match std::fs::rename(&from, &to) {
                        Ok(()) => moved_old_dir = true,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(fs_err(&from, e)),
                    }
                }
            }
            processed_count += 1;
            let myso = run_install_info(install_info, inforoot, x).unwrap_or_default();
            if let Some(bad) = classify_output(&myso) {
                badcount += 1;
                errmsg.push_str(bad);
                errmsg.push('\n');
            }
            icount += 1;
        }

        // No new `dir` produced: put the old files back.
        if moved_old_dir && !dir_file.exists() {
            for ext in DIR_EXTENSIONS {
                let from = PathBuf::from(format!("{}{}.old", dir_file.display(), ext));
                let to = PathBuf::from(format!("{}{}", dir_file.display(), ext));
                match std::fs::rename(&from, &to) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(fs_err(&from, e)),
                }
            }
        }

        // Clean `.old` cruft so it can't block an unmerge of an otherwise
        // empty directory.
        for ext in DIR_EXTENSIONS {
            let old = PathBuf::from(format!("{}{}.old", dir_file.display(), ext));
            match std::fs::remove_file(&old) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(fs_err(&old, e)),
            }
        }

        // Re-stat after the regen (install-info rewrote `dir`).
        prev_mtimes.insert(
            inforoot.clone(),
            std::fs::metadata(inforoot_path)
                .map_err(|e| fs_err(inforoot_path, e))?
                .mtime(),
        );
        report.regenerated.push(inforoot.clone());
    }

    report.icount = icount;
    report.badcount = badcount;
    if badcount > 0 {
        // `ERR`-coloured ` * ` on stderr, always (even `--quiet`).
        let _ = writeln!(
            err,
            "{}{}",
            color.c("ERR", " * "),
            format_args!("Processed {icount} info files; {badcount} errors.")
        );
        // Collected error text: stderr, always (even `--quiet`).
        let _ = write!(err, "{errmsg}");
    } else if icount > 0 && !quiet {
        let _ = writeln!(
            out,
            "{}{}",
            color.c("INFO", " * "),
            format_args!("Processed {icount} info files.")
        );
    }
    Ok(report)
}
