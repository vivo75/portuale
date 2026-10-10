// Native `chmod-lite` (#326 S2.4, D2/D3): a port of real
// `bin/chmod-lite.py`'s `main()` over `apply_recursive_permissions(
// filename, filemode=0o644, filemask=0o022, dirmode=0o755, dirmask=0o022)`
// (`3rdparty/portage/lib/portage/util/__init__.py:1251`), which itself
// wraps `apply_secpass_permissions` (`:1323`) over `apply_permissions`
// (`:1145`, the mode/mask arithmetic below).
//
// Real `bin/ebuild-pyhelper` records the caller's cwd in
// `__PORTAGE_HELPER_CWD` and `chmod-lite.py` chdirs back to it after the
// imports; the `bin/chmod-lite` shim keeps the caller's cwd, so this
// helper takes its arguments relative to the current directory, as raw
// bytes throughout (argv paths may not be UTF-8).
//
// Fidelity notes (all from the real sources above):
// - minimal chmod calls: a path is chmod'ed only when the arithmetic
//   says a bit must change;
// - symlinks are never followed and never chmod'ed (`follow_links=False`,
//   and `apply_permissions` clears `new_mode` for symlinks);
// - a directory is chmod'ed before it is walked (bug 554084): the walk
//   below chmods each directory before recursing into it;
// - the top-level `FileNotFound` is swallowed for backward compatibility
//   (a missing top returns True), while one deeper in the walk is
//   ignored as an `InvalidLocation`;
// - `EPERM` prints `Operation Not Permitted: ...` and `ENOENT` prints
//   `File Not Found: '...'` (the default `onerror`); anything else real
//   would let propagate (a traceback, exit 1).
// - `main()` returns `os.EX_OK` unconditionally: exit 0 even when some
//   permissions were left unapplied.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const FILEMODE: u32 = 0o644;
const FILEMASK: u32 = 0o022;
const DIRMODE: u32 = 0o755;
const DIRMASK: u32 = 0o022;

/// Native entry: `apply_recursive_permissions` for each argument.
/// Exit 0 like real (`os.EX_OK`), or 1 when an unexpected (non-EPERM,
/// non-ENOENT) OS error occurs, which real reports as a traceback.
pub(crate) fn run(argv: &[OsString]) -> i32 {
    // D2: `PORTUALE_PYTHON_HELPERS=real` brings back today's behaviour:
    // exec the checkout's upstream script with the same argv.
    if std::env::var("PORTUALE_PYTHON_HELPERS").as_deref() == Ok("real") {
        return exec_real_chmod_lite(argv);
    }
    let mut failed = false;
    for arg in argv {
        if apply_recursive(Path::new(arg)) {
            failed = true;
        }
    }
    if failed { 1 } else { 0 }
}

/// D2 `real` mode: exec `<checkout>/bin/chmod-lite` with the same argv.
/// `portage_checkout()` reads only env and the build path, never
/// `bin_dir()`, so this is safe from the helper entry (D7).
fn exec_real_chmod_lite(argv: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt;
    let script = crate::ebuild_phases::portage_checkout().join("bin/chmod-lite");
    if !script.is_file() {
        eprintln!(
            "portuale: no native helper for: chmod-lite {}",
            argv.iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        );
        return 127;
    }
    let err = std::process::Command::new(&script).args(argv).exec();
    eprintln!("portuale: cannot exec {}: {err}", script.display());
    127
}

/// Port of `apply_recursive_permissions(top, uid=-1, gid=-1, ...)`: with
/// no owner/group request the secpass/chown half is a no-op and only the
/// mode arithmetic remains. Returns true when an unexpected error
/// occurred (real: an uncaught exception).
fn apply_recursive(top: &Path) -> bool {
    let stat = match std::fs::symlink_metadata(top) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Backward compatibility: a missing top is silently OK.
            return false;
        }
        Err(e) => {
            return report_stat_error(top, &e);
        }
    };
    if stat.file_type().is_symlink() {
        return false;
    }
    let (mode, mask) = if stat.is_dir() {
        (DIRMODE, DIRMASK)
    } else {
        (FILEMODE, FILEMASK)
    };
    if apply_one(top, stat.permissions().mode() & 0o7777, mode, mask) {
        return true;
    }
    stat.is_dir() && walk(top)
}

