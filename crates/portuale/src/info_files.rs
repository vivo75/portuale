// Real `chk_updated_info_files` (`lib/portage/util/_info_files.py:14-141`),
// called from real `_emerge/post_emerge.py::post_emerge` (`post_emerge.
// py:126-130`): after an emerge that changed the vdb, regenerate every
// GNU info directory index (`<infodir>/dir`) whose directory mtime no
// longer matches the `mtimedb["info"]` memo, by running the host
// `/usr/bin/install-info` over each info file in it.
//
// The port is line-by-line in spirit:
//   - gated on the *host* `/usr/bin/install-info` existing (not
//     `ROOT`-relative): without it the whole function is a silent no-op
//     (real line 15 -- not even the blank line is printed).
//   - `inforoot = normalize_path(root + z)` per non-empty infodir (real
//     line 21); skipped when it is not a directory or contains an entry
//     starting with `.keepinfodir` (real lines 22-24); regenerated only
//     when its `st_mtime` (whole seconds) differs from
//     `prev_mtimes[inforoot]` or is absent there (real lines 25-27).
//   - output via real `EOutput` (`output.py:663-676,649-661`): a bare
//     `\n` on stdout, then ` * <msg>` einfo lines on stdout / eerror on
//     stderr -- each rendered here through the same
//     `color.c("INFO"|"ERR", " * ")` helper `elog::echo_summary` uses.
//     `--quiet` (real `noiselimit < 0`, `actions.py:3907-3908`) suppresses
//     the einfo lines AND the bare `\n` (real `writemsg_stdout` defaults
//     to `noiselevel=0`, which `--quiet`'s `noiselimit=-1` filters);
//     the eerror line and the collected error text (real
//     `writemsg_level(errmsg, level=ERROR, noiselevel=-1)`, always to
//     stderr since `ERROR >= WARNING`) print regardless.
//   - the regeneration loop (real lines 38-134): sorted `listdir`, skip
//     dot-files, subdirectories, and `dir{,ext}{,.old}` names (the
//     `dir_extensions` tuple); on the first processed file rename every
//     `dir<ext>` to `dir<ext>.old`; run `/usr/bin/install-info
//     --dir-file=<inforoot>/dir <inforoot>/<x>` with `LANG=C LANGUAGE=C`
//     (no shell), stdout+stderr merged; classify the output (contains
//     `already exists, for file \`` or starts with the 44-char
//     `install-info: warning: no info dir entry in ` prefix -- both
//     verified live against install-info (GNU texinfo) 7.3 -- as
//     harmless, anything else as an error); restore the `.old` files
//     when no new `dir` was produced; unlink every `dir<ext>.old`; store
//     the new `st_mtime` in `prev_mtimes` (real line 134, re-statted
//     after the regen).
//
// Two deliberate narrowings:
//   - real merges the child's stderr into its stdout pipe
//     (`stderr=STDOUT`, interleaved); portuale captures both and
//     concatenates stdout-then-stderr. Only a substring test and a
//     prefix test ever read the text, and both behave the same on the
//     concatenated form for every output install-info 7.3 was observed
//     to produce (silent success; the warning prefix first;
//     `already exists` anywhere).
//   - real checks writability with `os.access(inforoot, os.W_OK)`; the
//     port is `libc::access(W_OK)` on the same path (same syscall, same
//     real-UID semantics -- real uses the default `effective_ids=False`,
//     and `access(2)` likewise checks the real UID; `libc` is already a
//     direct dependency).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::color::Colorizer;

/// The host binary real shells out to (real `_info_files.py:76-81`):
/// a host path, never `ROOT`-relative.
const INSTALL_INFO: &str = "/usr/bin/install-info";

/// Real `_info_files.py:38`'s own `dir_extensions` tuple, in order.
const DIR_EXTENSIONS: &[&str] = &["", ".gz", ".bz2", ".xz", ".lz", ".lz4", ".zst", ".lzma"];

/// Real `_info_files.py:95`: an `already installed` file is not an error.
const ALREADY_EXISTS: &str = "already exists, for file `";

/// Real `_info_files.py:101`: the (harmless) no-DIR-header warning
/// prefix -- all 44 chars (`install-info: warning: no info dir entry in
/// `, verified live against install-info (GNU texinfo) 7.3).
const NO_DIR_ENTRY_PREFIX: &str = "install-info: warning: no info dir entry in ";

/// What one `chk_updated_info_files` run did (for tests and the
/// mtimedb-commit gate: `prev_mtimes` changed exactly when `regenerated`
/// is non-empty).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InfoReport {
    /// `false` when the host has no `/usr/bin/install-info` -- real's
    /// own whole-function no-op (nothing printed, nothing recorded).
    pub ran: bool,
    /// The inforoot dirs that were regenerated, in infodirs order.
    pub regenerated: Vec<String>,
    /// Real `icount`: every processed info file, errors included.
    pub icount: usize,
    /// Real `badcount`: processed files whose install-info output was
    /// neither silent nor one of the two harmless shapes.
    pub badcount: usize,
}

