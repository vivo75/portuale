//! Regenerate stale GNU info directory indexes (`<infodir>/dir`) after a
//! vdb-changing merge, by running the host `/usr/bin/install-info` over
//! each info file in them. Without the host binary the whole run is a
//! silent no-op.
// Portage: _info_files.py, chk_updated_info_files (called from post_emerge.py, post_emerge)

use std::collections::BTreeMap;
use std::io::Write;
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
    crate::privileges::path_writable(path)
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

/// `post_emerge`'s info block: `infodirs` re-read after `env-update` (here
/// `env_update::info_dir_values`), skipped when `"noinfo"` is in `FEATURES`,
/// then the memo written back only when the regen changed it. The
/// vdb-changed / `--pretend` / vdb-lock gates live with the callers: every
/// call site is a post-merge sequence after a successful non-pretend
/// merge/unmerge, and a merge that succeeded already proved the vdb writable.
pub fn post_merge_info_update(
    root: &Path,
    color: &Colorizer,
    quiet: bool,
    noinfo: bool,
) -> Result<InfoReport, String> {
    if noinfo {
        return Ok(InfoReport::default());
    }
    let infodirs = crate::env_update::info_dir_values(root);
    let mut prev_mtimes = crate::mtimedb::read_info_mtimes(root);
    let before = prev_mtimes.clone();
    // Production sinks: locked stdout/stderr.
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let report = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
        root,
        infodirs: &infodirs,
        prev_mtimes: &mut prev_mtimes,
        quiet,
        color,
        install_info: INSTALL_INFO,
        out: &mut out,
        err: &mut err,
    })?;
    if prev_mtimes != before {
        crate::mtimedb::write_info_mtimes(root, &prev_mtimes)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn test_color() -> Colorizer {
        // No colour in tests, so assertions read the plain strings.
        Colorizer::new(false)
    }

    fn tmproot() -> TempDir {
        TempDir::new("info_files_test")
    }

    fn install_info_present() -> bool {
        Path::new(INSTALL_INFO).exists()
    }

    /// A small valid `.info` file plus a stale but well-formed `dir` index
    /// (the shape the pmtest contract test installs through the
    /// `dev-libs/dirregenpkg` fixture ebuild).
    fn write_info_pair(info_dir: &Path) {
        let mut info = std::fs::File::create(info_dir.join("infopkg.info")).unwrap();
        write!(
            info,
            "This is infopkg.info, an Info document.\n\
             \n\
             INFO-DIR-SECTION Test\n\
             START-INFO-DIR-ENTRY\n\
             * infopkg: (infopkg). Fixture package for info dir regeneration.\n\
             END-INFO-DIR-ENTRY\n\
             \n\
             \x1f\n\
             File: infopkg.info,  Node: Top,  Up: (dir)\n\
             \n\
             Top\n\
             ***\n\
             \n\
             content here\n"
        )
        .unwrap();
        std::fs::write(
            info_dir.join("dir"),
            "This is the file .../info/dir, which contains the\n\
             topmost node of the Info hierarchy, called (dir)Top.\n\
             The first time you invoke Info you start off looking at this node.\n\
             \x1f\n\
             File: dir,\tNode: Top\tThis is the top of the INFO hierarchy\n\
             \n\
             * Menu:\n\
             \n\
             * Foo: (foo). Foo docs.\n",
        )
        .unwrap();
    }

    fn scratch_root_with_infopath() -> (TempDir, String) {
        let root = tmproot();
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/50-test"),
            "INFOPATH=\"/usr/share/info\"\n",
        )
        .unwrap();
        let info_dir = root.join("usr/share/info");
        std::fs::create_dir_all(&info_dir).unwrap();
        write_info_pair(&info_dir);
        let inforoot = normalize_path(&format!("{}/usr/share/info", root.display()));
        (root, inforoot)
    }

    #[test]
    fn regen_records_the_dir_mtime_and_cleans_the_old_files() {
        // After the post-merge step `dir` holds the entry, `dir.old` is
        // gone, and the memo records the dir's mtime.
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, inforoot) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();

        let report = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: false,
            color: &color,
            install_info: INSTALL_INFO,
            out: &mut out,
            err: &mut err,
        })
        .unwrap();

        assert!(report.ran);
        assert_eq!(report.regenerated, vec![inforoot.clone()]);
        assert_eq!((report.icount, report.badcount), (1, 0));
        // The user-visible lines printed for a regen.
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.contains("Regenerating GNU info directory index..."),
            "{printed}"
        );
        assert!(printed.contains("Processed 1 info files."), "{printed}");
        assert!(err.is_empty());
        let dir = std::fs::read_to_string(root.join("usr/share/info/dir")).unwrap();
        assert!(dir.contains("* infopkg: (infopkg)."), "{dir}");
        assert!(
            !root.join("usr/share/info/dir.old").exists(),
            "the .old rename must be cleaned up"
        );
        let mtime = std::fs::metadata(root.join("usr/share/info"))
            .unwrap()
            .mtime();
        assert_eq!(prev.get(&inforoot), Some(&mtime));
        // And the memo round-trips through the mtimedb file.
        crate::mtimedb::write_info_mtimes(&root, &prev).unwrap();
        assert_eq!(crate::mtimedb::read_info_mtimes(&root), prev);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn second_run_with_no_change_is_up_to_date() {
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, _) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let first = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: false,
            color: &color,
            install_info: INSTALL_INFO,
            out: &mut out,
            err: &mut err,
        })
        .unwrap();
        assert_eq!(first.regenerated.len(), 1);

        let mut out = Vec::new();
        let mut err = Vec::new();
        let second = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: false,
            color: &color,
            install_info: INSTALL_INFO,
            out: &mut out,
            err: &mut err,
        })
        .unwrap();
        // Up-to-date branch: nothing regenerated, nothing counted, and the
        // line itself reaches stdout.
        assert!(second.regenerated.is_empty());
        assert_eq!((second.icount, second.badcount), (0, 0));
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.contains("GNU info directory index is up-to-date."),
            "{printed}"
        );
        assert!(err.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quiet_regen_prints_nothing_but_records_the_memo() {
        // `--quiet`: the einfo lines and the bare `\n` are suppressed, while
        // the regen itself -- and its memo record -- still happens.
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, inforoot) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let report = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: true,
            color: &color,
            install_info: INSTALL_INFO,
            out: &mut out,
            err: &mut err,
        })
        .unwrap();
        assert!(report.ran);
        assert_eq!((report.icount, report.badcount), (1, 0));
        assert!(out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
        assert!(err.is_empty(), "{:?}", String::from_utf8_lossy(&err));
        assert!(prev.contains_key(&inforoot));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn noinfo_leaves_the_dir_and_the_memo_alone() {
        // `"noinfo"` in `FEATURES` gates the whole block.
        let (root, _) = scratch_root_with_infopath();
        let color = test_color();
        let before = std::fs::read(root.join("usr/share/info/dir")).unwrap();

        let report = post_merge_info_update(&root, &color, false, true).unwrap();

        assert_eq!(report, InfoReport::default());
        assert_eq!(
            std::fs::read(root.join("usr/share/info/dir")).unwrap(),
            before
        );
        assert!(crate::mtimedb::read_info_mtimes(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keepinfodir_skips_the_directory() {
        // A dir containing a `.keepinfodir*` entry is never regenerated.
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, inforoot) = scratch_root_with_infopath();
        std::fs::write(root.join("usr/share/info/.keepinfodir-foo"), "").unwrap();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();

        let report = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: false,
            color: &color,
            install_info: INSTALL_INFO,
            out: &mut out,
            err: &mut err,
        })
        .unwrap();

        assert!(report.regenerated.is_empty());
        assert!(!prev.contains_key(&inforoot));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_install_info_is_a_silent_no_op() {
        // Without the host binary nothing at all happens -- exercised here
        // against a bogus path so the test holds with or without install-info.
        let (root, _) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();

        let report = chk_updated_info_files(ChkUpdatedInfoFilesArgs {
            root: &root,
            infodirs: &["/usr/share/info".to_string()],
            prev_mtimes: &mut prev,
            quiet: false,
            color: &color,
            install_info: "/nonexistent/install-info-for-test",
            out: &mut out,
            err: &mut err,
        })
        .unwrap();

        assert_eq!(report, InfoReport::default());
        assert!(prev.is_empty());
        // Silent means silent: neither sink sees a byte.
        assert!(out.is_empty());
        assert!(err.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_output_matches_portage_three_way_split() {
        assert_eq!(classify_output(""), None);
        assert_eq!(
            classify_output("install-info: warning: no info dir entry in `/x'"),
            None
        );
        assert_eq!(
            classify_output(
                "something\ninstall-info: menu item `foo' already exists, for file `bar'"
            ),
            None
        );
        assert_eq!(
            classify_output("install-info: No such file or directory for /x"),
            Some("install-info: No such file or directory for /x")
        );
    }

    #[test]
    fn skippable_names_mirror_the_dir_extensions_tuple() {
        let inforoot = Path::new("/nonexistent-inforoot");
        for name in [
            "dir",
            "dir.gz",
            "dir.bz2",
            "dir.xz",
            "dir.lz",
            "dir.lz4",
            "dir.zst",
            "dir.lzma",
            "dir.old",
            "dir.gz.old",
            ".hidden",
        ] {
            assert!(
                skippable_name(std::ffi::OsStr::new(name), inforoot),
                "{name} must be skipped"
            );
        }
        for name in ["infopkg.info", "dirfoo", "mydir", "dir2"] {
            assert!(
                !skippable_name(std::ffi::OsStr::new(name), inforoot),
                "{name} must be processed"
            );
        }
    }

    #[test]
    fn normalize_path_collapses_the_root_join_double_slash() {
        // `normalize_path(root + z)`: `"/" + "/usr/share/info"`.
        assert_eq!(normalize_path("//usr/share/info"), "/usr/share/info");
        assert_eq!(normalize_path("/usr/share/info/"), "/usr/share/info");
        assert_eq!(normalize_path("/"), "/");
    }
}