/// Depth-first walk of `dir` (already chmod'ed by the caller, bug
/// 554084): files get `filemode`/`filemask`, subdirectories
/// `dirmode`/`dirmask` before recursing. A directory that cannot be
/// listed is silently skipped (real `os.walk` ignores its errors), and
/// entries that disappear mid-walk are ignored (`InvalidLocation`).
/// Returns true when an unexpected error occurred.
fn walk(dir: &Path) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut failed = false;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let stat = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                // Real maps the stat errno: EPERM prints, anything else
                // (EACCES, EROFS, ...) propagates as a traceback.
                if e.raw_os_error() == Some(libc::EPERM) {
                    eprintln!(
                        "Operation Not Permitted: stat('{}')",
                        String::from_utf8_lossy(path.as_os_str().as_bytes())
                    );
                    continue;
                }
                eprintln!(
                    "portuale: chmod-lite: cannot stat '{}': {e}",
                    path.display()
                );
                failed = true;
                continue;
            }
        };
        if stat.file_type().is_symlink() {
            continue;
        }
        let (mode, mask) = if stat.is_dir() {
            (DIRMODE, DIRMASK)
        } else {
            (FILEMODE, FILEMASK)
        };
        if apply_one(&path, stat.permissions().mode() & 0o7777, mode, mask) {
            failed = true;
        }
        if stat.is_dir() && walk(&path) {
            failed = true;
        }
    }
    failed
}

/// Port of `apply_permissions`' mode half (uid/gid are always -1 here):
/// with `mask >= 0`, add the `mode` bits and clear the `mask` bits, but
/// only chmod when something would actually change. `st_mode` carries
/// the full `0o7777` bits so owner-exec and the special bits survive
/// (the mode only adds, the mask only clears). Returns true on an
/// unexpected chmod error.
fn apply_one(path: &Path, st_mode: u32, mode: u32, mask: u32) -> bool {
    let st_mode = st_mode & 0o7777;
    let mode = mode & 0o7777;
    let new_mode = if (mode & st_mode != mode) || ((mask ^ st_mode) & st_mode != st_mode) {
        let mut new_mode = mode | st_mode;
        new_mode = (mask ^ new_mode) & new_mode;
        Some(new_mode)
    } else {
        None
    };
    let Some(new_mode) = new_mode else {
        return false;
    };
    match std::fs::set_permissions(path, std::fs::Permissions::from_mode(new_mode)) {
        Ok(()) => false,
        Err(e) => {
            let display = String::from_utf8_lossy(path.as_os_str().as_bytes()).into_owned();
            match e.raw_os_error() {
                Some(code) if code == libc::EPERM => {
                    eprintln!("Operation Not Permitted: chmod('{display}', 0o{new_mode:o})");
                    false
                }
                Some(code) if code == libc::ENOENT => {
                    eprintln!("File Not Found: '{display}'");
                    false
                }
                _ => {
                    eprintln!("portuale: chmod-lite: cannot chmod '{display}': {e}");
                    true
                }
            }
        }
    }
}

