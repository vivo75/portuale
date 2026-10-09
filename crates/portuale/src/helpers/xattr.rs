// Native `xattr-helper.py` + `install.py`: xattr dump/restore and install.
//
// Ports of `bin/xattr-helper.py` (`quote`, `unquote`, `dump_xattrs`,
// `restore_xattrs`) and `bin/install.py` (`parse_args`, `copy_xattrs`,
// `Which`), routed from the `python` dispatcher. `--dump` prints the
// getfattr-style text; `--restore` reads it back; `install.py` runs the
// real `install(1)` verbatim, then copies xattrs through doins.rs's
// shared `copy_xattr`. Uncaught failures print only the final
// `ExceptionType: message` line.
// Deviations: help text is pinned at 80 columns; missing env, failed
// chdir or `Which` stat print the final line only (rc 1); a
// signal-killed `install(1)` reports rc 1. Deliberately NO D7 write
// guard: the caller-named paths ARE the operation, as with `install(1)`
// itself, so there is no anchor to check against.
// Portage: bin/xattr-helper.py, main; bin/install.py, main

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

use super::doins::{copy_xattr, errno_line, xattr_get, xattr_list, xattr_set};

// ---------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------

fn raw(arg: &OsString) -> &[u8] {
    arg.as_os_str().as_bytes()
}

fn stderr_write(data: &[u8]) {
    let _ = std::io::stderr().write_all(data);
}

fn stdout_write(data: &[u8]) {
    let _ = std::io::stdout().write_all(data);
}

/// The raw basename of an argv element (bytes after the last `/`).
fn argv_basename(arg: &OsString) -> &[u8] {
    let b = raw(arg);
    match b.iter().rposition(|c| *c == b'/') {
        Some(i) => &b[i + 1..],
        None => b,
    }
}

/// Read all of stdin as raw bytes.
fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    buf
}

// ---------------------------------------------------------------------
// Quoting (`quote` / `unquote` / `octal_quote_byte`)
// ---------------------------------------------------------------------

/// `\ooo` (three octal digits) for one byte.
fn octal_quote_byte(b: u8) -> [u8; 4] {
    [
        b'\\',
        b'0' + ((b >> 6) & 7),
        b'0' + ((b >> 3) & 7),
        b'0' + (b & 7),
    ]
}

fn is_quote_char(c: u8, chars: &[u8]) -> bool {
    chars.contains(&c)
}

/// `quote(s, quote_chars)`: every byte in `quote_chars` becomes its
/// octal escape; all other bytes pass through raw (0xff and UTF-8 are
/// NOT escaped — pinned by the `dump-escapes` oracle case).
fn quote(s: &[u8], chars: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &b in s {
        if is_quote_char(b, chars) {
            out.extend_from_slice(&octal_quote_byte(b));
        } else {
            out.push(b);
        }
    }
    out
}

fn is_octal(c: u8) -> bool {
    (b'0'..=b'7').contains(&c)
}

