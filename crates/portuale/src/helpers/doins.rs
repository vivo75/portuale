// Native `doins.py`: installs files and dirs into the image.
//
// A port of `main`, `_parse_args`, `_install_dir`, `_doins` and the
// install runners: outer argv (long options, `--x=v` and `--x v`,
// unique prefixes, `--` before sources), install options from
// `shlex.split` (`-g/-o/-m/-p`, unknown ones falling back to
// `install(1)` unless `--strict_option`), then per-source install with
// Portage's return rule (`0 if not any_failure and any_success else 1`).
// Every mutation is D7-guarded under the canonical `$D` (else `--dest`);
// uncaught failures print only the final `ExceptionType: message` line.
// Deviations: help text is fixed at 80 columns; option values decode
// lossy (paths stay raw bytes); copies are plain read/write.
// Portage: bin/doins.py, main

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------
// fnmatch (posix: `*` matches everything including `/`) and
// `_xattr_excluder` (whitespace-split globs; empty never excludes).
// ---------------------------------------------------------------------

fn fnmatch_inner(pat: &[u8], name: &[u8]) -> bool {
    if pat.is_empty() {
        return name.is_empty();
    }
    match pat[0] {
        b'*' => {
            // Collapse runs of `*`, then try every split.
            let mut p = 1;
            while p < pat.len() && pat[p] == b'*' {
                p += 1;
            }
            if p == pat.len() {
                return true;
            }
            (0..=name.len()).any(|i| fnmatch_inner(&pat[p..], &name[i..]))
        }
        b'?' => !name.is_empty() && fnmatch_inner(&pat[1..], &name[1..]),
        b'[' => {
            let Some(close) = find_class_close(&pat[1..]) else {
                // Unterminated `[` is a literal (`fnmatch.translate`).
                return !name.is_empty() && name[0] == b'[' && fnmatch_inner(&pat[1..], &name[1..]);
            };
            if name.is_empty() {
                return false;
            }
            // Only `!` negates; a leading `^` is a literal member
            // (`translate` escapes it).
            let (neg, body) = match pat.get(1) {
                Some(b'!') => (true, &pat[2..1 + close]),
                _ => (false, &pat[1..1 + close]),
            };
            let hit = class_matches(body, name[0]);
            hit != neg && fnmatch_inner(&pat[1 + close + 1..], &name[1..])
        }
        // A backslash is an ordinary character in `fnmatch` (probed:
        // `a\*b` matches `a\xb`, not `a*b`).
        c => !name.is_empty() && c == name[0] && fnmatch_inner(&pat[1..], &name[1..]),
    }
}

/// Index (relative to the byte after `[`) of the closing `]`, as
/// `fnmatch.translate` scans it: an optional `!`, then a `]` right
/// after is a literal member.
fn find_class_close(body_after: &[u8]) -> Option<usize> {
    let mut i = 0;
    if body_after.first() == Some(&b'!') {
        i = 1;
    }
    if body_after.get(i) == Some(&b']') {
        i += 1;
    }
    while i < body_after.len() {
        if body_after[i] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Members of a class body: single bytes and `lo-hi` ranges (a `-`
/// first or last is literal; an inverted range matches nothing, as
/// `translate` drops it). Backslashes are literal members.
fn class_matches(body: &[u8], c: u8) -> bool {
    let mut i = 0;
    while i < body.len() {
        if i + 2 < body.len() && body[i + 1] == b'-' {
            if body[i] <= c && c <= body[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if body[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

fn fnmatch_glob(pat: &[u8], name: &[u8]) -> bool {
    fnmatch_inner(pat, name)
}

/// `_xattr_excluder(pattern)`: `None`/empty never excludes.
/// Shared with `helpers/xattr.rs`.
pub(crate) fn xattr_excluded(pattern: &str, attr: &[u8]) -> bool {
    for pat in pattern.split_whitespace() {
        if fnmatch_glob(pat.as_bytes(), attr) {
            return true;
        }
    }
    false
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

// ---------------------------------------------------------------------
// xattr (`movefile._copyxattr` over `portage.util._xattr`, which follows
// symlinks).
// ---------------------------------------------------------------------

fn c_path(p: &[u8]) -> Result<std::ffi::CString, ()> {
    std::ffi::CString::new(p).map_err(|_| ())
}

/// `xattr.list(src)`: ENOTSUP/EOPNOTSUPP means "none" (`attrs = ()`).
/// Shared with `helpers/xattr.rs` (one copier, not two).
pub(crate) fn xattr_list(path: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
    let c = c_path(path).map_err(|_| libc::EINVAL)?;
    let n = unsafe { libc::listxattr(c.as_ptr(), std::ptr::null_mut(), 0) };
    if n < 0 {
        let e = last_errno();
        if e == libc::ENOTSUP || e == libc::EOPNOTSUPP {
            return Ok(Vec::new());
        }
        return Err(e);
    }
    let mut buf = vec![0u8; n as usize];
    let m = unsafe { libc::listxattr(c.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if m < 0 {
        let e = last_errno();
        if e == libc::ENOTSUP || e == libc::EOPNOTSUPP {
            return Ok(Vec::new());
        }
        return Err(e);
    }
    Ok(buf[..m as usize]
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_vec())
        .collect())
}

pub(crate) fn xattr_get(path: &[u8], name: &[u8]) -> Result<Vec<u8>, i32> {
    let c = c_path(path).map_err(|_| libc::EINVAL)?;
    let n = std::ffi::CString::new(name).map_err(|_| libc::EINVAL)?;
    let len = unsafe { libc::getxattr(c.as_ptr(), n.as_ptr(), std::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(last_errno());
    }
    let mut buf = vec![0u8; len as usize];
    let got = unsafe { libc::getxattr(c.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if got < 0 {
        return Err(last_errno());
    }
    buf.truncate(got as usize);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    // ---- end to end: a real package whose src_install uses doins ----

    /// `dev-libs/packagepkg`'s `src_install` is `insinto` + `doins`
    /// (in-process, through the existing package test machinery).
    fn install_packagepkg(portage_tmpdir: &Path) {
        let ebuild = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild");
        let status = crate::ebuild_phases::run_commands(crate::ebuild_phases::RunCommandsArgs {
            ebuild_path: &ebuild,
            commands: &["install"],
            root: Path::new("/"),
            portage_tmpdir,
            distdir: &portage_tmpdir.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell: crate::ebuild_phases::ShellBackend::default(),
            build_env: &[],
        })
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");
        let installed = portage_tmpdir
            .join("portage/dev-libs/packagepkg-1.0/image/usr/share/packagepkg/hello.txt");
        let contents = std::fs::read_to_string(&installed)
            .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
        assert_eq!(contents, "hello from packagepkg\n");
    }
}