/// Port of `_do_stat`'s error mapping for the top-level stat: EPERM
/// prints through `onerror`, anything else real would let propagate.
fn report_stat_error(top: &Path, e: &std::io::Error) -> bool {
    let display = String::from_utf8_lossy(top.as_os_str().as_bytes()).into_owned();
    match e.raw_os_error() {
        Some(code) if code == libc::EPERM => {
            eprintln!("Operation Not Permitted: stat('{display}')");
            false
        }
        Some(code) if code == libc::ENOENT => {
            eprintln!("File Not Found: '{display}'");
            false
        }
        _ => {
            eprintln!("portuale: chmod-lite: cannot stat '{display}': {e}");
            true
        }
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use portage_util::TempDir;
    use std::collections::BTreeMap;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    /// One manifest entry: type, mode (files/dirs only) or target (links).
    #[derive(Debug, PartialEq)]
    enum Entry {
        Dir(u32),
        File(u32),
        Link(Vec<u8>),
    }

    /// Decode the manifest `%XX` escapes to raw bytes (paths are
    /// `/`-separated; every byte outside `[A-Za-z0-9._/+-]` is escaped).
    fn decode(s: &str) -> Vec<u8> {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() + 1 {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        out
    }

    /// Parse a manifest file into path-bytes -> entry.
    fn parse(text: &str) -> BTreeMap<Vec<u8>, Entry> {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let (kind, rest) = line.split_at(1);
            let rest = &rest[1..];
            match kind {
                "l" => {
                    let rest = rest.strip_prefix("---- ").unwrap();
                    let (path, target) = rest.split_once(" -> ").unwrap();
                    map.insert(decode(path), Entry::Link(decode(target)));
                }
                "d" | "f" => {
                    let (mode, path) = rest.split_once(' ').unwrap();
                    let mode = u32::from_str_radix(mode, 8).unwrap();
                    let entry = if kind == "d" {
                        Entry::Dir(mode)
                    } else {
                        Entry::File(mode)
                    };
                    map.insert(decode(path), entry);
                }
                _ => panic!("bad manifest line: {line}"),
            }
        }
        map
    }

    fn join(root: &Path, rel: &[u8]) -> PathBuf {
        use std::ffi::OsStr;
        root.join(OsStr::from_bytes(rel))
    }

    fn eilseq_skip(e: &std::io::Error) -> bool {
        e.raw_os_error() == Some(libc::EILSEQ) || e.kind() == std::io::ErrorKind::InvalidInput
    }

    /// Build a tree from `in.manifest` (byte paths throughout; modes set
    /// deepest-first so `0000` dirs can still be populated). Returns
    /// false with a printed reason when the filesystem rejects a name
    /// (EILSEQ: this host's zfs datasets reject non-UTF-8 names; the
    /// case must be skipped, never silently passed).
    fn build_tree(root: &Path, entries: &BTreeMap<Vec<u8>, Entry>) -> bool {
        for (rel, entry) in entries {
            let path = join(root, rel);
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                if eilseq_skip(&e) {
                    println!(
                        "SKIP: filesystem rejects non-UTF-8 names ({e}); \
                         stage this case on tmpfs (see fixtures/helpers/README.md)"
                    );
                    return false;
                }
                panic!("create_dir_all {}: {e}", path.display());
            }
            let r: std::io::Result<()> = match entry {
                Entry::Dir(_) => std::fs::create_dir_all(&path).map(|_| ()),
                Entry::File(_) => std::fs::write(&path, b"").map(|_| ()),
                Entry::Link(target) => {
                    use std::ffi::OsStr;
                    std::os::unix::fs::symlink(OsStr::from_bytes(target), &path).map(|_| ())
                }
            };
            if let Err(e) = r {
                if eilseq_skip(&e) {
                    println!(
                        "SKIP: filesystem rejects non-UTF-8 names ({e}); \
                         stage this case on tmpfs (see fixtures/helpers/README.md)"
                    );
                    return false;
                }
                panic!("create {}: {e}", path.display());
            }
        }
        // Modes deepest-first (children before parents).
        let mut rels: Vec<_> = entries.keys().collect();
        rels.sort_by_key(|r| std::cmp::Reverse(r.len()));
        for rel in rels {
            match &entries[rel] {
                Entry::Dir(mode) | Entry::File(mode) => {
                    std::fs::set_permissions(
                        join(root, rel),
                        std::fs::Permissions::from_mode(*mode),
                    )
                    .unwrap();
                }
                Entry::Link(_) => {}
            }
        }
        true
    }

    /// The lstat manifest of a tree (same shape as the generator's).
    fn result_manifest(root: &Path) -> BTreeMap<Vec<u8>, Entry> {
        let mut map = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut names: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap())
                .collect();
            names.sort_by_key(|e| e.file_name());
            for e in names {
                let rel = e
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .as_os_str()
                    .as_bytes()
                    .to_vec();
                let ft = e.file_type().unwrap();
                if ft.is_symlink() {
                    let target = std::fs::read_link(e.path()).unwrap();
                    map.insert(rel, Entry::Link(target.as_os_str().as_bytes().to_vec()));
                } else if ft.is_dir() {
                    let mode = e.metadata().unwrap().permissions().mode() & 0o7777;
                    map.insert(rel.clone(), Entry::Dir(mode));
                    stack.push(e.path());
                } else {
                    let mode = e.metadata().unwrap().permissions().mode() & 0o7777;
                    map.insert(rel, Entry::File(mode));
                }
            }
        }
        map
    }

    /// #326 S2: every `fixtures/helpers/chmod-lite/<case>` oracle --
    /// rebuild the tree from `in.manifest`, run the native helper as a
    /// child process (cwd = tree root, the same top-level-argv shape as
    /// real's `find ... -exec`), and compare the resulting lstat manifest
    /// with `out.manifest` (or `out.root.manifest` as root), plus rc and
    /// stderr. Includes the non-UTF-8 case (byte paths throughout).
    #[test]
    fn chmod_lite_matches_the_portage_helper_on_every_fixture_case() {
        let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/chmod-lite");
        let as_root = unsafe { libc::geteuid() } == 0;
        let mut names: Vec<_> = std::fs::read_dir(&cases)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(!names.is_empty(), "no fixture cases in {}", cases.display());
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        for name in names {
            let dir = cases.join(&name);
            let input = parse(&std::fs::read_to_string(dir.join("in.manifest")).unwrap());
            let suffix = if as_root {
                ".root.manifest"
            } else {
                ".manifest"
            };
            let expected =
                parse(&std::fs::read_to_string(dir.join(format!("out{suffix}"))).unwrap());
            let rc_suffix = if as_root { ".root.txt" } else { ".txt" };
            let expected_rc: i32 = std::fs::read_to_string(dir.join(format!("rc{rc_suffix}")))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let expected_stderr =
                std::fs::read_to_string(dir.join(format!("stderr{rc_suffix}"))).unwrap();
            // HOST QUIRK (fixtures/helpers/README.md): zfs rejects
            // non-UTF-8 names with EILSEQ; stage under temp_dir (tmpfs
            // here) and skip with a printed reason when even that fails.
            let tmp = TempDir::new_in(&std::env::temp_dir(), "helper-chmod-lite");
            let root = tmp.join("tree");
            std::fs::create_dir_all(&root).unwrap();
            if !build_tree(&root, &input) {
                println!("case {name}: skipped (filesystem rejects non-UTF-8 names)");
                continue;
            }
            // Real's call shape (`phase-helpers.sh:511`): top-level
            // entries, symlinks excluded (`! -type l`).
            let mut tops: Vec<OsString> = std::fs::read_dir(&root)
                .unwrap()
                .map(|e| e.unwrap())
                .filter(|e| !e.file_type().unwrap().is_symlink())
                .map(|e| e.file_name())
                .collect();
            tops.sort();
            let out = std::process::Command::new(&exe)
                .args(["__helper", "chmod-lite"])
                .args(&tops)
                .current_dir(&root)
                .env_remove("PORTUALE_PYTHON_HELPERS")
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(expected_rc), "case {name}: rc");
            assert_eq!(
                String::from_utf8_lossy(&out.stderr),
                expected_stderr,
                "case {name}: stderr"
            );
            assert_eq!(result_manifest(&root), expected, "case {name}: modes");
            println!("case {name}: ok");
        }
    }

    /// The isolation proof's counterpart (see the S2 report): with the
    /// native helper this case normalises; the proof breaks the helper
    /// and watches this assertion fail. Kept as its own test so the
    /// failure names the behaviour, not the fixture loop.
    #[test]
    fn chmod_lite_normalises_a_0600_file_to_0644() {
        let tmp = TempDir::new("helper-chmod-lite-single");
        let root = tmp.join("tree");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file-0600");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        let out = std::process::Command::new(&exe)
            .args(["__helper", "chmod-lite", "file-0600"])
            .current_dir(&root)
            .env_remove("PORTUALE_PYTHON_HELPERS")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777,
            0o644
        );
    }
}