/// `unquote(s)`: `\\[0-7]{3}` becomes the byte; anything else
/// (a `\` with fewer/different followers included) stays literal.
fn unquote(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut pos = 0;
    while pos < s.len() {
        // Leftmost `\\` + 3 octal digits at or after `pos` (Portage's
        // `_UNQUOTE_RE.search(s, pos=pos)`).
        let mut at = None;
        let mut i = pos;
        while i < s.len() {
            if s[i] == b'\\'
                && s.get(i + 1).is_some_and(|c| is_octal(*c))
                && s.get(i + 2).is_some_and(|c| is_octal(*c))
                && s.get(i + 3).is_some_and(|c| is_octal(*c))
            {
                at = Some(i);
                break;
            }
            i += 1;
        }
        match at {
            None => {
                out.extend_from_slice(&s[pos..]);
                pos = s.len();
            }
            Some(start) => {
                out.extend_from_slice(&s[pos..start]);
                let v = ((s[start + 1] - b'0') << 6)
                    | ((s[start + 2] - b'0') << 3)
                    | (s[start + 3] - b'0');
                out.push(v);
                pos = start + 4;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// `xattr-helper.py` argv (argparse)
// ---------------------------------------------------------------------

// Probed from Portage `xattr-helper.py --help` (80 columns, piped).
// `xattr-helper.py` occurs in the usage line only.
const XATTR_USAGE: &str = "usage: xattr-helper.py [-h] [--dump] [--restore] [paths ...]\n";

const XATTR_HELP_BODY: &str = "\nDump and restore extended attributes. We use formats like that used by\ngetfattr --dump. This is meant for shell helpers to save/restore. If you're\nlooking for a python/portage API, see portage.util.movefile._copyxattr\ninstead. https://en.wikipedia.org/wiki/Extended_file_attributes\n\npositional arguments:\n  paths\n\noptions:\n  -h, --help  show this help message and exit\n\nActions:\n  --dump      Dump the values of all extended attributes associated with paths\n              passed as arguments or null-separated paths read from stdin.\n  --restore   Restore extended attributes using a dump read from stdin.\n";

fn xattr_usage(prog: &str) -> Vec<u8> {
    XATTR_USAGE
        .replacen("xattr-helper.py", prog, 1)
        .into_bytes()
}

fn xattr_help(prog: &str) -> Vec<u8> {
    [xattr_usage(prog), XATTR_HELP_BODY.as_bytes().to_vec()].concat()
}

enum XattrParse {
    Help,
    /// Already printed; exit with this code.
    Done(i32),
    Dump(Vec<Vec<u8>>),
    Restore,
}

/// Unique-prefix match (`--du` -> `--dump`); exact match wins.
fn match_long(name: &[u8], cands: &[&str]) -> Result<usize, Vec<usize>> {
    if let Some(i) = cands.iter().position(|c| c.as_bytes() == name) {
        return Ok(i);
    }
    let hits: Vec<usize> = cands
        .iter()
        .enumerate()
        .filter(|(_, c)| c.as_bytes().starts_with(name))
        .map(|(i, _)| i)
        .collect();
    if hits.len() == 1 {
        Ok(hits[0])
    } else {
        Err(hits)
    }
}

// ---------------------------------------------------------------------
// `dump_xattrs` / `restore_xattrs`
// ---------------------------------------------------------------------

/// Each file's section is written as produced, so a later failure leaves
/// partial stdout. `Err` is the final `ExceptionType: message` line (rc 1).
fn dump_xattrs(paths: &[Vec<u8>]) -> Result<(), Vec<u8>> {
    // NOTE: always quote backslashes (Portage's comment).
    let path_chars = b"\n\r\\";
    let mut name_chars = vec![b'='];
    name_chars.extend_from_slice(path_chars);
    let mut value_chars = vec![b'\0', b'"'];
    value_chars.extend_from_slice(path_chars);
    for path in paths {
        let attrs = xattr_list(path).map_err(|e| errno_line(e, path))?;
        if attrs.is_empty() {
            continue;
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"# file: ");
        out.extend_from_slice(&quote(path, path_chars));
        out.extend_from_slice(b"\n");
        for attr in &attrs {
            let value = xattr_get(path, attr).map_err(|e| errno_line(e, path))?;
            out.extend_from_slice(&quote(attr, &name_chars));
            out.extend_from_slice(b"=\"");
            out.extend_from_slice(&quote(&value, &value_chars));
            out.extend_from_slice(b"\"\n");
        }
        stdout_write(&out);
    }
    Ok(())
}

/// `restore_xattrs(file_in)`: `data` is the raw stdin bytes.
fn restore_xattrs(data: &[u8]) -> Result<(), Vec<u8>> {
    let mut pathname: Option<Vec<u8>> = None;
    // File iteration splits on `\n`; the trailing empty chunk is skipped
    // by the blank-line rule, so `split` is equivalent.
    for (i, line) in data.split(|b| *b == b'\n').enumerate() {
        if line.starts_with(b"# file: ") {
            // `rstrip(b"\n")` is a no-op on a chunk; `[8:]` drops the tag.
            pathname = Some(unquote(&line[8..]));
        } else {
            match line.iter().position(|b| *b == b'=') {
                Some(eq) => {
                    let Some(ref path) = pathname else {
                        return Err(
                            format!("ValueError: line {}: missing pathname", i + 1).into_bytes()
                        );
                    };
                    let attr = unquote(&line[..eq]);
                    // Strip the surrounding quotes (`[1:-1]`, saturating).
                    let inner = line.get(eq + 1..).unwrap_or(b"");
                    let inner = inner.get(1..).unwrap_or(b"");
                    let inner = inner.get(..inner.len().saturating_sub(1)).unwrap_or(b"");
                    let value = unquote(inner);
                    xattr_set(path, &attr, &value).map_err(|e| errno_line(e, path))?;
                }
                None => {
                    if !line.iter().all(|b| {
                        *b == b' '
                            || *b == b'\t'
                            || *b == b'\n'
                            || *b == b'\r'
                            || *b == 0x0b
                            || *b == 0x0c
                    }) {
                        return Err(
                            format!("ValueError: line {}: malformed entry", i + 1).into_bytes()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------
// `install.py`
// ---------------------------------------------------------------------

struct InstallOpts {
    directory: bool,
    target_directory: Option<Vec<u8>>,
}

/// `shlex.join(args)`: quote for the `copy_xattrs failed` note.
fn shlex_join(args: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        let safe = !a.is_empty()
            && a.iter()
                .all(|c| c.is_ascii_alphanumeric() || b"@%_+=:,./-".contains(c));
        if safe {
            out.extend_from_slice(a);
        } else {
            out.push(b'\'');
            for &c in a {
                if c == b'\'' {
                    out.extend_from_slice(b"'\\''");
                } else {
                    out.push(c);
                }
            }
            out.push(b'\'');
        }
    }
    out
}

/// `os.EX_OSERR` (Portage's `return os.EX_OSERR` on xattr-copy failure).
const EX_OSERR: i32 = 71;

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    // ---- fixture plumbing --------------------------------------------

    /// The `portuale` binary next to the test binary (child processes).
    /// Needs `cargo build --release -p portuale` first: `cargo test`
    /// does not rebuild it, and these tests spawn it.
    fn portuale_exe() -> PathBuf {
        crate::self_exe::built_portuale_bin()
    }

    /// pmtest's `fixtures/helpers/xattr/` (the oracle generated by
    /// running the Portage helpers; see its generator).
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/xattr")
    }

    /// `materialise.py`'s `%XX` unescaping (uppercase hex only).
    fn unesc(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let hex = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        };
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
            {
                out.push(h * 16 + l);
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        assert!(b.len().is_multiple_of(2), "odd hex: {s}");
        let v = |c: u8| match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("bad hex: {s}"),
        };
        b.chunks(2).map(|c| v(c[0]) * 16 + v(c[1])).collect()
    }

    /// `materialise.py` ported to Rust (so the test needs no python3).
    fn materialise(manifest: &Path, dest: &Path) {
        const MTIME: i64 = 1_700_000_000;
        std::fs::create_dir_all(dest).unwrap();
        let text = std::fs::read_to_string(manifest).unwrap();
        let mut made: Vec<PathBuf> = vec![dest.to_path_buf()];
        let mut entries: Vec<(u8, u32, PathBuf)> = Vec::new();
        let mut prev: Option<String> = None;
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let t: Vec<&str> = line.split(' ').collect();
            assert!(
                t.len() >= 3,
                "{}: too few fields: {line}",
                manifest.display()
            );
            let typ = t[0].as_bytes()[0];
            let mode = if t[1] == "----" {
                0
            } else {
                assert!(
                    t[1].len() == 4 && t[1].bytes().all(|c| c.is_ascii_digit()),
                    "{}: mode must be 4 octal digits: {line}",
                    manifest.display()
                );
                u32::from_str_radix(t[1], 8).unwrap()
            };
            let qpath = t[2];
            if let Some(p) = &prev {
                assert!(
                    p.as_str() < qpath,
                    "{}: lines must be sorted: {line}",
                    manifest.display()
                );
            }
            prev = Some(qpath.to_string());
            let extra = &t[3..];
            let (target, content) = match typ {
                b'd' => {
                    assert!(
                        extra.is_empty(),
                        "{}: unexpected fields: {line}",
                        manifest.display()
                    );
                    (None, None)
                }
                b'f' => {
                    if extra.is_empty() {
                        (None, None)
                    } else {
                        assert!(
                            extra[0].starts_with('|'),
                            "{}: only f takes |content: {line}",
                            manifest.display()
                        );
                        (None, Some(unesc(&extra.join(" ")[1..])))
                    }
                }
                b'l' => {
                    assert!(
                        t[1] == "----" && extra.len() == 2 && extra[0] == "->",
                        "{}: symlink needs '----' and '-> target': {line}",
                        manifest.display()
                    );
                    (Some(unesc(extra[1])), None)
                }
                _ => panic!("{}: bad type: {line}", manifest.display()),
            };
            let rel = unesc(qpath);
            let full = dest.join(OsStr::from_bytes(&rel));
            let parent = full.parent().unwrap().to_path_buf();
            assert!(
                made.contains(&parent),
                "{}: parent of {qpath:?} is not an explicit d entry",
                manifest.display()
            );
            match typ {
                b'd' => {
                    std::fs::create_dir(&full).unwrap();
                    made.push(full.clone());
                }
                b'f' => {
                    std::fs::write(&full, content.unwrap_or_default()).unwrap();
                }
                b'l' => {
                    std::os::unix::fs::symlink(OsStr::from_bytes(&target.unwrap()), &full).unwrap();
                }
                _ => unreachable!(),
            }
            entries.push((typ, mode, full));
        }
        entries.sort_by_key(|e| {
            std::cmp::Reverse(
                e.2.as_os_str()
                    .as_bytes()
                    .iter()
                    .filter(|b| **b == b'/')
                    .count(),
            )
        });
        for (typ, mode, path) in &entries {
            if *typ != b'l' {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode)).unwrap();
            }
            let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            let times = [
                libc::timespec {
                    tv_sec: MTIME as libc::time_t,
                    tv_nsec: 0,
                },
                libc::timespec {
                    tv_sec: MTIME as libc::time_t,
                    tv_nsec: 0,
                },
            ];
            let r = unsafe {
                libc::utimensat(
                    libc::AT_FDCWD,
                    c.as_ptr(),
                    times.as_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            assert_eq!(r, 0, "utime {}", path.display());
        }
    }

    /// Apply a case's `xattrs` (or `src.xattrs`) file to a tree.
    fn apply_xattrs_file(p: &Path, tree: &Path) {
        if !p.is_file() {
            return;
        }
        for line in std::fs::read_to_string(p).unwrap().lines() {
            if line.is_empty() {
                continue;
            }
            let t: Vec<&str> = line.split(' ').collect();
            assert_eq!(t.len(), 3, "{}: bad xattrs line: {line}", p.display());
            let path = tree.join(OsStr::from_bytes(&unesc(t[0])));
            let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            let n = std::ffi::CString::new(t[1]).unwrap();
            let v = hex_decode(t[2]);
            let r =
                unsafe { libc::setxattr(c.as_ptr(), n.as_ptr(), v.as_ptr().cast(), v.len(), 0) };
            assert_eq!(r, 0, "setxattr {} {}", path.display(), t[1]);
        }
    }

    /// Replace all occurrences of `from` with `to` in raw bytes.
    fn bytes_replace(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            if data[i..].starts_with(from) {
                out.extend_from_slice(to);
                i += from.len();
            } else {
                out.push(data[i]);
                i += 1;
            }
        }
        out
    }

    /// `%XX` escaping for dump paths (bytes outside
    /// `[A-Za-z0-9._/+-]`, uppercase hex): the generator's `quote`.
    fn qesc(b: &[u8]) -> String {
        b.iter()
            .map(|c| {
                if c.is_ascii_alphanumeric() || b"._/+-".contains(c) {
                    (*c as char).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect()
    }

    struct Staged {
        tmp: TempDir,
        top: PathBuf,
        args: Vec<OsString>,
        envs: Vec<(String, String)>,
        stdin: Vec<u8>,
    }

    /// `user.*` xattrs work on `dir` (tmpfs here)?
    fn xattrs_work(dir: &Path) -> bool {
        let probe = dir.join(".xattr-probe");
        if std::fs::write(&probe, b"p").is_err() {
            return false;
        }
        let c = std::ffi::CString::new(probe.as_os_str().as_bytes()).unwrap();
        let n = std::ffi::CString::new("user.probe").unwrap();
        let v = b"ok";
        let r = unsafe { libc::setxattr(c.as_ptr(), n.as_ptr(), v.as_ptr().cast(), v.len(), 0) };
        let _ = std::fs::remove_file(&probe);
        r == 0
    }

    /// Stage a case on `/dev/shm`. Returns None (after printing why)
    /// without tmpfs or without `user.*` xattr support.
    fn stage_case(case: &str) -> Option<Staged> {
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            eprintln!("skipped {case}: no /dev/shm tmpfs for the oracle");
            return None;
        }
        let case_dir = fixtures().join(case);
        let tmp = TempDir::new_in(shm, &format!("xattr-{case}"));
        if !xattrs_work(tmp.path()) {
            eprintln!(
                "skipped {case}: user.* xattrs unsupported on {}",
                tmp.path().display()
            );
            return None;
        }
        let top = tmp.path().join(format!("stage-{case}"));
        let is_install = case.starts_with("install-");
        let tree = if is_install {
            top.join("src")
        } else {
            top.join("in")
        };
        // restore-roundtrip's attributed sources.
        if case_dir.join("src.manifest").is_file() {
            let src = top.join("src");
            materialise(&case_dir.join("src.manifest"), &src);
            apply_xattrs_file(&case_dir.join("src.xattrs"), &src);
        }
        if case_dir.join("in.manifest").is_file() {
            materialise(&case_dir.join("in.manifest"), &tree);
            apply_xattrs_file(&case_dir.join("xattrs"), &tree);
        }
        std::fs::create_dir_all(top.join("dest")).ok();
        let scratch = tmp.path().as_os_str().as_bytes().to_vec();
        let sub_bytes = |v: &[u8]| {
            let mut out = Vec::new();
            let mut i = 0;
            while i < v.len() {
                if v[i..].starts_with(b"<SCRATCH>") {
                    out.extend_from_slice(&scratch);
                    i += 9;
                } else if v[i..].starts_with(b"<CHECKOUT>") {
                    // No checkout and no Python here; the placeholder needs an
                    // existing path only where the helper stats it.
                    out.extend_from_slice(&scratch);
                    i += 10;
                } else {
                    out.push(v[i]);
                    i += 1;
                }
            }
            out
        };
        let args: Vec<OsString> = std::fs::read(case_dir.join("args"))
            .unwrap()
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| OsString::from_vec(sub_bytes(l)))
            .collect();
        let mut envs = Vec::new();
        for line in std::fs::read_to_string(case_dir.join("env"))
            .unwrap()
            .lines()
        {
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').unwrap();
            if v.is_empty() {
                continue;
            }
            // `<SCRATCH>`/`<CHECKOUT>` as bytes (values are ASCII).
            let vv = sub_bytes(v.as_bytes());
            envs.push((k.to_string(), String::from_utf8(vv).unwrap()));
        }
        let stdin = if case_dir.join("stdin").is_file() {
            sub_bytes(&std::fs::read(case_dir.join("stdin")).unwrap())
        } else {
            Vec::new()
        };
        // `install.py` stats `__PORTAGE_HELPER_PATH`: materialise it so
        // `Which` reaches the `PATH` search, as in the oracle.
        for (k, v) in &envs {
            if k == "__PORTAGE_HELPER_PATH" {
                let p = Path::new(v);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, b"").unwrap();
            }
        }
        Some(Staged {
            tmp,
            top,
            args,
            envs,
            stdin,
        })
    }

    /// The `user.*` xattrs under a tree: `<qpath> <name> <hex>` lines,
    /// sorted (the oracle's `out.xattrs` format).
    fn dump_result_xattrs(tree: &Path) -> Vec<String> {
        let mut lines = Vec::new();
        fn walk(dir: &Path, tree: &Path, lines: &mut Vec<String>) {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            for n in names {
                let p = dir.join(&n);
                let md = std::fs::symlink_metadata(&p).unwrap();
                if md.is_dir() {
                    walk(&p, tree, lines);
                    continue;
                }
                let rel = p.strip_prefix(tree).unwrap();
                let q = qesc(rel.as_os_str().as_bytes());
                if let Ok(list) = xattr_list(p.as_os_str().as_bytes()) {
                    for name in list {
                        if !name.starts_with(b"user.") {
                            continue;
                        }
                        let v = xattr_get(p.as_os_str().as_bytes(), &name).unwrap();
                        lines.push(format!(
                            "{q} {} {}",
                            String::from_utf8_lossy(&name),
                            v.iter().map(|b| format!("{b:02x}")).collect::<String>()
                        ));
                    }
                }
            }
        }
        walk(tree, tree, &mut lines);
        lines.sort();
        lines
    }

    /// The D3 tree dump in `out.manifest`'s exact format (doins/README).
    fn dump_tree(dest: &Path, stage: &Path) -> Vec<String> {
        let mut lines: Vec<(String, String)> = Vec::new();
        fn walk(dir: &Path, dest: &Path, stage: &Path, lines: &mut Vec<(String, String)>) {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            for n in names {
                let p = dir.join(&n);
                let rel = p.strip_prefix(dest).unwrap();
                let q = qesc(rel.as_os_str().as_bytes());
                let md = std::fs::symlink_metadata(&p).unwrap();
                let mut xs: Vec<String> = Vec::new();
                if let Ok(list) = xattr_list(p.as_os_str().as_bytes()) {
                    for name in list {
                        if !name.starts_with(b"user.") {
                            continue;
                        }
                        let v = xattr_get(p.as_os_str().as_bytes(), &name).unwrap();
                        xs.push(format!(
                            "xattr:{}={}",
                            String::from_utf8_lossy(&name),
                            v.iter().map(|b| format!("{b:02x}")).collect::<String>()
                        ));
                    }
                }
                xs.sort();
                let x = if xs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", xs.join(" "))
                };
                use std::os::unix::fs::MetadataExt;
                if md.file_type().is_symlink() {
                    let target = std::fs::read_link(&p).unwrap().into_os_string().into_vec();
                    let scrubbed = qesc(&target).replace(stage.to_str().unwrap(), "<SCRATCH>");
                    lines.push((
                        q.clone(),
                        format!("l ---- {} {} - - {q} -> {scrubbed}{x}", md.uid(), md.gid()),
                    ));
                } else if md.is_dir() {
                    lines.push((
                        q.clone(),
                        format!(
                            "d {:04o} {} {} - - {q}{x}",
                            md.mode() & 0o7777,
                            md.uid(),
                            md.gid()
                        ),
                    ));
                    walk(&p, dest, stage, lines);
                } else if md.is_file() {
                    let data = std::fs::read(&p).unwrap();
                    use sha2::Digest;
                    let hex: String = sha2::Sha256::digest(&data)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    lines.push((
                        q.clone(),
                        format!(
                            "f {:04o} {} {} {} sha256:{hex} {q}{x}",
                            md.mode() & 0o7777,
                            md.uid(),
                            md.gid(),
                            data.len()
                        ),
                    ));
                } else {
                    panic!("unexpected file type: {}", p.display());
                }
            }
        }
        if dest.is_dir() {
            walk(dest, dest, stage, &mut lines);
        }
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        // Scrub the stage (the oracle shows `<SCRATCH>`); uids stay
        // real here and are rewritten on the want side, as in doins.
        let stage_str = stage.to_str().unwrap();
        lines
            .into_iter()
            .map(|(_, l)| l.replace(stage_str, "<SCRATCH>"))
            .collect()
    }

    /// Run one case against the oracle: rc, stdout, stderr lines, and
    /// the resulting xattrs or tree. False when skipped.
    fn check_oracle(case: &str, script: &'static str) -> bool {
        let Some(st) = stage_case(case) else {
            return false;
        };
        let case_dir = fixtures().join(case);
        let mut cmd = Command::new(portuale_exe());
        cmd.args(["__helper", "python", script])
            .args(&st.args)
            .current_dir(st.tmp.path())
            .env_clear()
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .env("PATH", "/usr/bin:/bin");
        for (k, v) in &st.envs {
            cmd.env(k, v);
        }
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::umask(0o022);
                Ok(())
            });
        }
        use std::io::Write;
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("portuale __helper spawns");
        child.stdin.take().unwrap().write_all(&st.stdin).unwrap();
        let out = child.wait_with_output().expect("wait");
        let rc = out.status.code().unwrap_or(-1);
        let want_rc: i32 = std::fs::read_to_string(case_dir.join("rc.txt"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(rc, want_rc, "{case}: rc, stderr:\n{stderr}");
        // stdout bytes, exact (the oracle's `<SCRATCH>` substituted).
        let stage_str = st.tmp.path().to_str().unwrap();
        let want_out = if case_dir.join("out.txt").is_file() {
            std::fs::read(case_dir.join("out.txt")).unwrap()
        } else {
            Vec::new()
        };
        let want_out = bytes_replace(&want_out, b"<SCRATCH>", stage_str.as_bytes());
        assert_eq!(out.stdout, want_out, "{case}: stdout bytes differ");
        // stderr: every significant oracle line appears in order (only
        // the final `ExceptionType: message` line is pinned).
        let want_err = std::fs::read_to_string(case_dir.join("stderr.txt"))
            .unwrap()
            .replace("<SCRATCH>", stage_str);
        let got: Vec<&str> = stderr.lines().collect();
        let want_sig: Vec<&str> = want_err
            .lines()
            .filter(|l| {
                !l.is_empty()
                    && !l.starts_with(' ')
                    && !l.starts_with(" *")
                    && *l != "Traceback (most recent call last):"
            })
            .collect();
        let mut gi = 0;
        for w in &want_sig {
            match got[gi..].iter().position(|g| g == w) {
                Some(p) => gi += p + 1,
                None => panic!("{case}: stderr lacks {w:?}\n got:\n{stderr}"),
            }
        }
        // Resulting xattrs (restore) or tree (install).
        if case_dir.join("out.xattrs").is_file() {
            let tree = st.top.join("in");
            let mut got_x = dump_result_xattrs(&tree);
            got_x.sort();
            let mut want_x: Vec<String> = std::fs::read_to_string(case_dir.join("out.xattrs"))
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            want_x.sort();
            assert_eq!(got_x, want_x, "{case}: resulting xattrs differ");
            // Contents are untouched by a restore.
            for name in ["fresh.txt", "fresh2.txt"] {
                let data = std::fs::read(tree.join("top").join(name)).unwrap();
                assert!(
                    data == b"fresh-one" || data == b"fresh-two",
                    "{case}: restore rewrote file contents"
                );
            }
        }
        if case_dir.join("out.manifest").is_file() || script == "install.py" {
            let euid = unsafe { libc::geteuid() };
            let egid = unsafe { libc::getegid() };
            let dest = st.top.join("dest");
            let got_tree = dump_tree(&dest, st.tmp.path());
            // install-missing leaves an empty dest: the oracle's
            // out.manifest is empty too.
            let want_tree: Vec<String> = if case_dir.join("out.manifest").is_file() {
                std::fs::read_to_string(case_dir.join("out.manifest"))
                    .unwrap()
                    .lines()
                    .map(|l| {
                        let mut t: Vec<String> = l.split(' ').map(str::to_string).collect();
                        if matches!(t.first().map(String::as_str), Some("f" | "d" | "l"))
                            && t.len() > 3
                        {
                            t[2] = euid.to_string();
                            t[3] = egid.to_string();
                            t.join(" ")
                        } else {
                            l.to_string()
                        }
                    })
                    .filter(|l| !l.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            assert_eq!(got_tree, want_tree, "{case}: dest tree differs");
        }
        eprintln!("oracle {case}: ok");
        true
    }

    macro_rules! oracle_case {
        ($name:ident, $case:literal, $script:literal) => {
            #[test]
            fn $name() {
                check_oracle($case, $script);
            }
        };
    }

    oracle_case!(oracle_dump_empty, "dump-empty", "xattr-helper.py");
    oracle_case!(oracle_dump_two, "dump-two", "xattr-helper.py");
    oracle_case!(oracle_dump_escapes, "dump-escapes", "xattr-helper.py");
    oracle_case!(oracle_dump_names, "dump-names", "xattr-helper.py");
    oracle_case!(oracle_dump_symlink, "dump-symlink", "xattr-helper.py");
    oracle_case!(oracle_dump_argv, "dump-argv", "xattr-helper.py");
    oracle_case!(oracle_dump_missing, "dump-missing", "xattr-helper.py");
    oracle_case!(
        oracle_dump_newline_joined,
        "dump-newline-joined",
        "xattr-helper.py"
    );
    oracle_case!(
        oracle_restore_roundtrip,
        "restore-roundtrip",
        "xattr-helper.py"
    );
    oracle_case!(
        oracle_restore_malformed,
        "restore-malformed",
        "xattr-helper.py"
    );
    oracle_case!(
        oracle_restore_no_pathname,
        "restore-no-pathname",
        "xattr-helper.py"
    );
    oracle_case!(
        oracle_restore_missing_file,
        "restore-missing-file",
        "xattr-helper.py"
    );
    oracle_case!(oracle_argv_no_action, "argv-no-action", "xattr-helper.py");
    oracle_case!(oracle_argv_both, "argv-both", "xattr-helper.py");
    oracle_case!(oracle_argv_help, "argv-help", "xattr-helper.py");
    oracle_case!(oracle_install_file, "install-file", "install.py");
    oracle_case!(oracle_install_d_caps, "install-D", "install.py");
    oracle_case!(oracle_install_d, "install-d", "install.py");
    oracle_case!(oracle_install_t, "install-t", "install.py");
    oracle_case!(oracle_install_multi, "install-multi", "install.py");
    oracle_case!(oracle_install_missing, "install-missing", "install.py");
}