/// Real `portage.util.normalize_path` (`util/__init__.py:139-153`):
/// `normpath`, except a leading `//` collapses to `/` (real joins
/// `root + z`, so `"/" + "/usr/share/info"` reaches here as
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

/// Real `_info_files.py:46`'s own `os.access(inforoot, os.W_OK)` gate,
/// same syscall via the already-declared `libc` dependency (real-UID
/// semantics on both sides: real passes the default
/// `effective_ids=False`, and `access(2)` checks the real UID).
fn is_writable(path: &Path) -> bool {
    let Ok(bytes) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `bytes` is a NUL-terminated path buffer; `access(2)` only
    // reads it.
    unsafe { libc::access(bytes.as_ptr(), libc::W_OK) == 0 }
}

/// Real `_info_files.py:57-64`: dot-files, subdirectories, and the
/// `dir{,ext}{,.old}` names are never info files.
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

/// How one install-info run's merged output classifies (real
/// `_info_files.py:95-110`): silent, `already exists`, or the no-entry
/// warning are all harmless (`None`); anything else is collected error
/// text (`Some`). Verified live against install-info (GNU texinfo) 7.3.
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
/// missing at `binary` (real `_info_files.py:86-87`'s own `OSError ->
/// myso = None`) or fails to start.
fn run_install_info(binary: &str, inforoot: &str, name: &std::ffi::OsStr) -> Option<String> {
    let out = std::process::Command::new(binary)
        .arg(format!("--dir-file={inforoot}/dir"))
        .arg(Path::new(inforoot).join(name))
        .env("LANG", "C")
        .env("LANGUAGE", "C")
        .output()
        .ok()?;
    // Real `stderr=STDOUT`: merged. Captured separately here and
    // concatenated stdout-then-stderr (see this module's doc comment).
    let mut merged = out.stdout;
    merged.extend_from_slice(&out.stderr);
    // Real `.decode("utf-8", "replace").rstrip("\n")`.
    Some(
        String::from_utf8_lossy(&merged)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// Human error context for the filesystem operations real lets raise
/// (`listdir`/`stat` outside the tolerated `ENOENT` renames/unlinks).
fn fs_err(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Real `chk_updated_info_files(root, infodirs, prev_mtimes)`
/// (`lib/portage/util/_info_files.py:14-141`): `prev_mtimes` is real
/// `mtimedb["info"]` (mutated in place -- the caller commits it);
/// `quiet` is real `--quiet` (`noiselimit < 0`). Output goes to the two
/// sinks (production passes locked stdout/stderr; tests pass `Vec<u8>`
/// buffers so the `--quiet` suppression itself is pinned, not just the
/// memo side-effect). Eight params (real's three + quiet/color/binary +
/// the two sinks) -- arity-lint noise, not a design smell.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn chk_updated_info_files(
    root: &Path,
    infodirs: &[String],
    prev_mtimes: &mut BTreeMap<String, i64>,
    quiet: bool,
    color: &Colorizer,
    install_info: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<InfoReport, String> {
    let mut report = InfoReport::default();
    // Real line 15: without the host binary the whole function is a
    // silent no-op.
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
        // Real lines 29-32: `writemsg_stdout("\n")` (noiselevel 0 --
        // `--quiet` filters it) + the einfo. (`let _ =` matches
        // `println!`'s own error-ignoring semantics: `print_to`
        // discards write errors.)
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

    // Real lines 33-36.
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
        // Real lines 43-47 (the `""` guard is dead -- `normalize_path`
        // never returns it -- but mirrors real line 43).
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
                // Real lines 65-73: rename every `dir<ext>` aside,
                // tolerating only `ENOENT`.
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
            // Real lines 75-94: spawn, merged output, `OSError -> None`.
            let myso = run_install_info(install_info, inforoot, x).unwrap_or_default();
            // Real lines 95-110.
            if let Some(bad) = classify_output(&myso) {
                badcount += 1;
                errmsg.push_str(bad);
                errmsg.push('\n');
            }
            icount += 1;
        }

        // Real lines 112-121: no new `dir` produced -- put the old files
        // back. (`Path::exists` follows symlinks, like `os.path.exists`.)
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

        // Real lines 123-131: clean `.old` cruft so it can't block an
        // unmerge of an otherwise empty directory.
        for ext in DIR_EXTENSIONS {
            let old = PathBuf::from(format!("{}{}.old", dir_file.display(), ext));
            match std::fs::remove_file(&old) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(fs_err(&old, e)),
            }
        }

        // Real line 134: re-stat after the regen (install-info rewrote
        // `dir`, so the mtime moved).
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
    // Real lines 136-141.
    if badcount > 0 {
        // Real `out.eerror(...)`: `ERR`-coloured ` * ` on stderr (real's
        // `EOutput` here is never quiet -- the `--quiet` suppression is
        // purely the `noiselimit` checks around the einfo calls).
        let _ = writeln!(
            err,
            "{}{}",
            color.c("ERR", " * "),
            format_args!("Processed {icount} info files; {badcount} errors.")
        );
        // Real `writemsg_level(errmsg, level=ERROR, noiselevel=-1)`:
        // stderr, always (even `--quiet`).
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

/// Real `_emerge/post_emerge.py::post_emerge`'s own info block
/// (`post_emerge.py:89-130`): `infodirs` re-read after `env-update`
/// (here: `env_update::info_dir_values`, the same env.d collation real
/// `settings.reload(); settings.regenerate()` derives
/// `${ROOT}/etc/profile.env` from), skipped when `"noinfo" in FEATURES`,
/// then the equivalent of real `mtimedb.commit()` -- the memo is
/// written back only when the regen changed it (real `commit()` itself
/// only writes on change). The vdb-changed / `--pretend` / vdb-lock
/// gates live with the callers: every call site is a post-merge
/// sequence reached only after a successful non-pretend merge/unmerge
/// (see `pretend.rs`); portuale is a single process with no `vardbapi`
/// lock to take, and a merge that succeeded already proved the vdb
/// writable.
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
    // Production sinks: locked stdout/stderr (same bytes `println!` /
    // `eprintln!` emitted before the sink refactor).
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let report = chk_updated_info_files(
        root,
        &infodirs,
        &mut prev_mtimes,
        quiet,
        color,
        INSTALL_INFO,
        &mut out,
        &mut err,
    )?;
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
        // No colour in tests: `color.c` passes text through when the
        // style is disabled, so assertions read the plain real strings.
        Colorizer::new(false)
    }

    fn tmproot() -> std::path::PathBuf {
        TempDir::new("info_files_test").keep()
    }

    fn install_info_present() -> bool {
        Path::new(INSTALL_INFO).exists()
    }

    /// A small valid `.info` file (a `START-INFO-DIR-ENTRY` block naming
    /// `* infopkg:`) plus a stale but well-formed `dir` index -- the
    /// same shape the pmtest real-merge contract test installs through
    /// the `dev-libs/dirregenpkg` fixture ebuild.
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

    fn scratch_root_with_infopath() -> (std::path::PathBuf, String) {
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
        // Real `_info_files.py`'s own contract: after the post-merge
        // step `dir` is regenerated containing the entry, `dir.old` is
        // gone, and `mtimedb["info"][<abs inforoot>]` is the dir's mtime.
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, inforoot) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();

        let report = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            false,
            &color,
            INSTALL_INFO,
            &mut out,
            &mut err,
        )
        .unwrap();

        assert!(report.ran);
        assert_eq!(report.regenerated, vec![inforoot.clone()]);
        assert_eq!((report.icount, report.badcount), (1, 0));
        // The user-visible lines real prints for a regen.
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
        // And the memo round-trips through the real mtimedb file.
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
        let first = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            false,
            &color,
            INSTALL_INFO,
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(first.regenerated.len(), 1);

        let mut out = Vec::new();
        let mut err = Vec::new();
        let second = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            false,
            &color,
            INSTALL_INFO,
            &mut out,
            &mut err,
        )
        .unwrap();
        // Real's own `GNU info directory index is up-to-date.` branch:
        // nothing regenerated, nothing counted -- and the line itself
        // reaches stdout.
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
        // Real `--quiet` (`noiselimit < 0`, `actions.py:3907-3908`): the
        // einfo lines and the bare `\n` are suppressed, while the regen
        // itself -- and its memo record -- still happens. The sinks pin
        // the suppression directly (the pmtest contract test pins the
        // non-quiet lines end to end through the real binary).
        if !install_info_present() {
            eprintln!("skip: no {INSTALL_INFO} on this host");
            return;
        }
        let (root, inforoot) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let report = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            true,
            &color,
            INSTALL_INFO,
            &mut out,
            &mut err,
        )
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
        // Real `post_emerge.py:127`: `"noinfo" not in settings.features`
        // gates the whole block.
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
        // Real `_info_files.py:22-24`: a dir containing an entry
        // starting with `.keepinfodir` is never regenerated.
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

        let report = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            false,
            &color,
            INSTALL_INFO,
            &mut out,
            &mut err,
        )
        .unwrap();

        assert!(report.regenerated.is_empty());
        assert!(!prev.contains_key(&inforoot));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_install_info_is_a_silent_no_op() {
        // Real `_info_files.py:15`: without the host binary nothing at
        // all happens -- exercised here against a bogus path so the
        // test holds whether or not the host ships install-info.
        let (root, _) = scratch_root_with_infopath();
        let color = test_color();
        let mut prev = BTreeMap::new();
        let mut out = Vec::new();
        let mut err = Vec::new();

        let report = chk_updated_info_files(
            &root,
            &["/usr/share/info".to_string()],
            &mut prev,
            false,
            &color,
            "/nonexistent/install-info-for-test",
            &mut out,
            &mut err,
        )
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
        // Real `normalize_path(root + z)`: `"/" + "/usr/share/info"`.
        assert_eq!(normalize_path("//usr/share/info"), "/usr/share/info");
        assert_eq!(normalize_path("/usr/share/info/"), "/usr/share/info");
        assert_eq!(normalize_path("/"), "/");
    }
}
