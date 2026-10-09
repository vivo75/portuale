// Native `doins.py` (#326 S6, D1/D3/D4/D7): a port of
// `3rdparty/portage/bin/doins.py` (all 619 lines: `main`, `_parse_args`,
// `_install_dir`, `_doins`, `_InstallRunner`, the two in-process runners,
// the two subprocess runners, `_parse_install_options` and
// `_set_attributes`/`_set_timestamps`), routed from the `python`
// dispatcher when the script argument's basename is `doins.py`.
//
// Behaviour is real's, in real's order:
// - Outer argv (`_create_arg_parser`): the long options as real spells
//   them, both `--x=v` and `--x v`, unique long-option prefixes, and
//   `--` before the sources. `-h`/`--help` (and the single-dash `-h*`
//   cluster, as argparse parses it) prints the help to stdout and exits
//   0, winning over any other error. Any other argparse error exits 2
//   with real's usage text (probed live, pinned in tests). `--distdir`
//   becomes its value + `/` (an empty one is `/`, so every absolute
//   link target counts as inside it).
// - Install options (`_parse_install_options`): `-g/--group`,
//   `-o/--owner`, `-m/--mode` (default 0o755) and `-p`, parsed from
//   `shlex.split(options)` with `parse_known_args` semantics. `-m` uses
//   `int(x, 8)` (whitespace, sign, `0o` prefix and `_` separators
//   accepted; failure gives `mode=None`). Owner/group resolve a name
//   first (from `/etc/passwd` and `/etc/group` by parsing the files:
//   no NSS in a static binary), then `int()`; an `int()` failure is an
//   argparse type error (exit 2). Remaining args, or `mode is None`:
//   the two `!!! <helper>:` warnings, then `--strict_option` exits 1,
//   otherwise fall back to `install(1)` from PATH.
// - File install: `stat` (following links; failure is uncaught, rc 1),
//   `_is_install_allowed` with its same-file warning, unlink (ENOENT is
//   fine; anything else is uncaught), copy the contents into a newly
//   created file (`0o666 & ~umask`), then lchown, chmod, the xattr copy,
//   then `-p` utime (ns). Any failure in that block is logged and
//   returns False; the file stays behind.
// - xattr (`--enable_copy_xattr`): list (ENOTSUP means none), skip
//   `--xattr_exclude` whitespace-split globs (fnmatch semantics), set
//   the rest; a set failure becomes real's `OperationNotSupported`
//   message. The following variants (`listxattr`/`getxattr`/`setxattr`)
//   are used, as real's `portage.util._xattr` (which calls
//   `os.listxattr`/`getxattr`/`setxattr` with `follow_symlinks=True`)
//   does.
// - Dirs: `makedirs`, so created parents get `0o777 & ~umask` and only
//   the leaf gets `_set_attributes`. EEXIST on an existing dir is fine.
//   `install_dir` re-raises only with `--helpers_can_die`, otherwise it
//   logs `install_dir failed.`.
// - `_doins`: the symlink branch preserves the link (unlink the dest,
//   rmtree if it is a dir) when `--preserve_symlinks` is set and its
//   target does not start with distdir; otherwise the file is
//   installed. `_install_dir` walks without following symlinks: a
//   symlinked dir is queued for `_doins`, a real dir gets an
//   `install_dir`. A dir without `-r` is skipped (None); for `dodoc` it
//   is a warning plus failure.
// - `main`: create `--dest` if it is not a dir, loop over the sources,
//   return `0 if not any_failure and any_success else 1`.
// - D7 write guard: before every create, unlink, mkdir, symlink, chown,
//   chmod, xattr-set or utime, the canonical parent (for mutations that
//   follow links: the canonical path itself) must lie under the
//   canonical `$D` (when `D` is set in the env), else under the
//   canonical `--dest`. A violation prints an error naming the path and
//   exits 1.
// - Python tracebacks are not reproduced. Where real logs an exception
//   (`logger.exception(...)`), the first line (the message) and the
//   final `ExceptionType: message` line are printed. Uncaught
//   exceptions print only the final line.
//
// Deviations from real, all documented where they occur:
// - Usage/help text is rendered at a fixed width of 80 columns: real's
//   argparse re-wraps when `COLUMNS` is set or stdout is a wide tty.
// - Non-UTF-8 option values are decoded lossy for parsing (paths stay
//   raw bytes throughout).
// - `int()`-style owner/group values beyond `i64` are type errors
//   rather than unbounded Python ints.
// - `copyfile` uses a plain read/write copy (no reflink/sparse
//   acceleration): contents are identical, which is what D3 compares.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

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

// ---------------------------------------------------------------------
// Python repr helpers (warning/exception lines quote bytes the way
// Python's `repr()` does, so the pinned lines match real's byte for
// byte on ASCII paths).
// ---------------------------------------------------------------------

/// Python `repr()` of a byte string: `b'...'` (double quotes when the
/// value contains a single quote and no double quote).
fn py_bytes_repr(b: &[u8]) -> Vec<u8> {
    let mut out = vec![b'b'];
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    out.push(quote);
    for &c in b {
        match c {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            c if c == quote => {
                out.push(b'\\');
                out.push(c);
            }
            0x20..=0x7e => out.push(c),
            c => out.extend_from_slice(format!("\\x{c:02x}").as_bytes()),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a (lossy-decoded) `str` value.
fn py_str_repr(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    out.push(quote);
    for c in s.chars() {
        match c {
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\\' => out.extend_from_slice(b"\\\\"),
            c if c == quote as char => {
                out.push(b'\\');
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
            _ => {
                let mut buf = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a list of `str`: `['-v']`.
fn py_str_list_repr(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![b'['];
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b", ");
        }
        out.extend_from_slice(&py_str_repr(&String::from_utf8_lossy(item)));
    }
    out.push(b']');
    out
}

/// Python `repr()` of a list of byte strings: `[b'a', b'b']`.
fn py_bytes_list_repr(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![b'['];
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b", ");
        }
        out.extend_from_slice(&py_bytes_repr(item));
    }
    out.push(b']');
    out
}

// ---------------------------------------------------------------------
// Help/usage text (probed live from real `doins.py`, Portage 3.0.82.2,
// pipe width 80; see the module docs for the fixed-width deviation).
// `doins.py` occurs exactly once in each and is the program name.
// ---------------------------------------------------------------------

const OUTER_USAGE: &str = "usage: doins.py [-h] [--recursive] [--preserve_symlinks] [--helpers_can_die]\n                [--distdir DISTDIR] [--insoptions INSOPTIONS]\n                [--diroptions DIROPTIONS] [--strict_option]\n                [--enable_copy_xattr] [--xattr_exclude XATTR_EXCLUDE]\n                [--helper HELPER] [--dest DEST]\n                [sources ...]\n";

const OUTER_HELP: &str = "usage: doins.py [-h] [--recursive] [--preserve_symlinks] [--helpers_can_die]\n                [--distdir DISTDIR] [--insoptions INSOPTIONS]\n                [--diroptions DIROPTIONS] [--strict_option]\n                [--enable_copy_xattr] [--xattr_exclude XATTR_EXCLUDE]\n                [--helper HELPER] [--dest DEST]\n                [sources ...]\n\npositional arguments:\n  sources               Source file/directory paths to be installed.\n\noptions:\n  -h, --help            show this help message and exit\n  --recursive           If set, installs files recursively. Otherwise, just\n                        skips directories.\n  --preserve_symlinks   If set, a symlink will be installed as symlink.\n  --helpers_can_die     If set, die in isolated-functions.sh is enabled.\n                        Specifically this is used to keep compatible dodir's\n                        behavior.\n  --distdir DISTDIR     Path to the actual distdir.\n  --insoptions INSOPTIONS\n                        Options passed to `install` command for installing a\n                        file.\n  --diroptions DIROPTIONS\n                        Options passed to `install` command for installing a\n                        dir.\n  --strict_option       If set True, abort if insoptions/diroptions contains\n                        an option which cannot be interpreted by this script,\n                        instead of fallback to execute `install` command.\n  --enable_copy_xattr   Copies xattrs, if set True\n  --xattr_exclude XATTR_EXCLUDE\n                        White space delimited glob pattern to exclude xattr\n                        copy.Used only if --enable_xattr_copy is set.\n  --helper HELPER       Name of helper.\n  --dest DEST           Destination where the files are installed.\n";

const INSTALL_USAGE: &str = "usage: doins.py [-h] [-g GROUP] [-o OWNER] [-m MODE] [-p]\n";

const INSTALL_HELP: &str = "usage: doins.py [-h] [-g GROUP] [-o OWNER] [-m MODE] [-p]\n\noptions:\n  -h, --help            show this help message and exit\n  -g, --group GROUP\n  -o, --owner OWNER\n  -m, --mode MODE\n  -p, --preserve-timestamps\n";

fn outer_usage(prog: &str) -> Vec<u8> {
    OUTER_USAGE.replacen("doins.py", prog, 1).into_bytes()
}

fn outer_help(prog: &str) -> Vec<u8> {
    OUTER_HELP.replacen("doins.py", prog, 1).into_bytes()
}

fn install_usage(prog: &str) -> Vec<u8> {
    INSTALL_USAGE.replacen("doins.py", prog, 1).into_bytes()
}

fn install_help(prog: &str) -> Vec<u8> {
    INSTALL_HELP.replacen("doins.py", prog, 1).into_bytes()
}

// ---------------------------------------------------------------------
// `_warn`: `print(f"!!! {helper}: {msg}\n", file=sys.stderr)` — the
// `print` adds a second newline, so every warning is followed by a
// blank line.
// ---------------------------------------------------------------------

fn warn(helper: &str, msg: &[u8]) {
    let mut out = Vec::from(format!("!!! {helper}: ").as_bytes());
    out.extend_from_slice(msg);
    out.extend_from_slice(b"\n\n");
    stderr_write(&out);
}

// ---------------------------------------------------------------------
// `shlex.split` (posix mode, `whitespace_split`, no comments; ASCII
// syntax, arbitrary bytes pass through). Probed against real
// `shlex.split`: inside `"..."` a backslash escapes only `"` and `\`
// (`"06\44"` keeps the backslash); outside quotes it escapes any byte,
// a newline included (`a\<newline>b` is `a<newline>b`, no line
// continuation); `''`/`""` give an empty word. An unterminated quote
// or a trailing backslash is real's uncaught `ValueError`.
// ---------------------------------------------------------------------

fn shlex_split(s: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    let mut words: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut in_word = false;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\'' || c == b'"' {
            in_word = true;
            let q = c;
            i += 1;
            loop {
                let Some(&d) = s.get(i) else {
                    return Err("No closing quotation");
                };
                if d == q {
                    i += 1;
                    break;
                }
                if q == b'"' && d == b'\\' {
                    let Some(&e) = s.get(i + 1) else {
                        return Err("No escaped character");
                    };
                    if e != b'"' && e != b'\\' {
                        cur.push(b'\\');
                    }
                    cur.push(e);
                    i += 2;
                } else {
                    cur.push(d);
                    i += 1;
                }
            }
        } else if c == b'\\' {
            in_word = true;
            let Some(&e) = s.get(i + 1) else {
                return Err("No escaped character");
            };
            cur.push(e);
            i += 2;
        } else if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            if in_word {
                words.push(std::mem::take(&mut cur));
                in_word = false;
            }
            i += 1;
        } else {
            in_word = true;
            cur.push(c);
            i += 1;
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

// ---------------------------------------------------------------------
// Python `int(x, base)`: surrounding ASCII whitespace, an optional
// sign, a matching `0o`/`0O` prefix for base 8, and single `_`
// separators between digits. Anything else fails.
// ---------------------------------------------------------------------

fn py_int(s: &str, base: u32) -> Option<i64> {
    let t = s.trim_matches(|c: char| {
        c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\x0b' || c == '\x0c'
    });
    let (neg, digits) = match t.strip_prefix('+') {
        Some(rest) => (false, rest),
        None => match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t),
        },
    };
    let digits = if base == 8 {
        digits
            .strip_prefix("0o")
            .or_else(|| digits.strip_prefix("0O"))
            .unwrap_or(digits)
    } else {
        digits
    };
    if digits.is_empty() {
        return None;
    }
    let valid = |c: char| c.is_digit(base);
    let parts: Vec<&str> = digits.split('_').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    if !parts.iter().all(|p| p.chars().all(valid)) {
        return None;
    }
    let joined: String = parts.concat();
    let mut value: i64 = 0;
    for c in joined.chars() {
        value = value
            .checked_mul(i64::from(base))?
            .checked_add(i64::from(c.to_digit(base)?))?;
    }
    Some(if neg { -value } else { value })
}

// ---------------------------------------------------------------------
// Owner/group names: `/etc/passwd` and `/etc/group` parsed by hand (no
// NSS in a static binary). A name first, then `int()`.
// ---------------------------------------------------------------------

/// Look up `name` in `data` (the bytes of a passwd/group file):
/// field 0 is the name, field 2 the id. The first match wins.
fn parse_id_file(data: &[u8], name: &str) -> Option<u32> {
    for line in data.split(|b| *b == b'\n') {
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let mut fields = line.split(|b| *b == b':');
        let (Some(n), Some(id)) = (fields.next(), fields.nth(1)) else {
            continue;
        };
        if n != name.as_bytes() {
            continue;
        }
        // The id field must be a plain decimal number.
        let text = std::str::from_utf8(id).ok()?;
        if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        return text.parse().ok();
    }
    None
}

fn lookup_uid(name: &str) -> Option<i64> {
    let data = std::fs::read("/etc/passwd").ok()?;
    parse_id_file(&data, name).map(i64::from)
}

fn lookup_gid(name: &str) -> Option<i64> {
    let data = std::fs::read("/etc/group").ok()?;
    parse_id_file(&data, name).map(i64::from)
}

/// `_parse_user` / `_parse_group`: a name first, then `int()`.
/// Returns `None` when both fail (the caller's argparse type error).
fn parse_user(s: &str) -> Option<i64> {
    if let Some(id) = lookup_uid(s) {
        return Some(id);
    }
    py_int(s, 10)
}

fn parse_group(s: &str) -> Option<i64> {
    if let Some(id) = lookup_gid(s) {
        return Some(id);
    }
    py_int(s, 10)
}

/// `_parse_mode`: `int(mode, 8)`, `None` on failure.
fn parse_mode(s: &str) -> Option<i64> {
    py_int(s, 8)
}

// ---------------------------------------------------------------------
// fnmatch (posix: `*` matches everything including `/`; case is
// significant) and `_xattr_excluder` (whitespace-split globs, any
// match excludes; empty pattern never excludes).
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
/// Shared with `helpers/xattr.rs` (#326 S7).
pub(super) fn xattr_excluded(pattern: &str, attr: &[u8]) -> bool {
    for pat in pattern.split_whitespace() {
        if fnmatch_glob(pat.as_bytes(), attr) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------
// Outer argv (`_create_arg_parser` + `_parse_args`).
// ---------------------------------------------------------------------

struct OuterOpts {
    recursive: bool,
    preserve_symlinks: bool,
    helpers_can_die: bool,
    /// `fsencode(value) + b"/"` — empty stays `/`.
    distdir: Vec<u8>,
    insoptions: Vec<u8>,
    diroptions: Vec<u8>,
    strict_option: bool,
    enable_copy_xattr: bool,
    xattr_exclude: Vec<u8>,
    helper: Option<Vec<u8>>,
    dest: Option<Vec<u8>>,
    sources: Vec<Vec<u8>>,
}

fn helper_display(helper: &Option<Vec<u8>>) -> String {
    match helper {
        Some(h) => String::from_utf8_lossy(h).into_owned(),
        None => "None".to_string(),
    }
}

/// Python `Namespace(...)` repr of the outer options, for the
/// `Failed to create symlink` first line (field order = real's
/// `add_argument` order; bytes values use `b'...'`).
fn outer_namespace_repr(o: &OuterOpts) -> Vec<u8> {
    let b = |v: &[u8]| py_bytes_repr(v);
    let s = |v: &[u8]| py_str_repr(&String::from_utf8_lossy(v));
    let mut out = Vec::from(b"Namespace(recursive=".as_slice());
    out.extend_from_slice(if o.recursive { b"True" } else { b"False" });
    out.extend_from_slice(b", preserve_symlinks=");
    out.extend_from_slice(if o.preserve_symlinks {
        b"True"
    } else {
        b"False"
    });
    out.extend_from_slice(b", helpers_can_die=");
    out.extend_from_slice(if o.helpers_can_die { b"True" } else { b"False" });
    out.extend_from_slice(b", distdir=");
    out.extend_from_slice(&b(&o.distdir));
    out.extend_from_slice(b", insoptions=");
    out.extend_from_slice(&s(&o.insoptions));
    out.extend_from_slice(b", diroptions=");
    out.extend_from_slice(&s(&o.diroptions));
    out.extend_from_slice(b", strict_option=");
    out.extend_from_slice(if o.strict_option { b"True" } else { b"False" });
    out.extend_from_slice(b", enable_copy_xattr=");
    out.extend_from_slice(if o.enable_copy_xattr {
        b"True"
    } else {
        b"False"
    });
    out.extend_from_slice(b", xattr_exclude=");
    out.extend_from_slice(&s(&o.xattr_exclude));
    out.extend_from_slice(b", helper=");
    match &o.helper {
        Some(h) => out.extend_from_slice(&s(h)),
        None => out.extend_from_slice(b"None"),
    }
    out.extend_from_slice(b", dest=");
    match &o.dest {
        Some(d) => out.extend_from_slice(&b(d)),
        None => out.extend_from_slice(b"None"),
    }
    out.extend_from_slice(b", sources=");
    out.extend_from_slice(&py_bytes_list_repr(&o.sources));
    out.push(b')');
    out
}

enum OuterParse {
    Help,
    /// Already printed; exit with this code.
    Done(i32),
    Ok(OuterOpts),
}

/// Unique-prefix match of `name` against `cands` (exact match wins).
/// `Err(vec![])` = no match; otherwise the ambiguous candidates.
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

fn parse_outer(prog: &str, argv: &[OsString]) -> OuterParse {
    // Long options as real spells them, in `add_argument` order (the
    // order ambiguous-option errors list).
    const LONGS: &[&str] = &[
        "recursive",
        "preserve_symlinks",
        "helpers_can_die",
        "distdir",
        "insoptions",
        "diroptions",
        "strict_option",
        "enable_copy_xattr",
        "xattr_exclude",
        "helper",
        "dest",
        "help",
    ];
    // Which take a value (`help` is handled separately).
    const TAKES_VALUE: &[bool] = &[
        false, false, false, true, true, true, false, false, true, true, true, false,
    ];
    let fail = |msg: Vec<u8>| -> OuterParse {
        let mut out = outer_usage(prog);
        out.extend_from_slice(prog.as_bytes());
        out.extend_from_slice(b": error: ");
        out.extend_from_slice(&msg);
        out.push(b'\n');
        stderr_write(&out);
        OuterParse::Done(2)
    };
    let mut o = OuterOpts {
        recursive: false,
        preserve_symlinks: false,
        helpers_can_die: false,
        distdir: Vec::new(),
        insoptions: Vec::new(),
        diroptions: Vec::new(),
        strict_option: false,
        enable_copy_xattr: false,
        xattr_exclude: Vec::new(),
        helper: None,
        dest: None,
        sources: Vec::new(),
    };
    let mut unrecognized: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    let mut end_opts = false;
    while i < argv.len() {
        let tok = raw(&argv[i]);
        if end_opts {
            o.sources.push(tok.to_vec());
            i += 1;
            continue;
        }
        if tok == b"--" {
            end_opts = true;
            i += 1;
            continue;
        }
        if tok.len() > 1 && tok.starts_with(b"-") && tok != b"-" {
            if tok.starts_with(b"--") {
                // Long option, maybe `--name=value`.
                let body = &tok[2..];
                let (name, inline) = match body.iter().position(|c| *c == b'=') {
                    Some(p) => (&body[..p], Some(&body[p + 1..])),
                    None => (body, None),
                };
                match match_long(name, LONGS) {
                    Ok(idx) if LONGS[idx] == "help" => {
                        if let Some(v) = inline {
                            let mut m = Vec::from(
                                b"argument -h/--help: ignored explicit argument ".as_slice(),
                            );
                            m.extend_from_slice(&py_str_repr(&String::from_utf8_lossy(v)));
                            return fail(m);
                        }
                        stdout_write(&outer_help(prog));
                        return OuterParse::Help;
                    }
                    Ok(idx) => {
                        if TAKES_VALUE[idx] {
                            if let Some(v) = inline {
                                match idx {
                                    3 => o.distdir = v.to_vec(),
                                    4 => o.insoptions = v.to_vec(),
                                    5 => o.diroptions = v.to_vec(),
                                    8 => o.xattr_exclude = v.to_vec(),
                                    9 => o.helper = Some(v.to_vec()),
                                    10 => o.dest = Some(v.to_vec()),
                                    _ => {}
                                }
                            } else if i + 1 < argv.len() {
                                i += 1;
                                let v = raw(&argv[i]);
                                match idx {
                                    3 => o.distdir = v.to_vec(),
                                    4 => o.insoptions = v.to_vec(),
                                    5 => o.diroptions = v.to_vec(),
                                    8 => o.xattr_exclude = v.to_vec(),
                                    9 => o.helper = Some(v.to_vec()),
                                    10 => o.dest = Some(v.to_vec()),
                                    _ => {}
                                }
                            } else {
                                let m = format!("argument --{}: expected one argument", LONGS[idx]);
                                return fail(m.into_bytes());
                            }
                        } else if let Some(v) = inline {
                            let r = py_str_repr(&String::from_utf8_lossy(v));
                            let m = format!(
                                "argument --{}: ignored explicit argument {}",
                                LONGS[idx],
                                String::from_utf8_lossy(&r)
                            );
                            return fail(m.into_bytes());
                        } else {
                            match idx {
                                0 => o.recursive = true,
                                1 => o.preserve_symlinks = true,
                                2 => o.helpers_can_die = true,
                                6 => o.strict_option = true,
                                7 => o.enable_copy_xattr = true,
                                _ => {}
                            }
                        }
                    }
                    Err(hits) if hits.is_empty() => {
                        unrecognized.push(tok.to_vec());
                    }
                    Err(hits) => {
                        let list: Vec<String> =
                            hits.iter().map(|h| format!("--{}", LONGS[*h])).collect();
                        let m = format!(
                            "ambiguous option: {} could match {}",
                            String::from_utf8_lossy(tok),
                            list.join(", ")
                        );
                        return fail(m.into_bytes());
                    }
                }
                i += 1;
                continue;
            }
            // Single dash: only `-h` exists. `-h` fires the help action;
            // `-h*` (no `=`) does too, as argparse parses the `-h`
            // prefix first (probed: `-hx`, `-help`). With `=` it is an
            // explicit-argument error (probed: `-h=x`).
            if tok == b"-h" || (tok.starts_with(b"-h") && !tok.contains(&b'=')) {
                stdout_write(&outer_help(prog));
                return OuterParse::Help;
            }
            if tok.starts_with(b"-h=") {
                let v = &tok[3..];
                let mut m = Vec::from(b"argument -h/--help: ignored explicit argument ".as_slice());
                m.extend_from_slice(&py_str_repr(&String::from_utf8_lossy(v)));
                return fail(m);
            }
            unrecognized.push(tok.to_vec());
            i += 1;
            continue;
        }
        o.sources.push(tok.to_vec());
        i += 1;
    }
    if !unrecognized.is_empty() {
        let m = [
            b"unrecognized arguments: ".as_slice(),
            unrecognized.join(b" ".as_slice()).as_slice(),
        ]
        .concat();
        return fail(m);
    }
    // `_parse_args`: `distdir` becomes its value + `/`.
    let mut d = std::mem::take(&mut o.distdir);
    d.push(b'/');
    o.distdir = d;
    OuterParse::Ok(o)
}

// ---------------------------------------------------------------------
// Install options (`_parse_install_options`).
// ---------------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct InstallParsed {
    owner: i64,
    group: i64,
    mode: Option<i64>,
    preserve_timestamps: bool,
}

impl InstallParsed {
    /// `Namespace(group=.., owner=.., mode=.., preserve_timestamps=..)`
    /// for the `Failed to copy file` first line (real's field order).
    fn namespace_repr(&self) -> Vec<u8> {
        let mut out = format!(
            "Namespace(group={}, owner={}, mode=",
            self.group, self.owner
        )
        .into_bytes();
        match self.mode {
            Some(m) => out.extend_from_slice(m.to_string().as_bytes()),
            None => out.extend_from_slice(b"None"),
        }
        out.extend_from_slice(b", preserve_timestamps=");
        out.extend_from_slice(if self.preserve_timestamps {
            b"True"
        } else {
            b"False"
        });
        out.push(b')');
        out
    }
}

enum InstallRoute {
    InProcess(InstallParsed),
    /// Raw `shlex.split` tokens for `install(1)`.
    Subprocess(Vec<Vec<u8>>),
}

/// What `_parse_install_options` decided. `Printed(i32)` means the
/// message is already on stdout/stderr — return the code.
enum InstallOutcome {
    Route(InstallRoute),
    Printed(i32),
    /// Real's uncaught exception (`shlex.split`'s `ValueError`): its
    /// final traceback line, rc 1.
    Uncaught(Vec<u8>),
}

fn install_arg_error(prog: &str, msg: &str) -> InstallOutcome {
    let mut m = install_usage(prog);
    m.extend_from_slice(format!("{prog}: error: {msg}").as_bytes());
    m.push(b'\n');
    stderr_write(&m);
    InstallOutcome::Printed(2)
}

/// `argument -o/--owner: invalid _parse_user value: '...'` (exit 2).
fn type_error(prog: &str, names: &str, value: &str, func: &str) -> InstallOutcome {
    install_arg_error(
        prog,
        &format!(
            "argument {names}: invalid {func} value: {}",
            String::from_utf8_lossy(&py_str_repr(value))
        ),
    )
}

/// Parse one short-option cluster (without the leading dash).
/// Unknown shorts put the token (or, mid-cluster, the `-tail`) in
/// `remaining`, as argparse's extras do (`-vp` stays whole, `-pv`
/// leaves `-v`).
#[allow(clippy::too_many_arguments)]
fn parse_install_shorts(
    prog: &str,
    rest: &[u8],
    argv: &[Vec<u8>],
    pos: &mut usize,
    parsed: &mut InstallParsed,
    remaining: &mut Vec<Vec<u8>>,
) -> Result<(), InstallOutcome> {
    let mut off = 0;
    while off < rest.len() {
        let c = rest[off];
        match c {
            b'h' => {
                // `-h`: help fires wherever argparse reaches it
                // (`-hx`, `-help`, `-ph`); `-h=x` is an
                // explicit-argument error.
                if off == 0 && rest.len() == 1 {
                    stdout_write(&install_help(prog));
                    return Err(InstallOutcome::Printed(0));
                }
                if off == 0 && rest.get(1) == Some(&b'=') {
                    let v = &rest[2..];
                    return Err(install_arg_error(
                        prog,
                        &format!(
                            "argument -h/--help: ignored explicit argument {}",
                            String::from_utf8_lossy(&py_str_repr(&String::from_utf8_lossy(v)))
                        ),
                    ));
                }
                stdout_write(&install_help(prog));
                return Err(InstallOutcome::Printed(0));
            }
            b'p' => {
                parsed.preserve_timestamps = true;
                off += 1;
            }
            b'g' | b'o' | b'm' => {
                let (names, is_mode) = match c {
                    b'g' => ("-g/--group", false),
                    b'o' => ("-o/--owner", false),
                    _ => ("-m/--mode", true),
                };
                let inline = &rest[off + 1..];
                // A leading `=` is the `--x=v` spelling (`-m=0600`).
                let inline = inline.strip_prefix(b"=").unwrap_or(inline);
                let value: Vec<u8> = if !inline.is_empty() {
                    inline.to_vec()
                } else if *pos + 1 < argv.len() {
                    *pos += 1;
                    argv[*pos].clone()
                } else {
                    return Err(install_arg_error(
                        prog,
                        &format!("argument {names}: expected one argument"),
                    ));
                };
                let text = String::from_utf8_lossy(&value).into_owned();
                if is_mode {
                    parsed.mode = parse_mode(&text);
                } else if c == b'g' {
                    match parse_group(&text) {
                        Some(v) => parsed.group = v,
                        None => return Err(type_error(prog, names, &text, "_parse_group")),
                    }
                } else {
                    match parse_user(&text) {
                        Some(v) => parsed.owner = v,
                        None => return Err(type_error(prog, names, &text, "_parse_user")),
                    }
                }
                return Ok(());
            }
            _ => {
                let mut tok = vec![b'-'];
                tok.extend_from_slice(if off == 0 { rest } else { &rest[off..] });
                remaining.push(tok);
                return Ok(());
            }
        }
    }
    Ok(())
}

fn parse_install_options(
    prog: &str,
    options: &[u8],
    is_strict: bool,
    helper: &str,
) -> InstallOutcome {
    const LONGS: &[&str] = &["group", "owner", "mode", "preserve-timestamps", "help"];
    let split = match shlex_split(options) {
        Ok(split) => split,
        Err(msg) => return InstallOutcome::Uncaught(format!("ValueError: {msg}").into_bytes()),
    };
    let mut parsed = InstallParsed {
        owner: -1,
        group: -1,
        mode: Some(0o755),
        preserve_timestamps: false,
    };
    let mut remaining: Vec<Vec<u8>> = Vec::new();
    let mut pos = 0;
    let mut end_opts = false;
    while pos < split.len() {
        let tok = &split[pos];
        // With no positionals, argparse's `parse_known_args` keeps a
        // `--` separator in extras (probed: `-- -v` gives
        // `['--', '-v']`).
        if end_opts {
            remaining.push(tok.clone());
            pos += 1;
            continue;
        }
        if tok == b"--" {
            remaining.push(tok.clone());
            end_opts = true;
            pos += 1;
            continue;
        }
        if tok.len() > 1 && tok.starts_with(b"-") && tok != b"-" {
            if tok.starts_with(b"--") {
                let body = &tok[2..];
                let (name, inline) = match body.iter().position(|c| *c == b'=') {
                    Some(p) => (&body[..p], Some(&body[p + 1..])),
                    None => (body, None),
                };
                match match_long(name, LONGS) {
                    Ok(idx) if LONGS[idx] == "help" => {
                        if let Some(v) = inline {
                            return install_arg_error(
                                prog,
                                &format!(
                                    "argument -h/--help: ignored explicit argument {}",
                                    String::from_utf8_lossy(&py_str_repr(
                                        &String::from_utf8_lossy(v)
                                    ))
                                ),
                            );
                        }
                        stdout_write(&install_help(prog));
                        return InstallOutcome::Printed(0);
                    }
                    Ok(idx) => {
                        if LONGS[idx] == "preserve-timestamps" {
                            if let Some(v) = inline {
                                return install_arg_error(
                                    prog,
                                    &format!(
                                        "argument -p/--preserve-timestamps: ignored explicit argument {}",
                                        String::from_utf8_lossy(&py_str_repr(
                                            &String::from_utf8_lossy(v)
                                        ))
                                    ),
                                );
                            }
                            parsed.preserve_timestamps = true;
                        } else {
                            let (names, func, is_mode) = match LONGS[idx].as_bytes()[0] {
                                b'g' => ("-g/--group", "_parse_group", false),
                                b'o' => ("-o/--owner", "_parse_user", false),
                                _ => ("-m/--mode", "_parse_mode", true),
                            };
                            let value: Vec<u8> = if let Some(v) = inline {
                                v.to_vec()
                            } else if pos + 1 < split.len() {
                                pos += 1;
                                split[pos].clone()
                            } else {
                                return install_arg_error(
                                    prog,
                                    &format!("argument {names}: expected one argument"),
                                );
                            };
                            let text = String::from_utf8_lossy(&value).into_owned();
                            if is_mode {
                                parsed.mode = parse_mode(&text);
                            } else if LONGS[idx].as_bytes()[0] == b'g' {
                                match parse_group(&text) {
                                    Some(v) => parsed.group = v,
                                    None => return type_error(prog, names, &text, func),
                                }
                            } else {
                                match parse_user(&text) {
                                    Some(v) => parsed.owner = v,
                                    None => return type_error(prog, names, &text, func),
                                }
                            }
                        }
                    }
                    Err(hits) if hits.is_empty() => remaining.push(tok.clone()),
                    Err(hits) => {
                        // An ambiguous long prefix is an error even in
                        // `parse_known_args`.
                        let list: Vec<String> =
                            hits.iter().map(|h| format!("--{}", LONGS[*h])).collect();
                        return install_arg_error(
                            prog,
                            &format!(
                                "ambiguous option: {} could match {}",
                                String::from_utf8_lossy(tok),
                                list.join(", ")
                            ),
                        );
                    }
                }
                pos += 1;
                continue;
            }
            // Short cluster.
            let rest = tok[1..].to_vec();
            if let Err(o) =
                parse_install_shorts(prog, &rest, &split, &mut pos, &mut parsed, &mut remaining)
            {
                return o;
            }
            pos += 1;
            continue;
        }
        remaining.push(tok.clone());
        pos += 1;
    }
    if !remaining.is_empty() || parsed.mode.is_none() {
        let mut first = Vec::from(b"Unknown install options: ".as_slice());
        first.extend_from_slice(options);
        first.extend_from_slice(b", ");
        first.extend_from_slice(&py_str_list_repr(&remaining));
        warn(helper, &first);
        if is_strict {
            return InstallOutcome::Printed(1);
        }
        warn(
            helper,
            b"Continue with falling back to `install` command execution, which can be slower.",
        );
        return InstallOutcome::Route(InstallRoute::Subprocess(split));
    }
    InstallOutcome::Route(InstallRoute::InProcess(parsed))
}

// ---------------------------------------------------------------------
// D7 write guard: every filesystem mutation must land under the
// canonical `$D` (when `D` is set and non-empty in the env), else under
// the canonical `--dest`. A static binary escapes libsandbox's
// `LD_PRELOAD`, so without this a symlink inside the dest tree could
// redirect writes outside it.
// ---------------------------------------------------------------------

struct Guard {
    root: PathBuf,
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(p)
    }
}

/// Lexically normalise an absolute path (`.`/`..` only; symlinks are
/// left alone — this is the fallback when nothing exists to
/// canonicalize).
fn lexical_normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            _ => out.push(comp.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Canonicalize `p`: the real path when it (or an ancestor) exists,
/// else the absolutised lexical form.
fn canonical_best_effort(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    let abs = absolutize(p);
    let mut anc = abs.as_path();
    while let Some(parent) = anc.parent() {
        if let Ok(c) = std::fs::canonicalize(parent) {
            let mut out = c;
            if let Ok(rest) = abs.strip_prefix(parent) {
                out.push(rest);
            }
            return lexical_normalise(&out);
        }
        anc = parent;
    }
    lexical_normalise(&abs)
}

fn under_root(root: &Path, p: &Path) -> bool {
    p == root || p.starts_with(root)
}

impl Guard {
    fn from_anchor(anchor: &Path) -> Self {
        Guard {
            root: canonical_best_effort(anchor),
        }
    }

    fn new(dest: &[u8]) -> Self {
        match std::env::var_os("D") {
            Some(d) if !d.is_empty() => Self::from_anchor(Path::new(&d)),
            _ => Self::from_anchor(Path::new(OsStr::from_bytes(dest))),
        }
    }

    /// For creates/unlinks/mkdirs/symlinks: the canonical parent must
    /// be under the root. Returns the offending path on violation.
    /// Creating the root itself (or, equivalently, resolving to it)
    /// is allowed too — the bootstrap for `--dest` when `D` is unset.
    /// Anything else outside, including siblings of the root, is
    /// refused.
    fn check_parent(&self, p: &[u8]) -> Result<(), Vec<u8>> {
        let path = Path::new(OsStr::from_bytes(p));
        let abs = absolutize(path);
        let parent = abs
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let canon = canonical_best_effort(&parent);
        if under_root(&self.root, &canon) {
            return Ok(());
        }
        if under_root(&canonical_best_effort(&abs), &self.root) {
            return Ok(());
        }
        Err(p.to_vec())
    }

    /// For mutating a path itself (chown/chmod/utime/xattr, which
    /// follow links): the canonical path must be under the root.
    fn check_resolved(&self, p: &[u8]) -> Result<(), Vec<u8>> {
        let path = Path::new(OsStr::from_bytes(p));
        match std::fs::canonicalize(path) {
            Ok(c) if under_root(&self.root, &c) => Ok(()),
            Ok(_) => Err(p.to_vec()),
            // Dangling or missing: fall back to the parent check and
            // let the operation itself fail naturally.
            Err(_) => self.check_parent(p),
        }
    }
}

// ---------------------------------------------------------------------
// Failures. `Uncaught` prints real's final `ExceptionType: message`
// line (rc 1). `Guard` prints the D7 refusal (rc 1).
// ---------------------------------------------------------------------

enum Fatal {
    Uncaught(Vec<u8>),
    Guard(Vec<u8>),
}

/// Map an errno to real's Python exception type name.
fn exc_name(errno: i32) -> &'static str {
    match errno {
        x if x == libc::ENOENT => "FileNotFoundError",
        x if x == libc::EEXIST => "FileExistsError",
        x if x == libc::EISDIR => "IsADirectoryError",
        x if x == libc::ENOTDIR => "NotADirectoryError",
        x if x == libc::EACCES || x == libc::EPERM => "PermissionError",
        _ => "OSError",
    }
}

fn c_strerror(errno: i32) -> String {
    unsafe {
        let p = libc::strerror(errno);
        if p.is_null() {
            return format!("Unknown error {errno}");
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// `TypeError: message` (no errno), e.g. the missing-`--dest` crash.
fn type_error_line(msg: &str) -> Vec<u8> {
    format!("TypeError: {msg}").into_bytes()
}

/// `ExcType: [Errno n] strerror: b'path'`.
pub(super) fn errno_line(errno: i32, path: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(exc_name(errno).as_bytes());
    out.extend_from_slice(format!(": [Errno {errno}] {}: ", c_strerror(errno)).as_bytes());
    out.extend_from_slice(&py_bytes_repr(path));
    out
}

/// `ExcType: [Errno n] strerror: 'str-path'` (for the `install`
/// fallback's `FileNotFoundError: ...: 'install'`).
fn errno_line_str(errno: i32, path: &str) -> Vec<u8> {
    let mut out = Vec::from(exc_name(errno).as_bytes());
    out.extend_from_slice(format!(": [Errno {errno}] {}: ", c_strerror(errno)).as_bytes());
    out.extend_from_slice(&py_str_repr(path));
    out
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

// ---------------------------------------------------------------------
// xattr (`movefile._copyxattr` over `portage.util._xattr`, which calls
// the following variants).
// ---------------------------------------------------------------------

fn c_path(p: &[u8]) -> Result<std::ffi::CString, ()> {
    std::ffi::CString::new(p).map_err(|_| ())
}

/// `xattr.list(src)`: ENOTSUP/EOPNOTSUPP means "none" (`attrs = ()`).
/// Shared with `helpers/xattr.rs` (#326 S7: one copier, not two).
pub(super) fn xattr_list(path: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
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

pub(super) fn xattr_get(path: &[u8], name: &[u8]) -> Result<Vec<u8>, i32> {
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

/// Shared with `helpers/xattr.rs` (#326 S7).
pub(super) fn xattr_set(path: &[u8], name: &[u8], value: &[u8]) -> Result<(), i32> {
    let c = c_path(path).map_err(|_| libc::EINVAL)?;
    let n = std::ffi::CString::new(name).map_err(|_| libc::EINVAL)?;
    let r = unsafe {
        libc::setxattr(
            c.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if r != 0 { Err(last_errno()) } else { Ok(()) }
}

/// `_copyxattr(src, dest, exclude)`. `Err` is either an errno line
/// (list/get failures other than ENOTSUP) or the full
/// `OperationNotSupported: ...` line (set failures).
/// Shared with `helpers/xattr.rs` (#326 S7: `install.py` copies through
/// this, never its own).
pub(super) fn copy_xattr(src: &[u8], dest: &[u8], exclude: &[u8]) -> Result<(), Vec<u8>> {
    let attrs = xattr_list(src).map_err(|e| errno_line(e, src))?;
    if attrs.is_empty() {
        return Ok(());
    }
    let excl = String::from_utf8_lossy(exclude).into_owned();
    for attr in &attrs {
        if xattr_excluded(&excl, attr) {
            continue;
        }
        let value = xattr_get(src, attr).map_err(|e| errno_line(e, src))?;
        if xattr_set(dest, attr, &value).is_err() {
            // `OperationNotSupported("Filesystem containing file '%s'
            // does not support extended attribute '%s'")`.
            let mut line =
                Vec::from("OperationNotSupported: Filesystem containing file '".as_bytes());
            line.extend_from_slice(String::from_utf8_lossy(dest).as_bytes());
            line.extend_from_slice(b"' does not support extended attribute '");
            line.extend_from_slice(String::from_utf8_lossy(attr).as_bytes());
            line.push(b'\'');
            return Err(line);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Filesystem operations (all D7-guarded).
// ---------------------------------------------------------------------

fn path_exists_follow(p: &[u8]) -> bool {
    std::fs::metadata(OsStr::from_bytes(p)).is_ok()
}

fn path_is_dir_follow(p: &[u8]) -> bool {
    Path::new(OsStr::from_bytes(p)).is_dir()
}

fn path_is_link(p: &[u8]) -> bool {
    std::fs::symlink_metadata(OsStr::from_bytes(p))
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn read_link(p: &[u8]) -> Result<Vec<u8>, i32> {
    std::fs::read_link(OsStr::from_bytes(p))
        .map(|p| p.into_os_string().into_vec())
        .map_err(|e| e.raw_os_error().unwrap_or(libc::EIO))
}

fn os_stat(p: &[u8]) -> Result<std::fs::Metadata, i32> {
    std::fs::metadata(OsStr::from_bytes(p)).map_err(|e| e.raw_os_error().unwrap_or(libc::EIO))
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

fn realpath_best_effort(p: &[u8]) -> Vec<u8> {
    canonical_best_effort(Path::new(OsStr::from_bytes(p)))
        .into_os_string()
        .into_vec()
}

/// `posixpath.dirname`.
fn dirname_of(p: &[u8]) -> Vec<u8> {
    match p.iter().rposition(|c| *c == b'/') {
        None => Vec::new(),
        Some(0) => b"/".to_vec(),
        Some(i) => {
            let mut head = &p[..i];
            while head.len() > 1 && head.ends_with(b"/") {
                head = &head[..head.len() - 1];
            }
            if head.is_empty() {
                b"/".to_vec()
            } else {
                head.to_vec()
            }
        }
    }
}

/// `posixpath.basename`.
fn basename_of(p: &[u8]) -> Vec<u8> {
    match p.iter().rposition(|c| *c == b'/') {
        Some(i) => p[i + 1..].to_vec(),
        None => p.to_vec(),
    }
}

/// `os.path.join(a, b)` for a non-absolute `b`.
fn join_path(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() {
        return b.to_vec();
    }
    let mut head = a;
    while head.len() > 1 && head.ends_with(b"/") {
        head = &head[..head.len() - 1];
    }
    [head, b"/", b].concat()
}

/// `source.rstrip(b"/")`, exactly (all-slash input strips to empty).
fn strip_trailing_slashes(p: &[u8]) -> Vec<u8> {
    let mut end = p.len();
    while end > 0 && p[end - 1] == b'/' {
        end -= 1;
    }
    p[..end].to_vec()
}

fn c_mkdir(p: &[u8]) -> Result<(), i32> {
    let c = c_path(p).map_err(|_| libc::EINVAL)?;
    if unsafe { libc::mkdir(c.as_ptr(), 0o777) } == 0 {
        Ok(())
    } else {
        Err(last_errno())
    }
}

fn c_unlink(p: &[u8]) -> Result<(), i32> {
    let c = c_path(p).map_err(|_| libc::EINVAL)?;
    if unsafe { libc::unlink(c.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(last_errno())
    }
}

fn c_symlink(target: &[u8], link: &[u8]) -> Result<(), i32> {
    let t = c_path(target).map_err(|_| libc::EINVAL)?;
    let l = c_path(link).map_err(|_| libc::EINVAL)?;
    if unsafe { libc::symlink(t.as_ptr(), l.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(last_errno())
    }
}

fn c_lchown(p: &[u8], owner: i64, group: i64) -> Result<(), Vec<u8>> {
    // `-1` keeps the id; anything outside `u32` cannot be passed to
    // the syscall (real's `OverflowError`, caught and logged there).
    const OVERFLOW: &[u8] = b"OverflowError: Python int too large to convert to C long";
    let uid = if owner == -1 {
        u32::MAX
    } else if (0..=i64::from(u32::MAX)).contains(&owner) {
        owner as u32
    } else {
        return Err(OVERFLOW.to_vec());
    };
    let gid = if group == -1 {
        u32::MAX
    } else if (0..=i64::from(u32::MAX)).contains(&group) {
        group as u32
    } else {
        return Err(OVERFLOW.to_vec());
    };
    let c = c_path(p).map_err(|_| errno_line(libc::EINVAL, p))?;
    if unsafe { libc::lchown(c.as_ptr(), uid, gid) } == 0 {
        Ok(())
    } else {
        Err(errno_line(last_errno(), p))
    }
}

fn c_chmod(p: &[u8], mode: i64) -> Result<(), Vec<u8>> {
    if mode < 0 {
        return Err(b"OverflowError: Python int too large to convert to C long".to_vec());
    }
    let c = c_path(p).map_err(|_| errno_line(libc::EINVAL, p))?;
    if unsafe { libc::chmod(c.as_ptr(), mode as u32) } == 0 {
        Ok(())
    } else {
        Err(errno_line(last_errno(), p))
    }
}

fn c_utime_ns(p: &[u8], atime: (i64, i64), mtime: (i64, i64)) -> Result<(), Vec<u8>> {
    let c = c_path(p).map_err(|_| errno_line(libc::EINVAL, p))?;
    let times = [
        libc::timespec {
            tv_sec: atime.0 as libc::time_t,
            tv_nsec: atime.1 as libc::c_long,
        },
        libc::timespec {
            tv_sec: mtime.0 as libc::time_t,
            tv_nsec: mtime.1 as libc::c_long,
        },
    ];
    if unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(errno_line(last_errno(), p))
    }
}

// ---------------------------------------------------------------------
// The install runners.
// ---------------------------------------------------------------------

/// `makedirs` without `exist_ok`: missing parents are created, a final
/// EEXIST is reported for the caller to judge.
enum MakedirsFail {
    Errno(i32, Vec<u8>),
    Guard(Vec<u8>),
}

struct Ctx {
    opts: OuterOpts,
    guard: Guard,
    ins: InstallRoute,
    dir: InstallRoute,
}

impl Ctx {
    fn helper(&self) -> String {
        helper_display(&self.opts.helper)
    }

    fn dest(&self) -> Vec<u8> {
        self.opts.dest.clone().unwrap_or_default()
    }

    /// `_set_attributes(options, path)`.
    fn set_attributes(&self, parsed: &InstallParsed, path: &[u8]) -> Result<(), Fatal> {
        if parsed.owner != -1 || parsed.group != -1 {
            self.guard.check_resolved(path).map_err(Fatal::Guard)?;
            c_lchown(path, parsed.owner, parsed.group).map_err(Fatal::Uncaught)?;
        }
        if let Some(mode) = parsed.mode {
            self.guard.check_resolved(path).map_err(Fatal::Guard)?;
            c_chmod(path, mode).map_err(Fatal::Uncaught)?;
        }
        Ok(())
    }

    fn makedirs(&self, dest: &[u8]) -> Result<(), MakedirsFail> {
        let path = Path::new(OsStr::from_bytes(dest));
        // Missing ancestors, created top-down (their own EEXIST is
        // swallowed, like the `except FileExistsError: pass` in real's
        // recursion — but only when the ancestor is reachable).
        let mut missing: Vec<Vec<u8>> = Vec::new();
        let mut anc = path;
        while std::fs::symlink_metadata(anc).is_err() {
            missing.push(anc.as_os_str().as_bytes().to_vec());
            match anc.parent() {
                Some(p) if !p.as_os_str().is_empty() => anc = p,
                _ => break,
            }
        }
        for prefix in missing.iter().rev() {
            if prefix.as_slice() == dest {
                continue;
            }
            // `os.path.exists` follows links; anything reachable is
            // left alone.
            if path_exists_follow(prefix) {
                continue;
            }
            self.guard
                .check_parent(prefix)
                .map_err(MakedirsFail::Guard)?;
            if let Err(e) = c_mkdir(prefix)
                && (e != libc::EEXIST || !path_is_dir_follow(prefix))
            {
                return Err(MakedirsFail::Errno(e, prefix.clone()));
            }
        }
        self.guard.check_parent(dest).map_err(MakedirsFail::Guard)?;
        if let Err(e) = c_mkdir(dest) {
            return Err(MakedirsFail::Errno(e, dest.to_vec()));
        }
        Ok(())
    }

    /// `_DirInProcessInstallRunner.run`.
    fn dir_in_process(&self, parsed: &InstallParsed, dest: &[u8]) -> Result<(), Fatal> {
        match self.makedirs(dest) {
            Ok(()) => {}
            Err(MakedirsFail::Errno(e, _)) if e == libc::EEXIST && path_is_dir_follow(dest) => {}
            Err(MakedirsFail::Errno(e, p)) => return Err(Fatal::Uncaught(errno_line(e, &p))),
            Err(MakedirsFail::Guard(p)) => return Err(Fatal::Guard(p)),
        }
        self.set_attributes(parsed, dest)
    }

    /// `_DirSubprocessInstallRunner.run`: `install -d <opts> <dest>`.
    /// A failure is an error for `install_dir` to judge (real's
    /// `check_call` raises into it).
    fn dir_subprocess(&self, split: &[Vec<u8>], dest: &[u8]) -> Result<(), Fatal> {
        self.guard.check_parent(dest).map_err(Fatal::Guard)?;
        let mut cmd = std::process::Command::new("install");
        cmd.arg("-d");
        for s in split {
            cmd.arg(OsStr::from_bytes(s));
        }
        cmd.arg(OsStr::from_bytes(dest));
        match cmd.status() {
            Ok(st) if st.success() => Ok(()),
            Ok(st) => {
                // `["install", "-d"] + split_options + [dest]`: the
                // options are `str`, `dest` is `bytes`; the message ends
                // with a period (`CalledProcessError.__str__`).
                let mut full: Vec<Vec<u8>> = vec![b"install".to_vec(), b"-d".to_vec()];
                full.extend(split.iter().cloned());
                let mut cmd_repr = py_str_list_repr(&full);
                cmd_repr.pop();
                cmd_repr.extend_from_slice(b", ");
                cmd_repr.extend_from_slice(&py_bytes_repr(dest));
                cmd_repr.push(b']');
                let mut line = Vec::from(b"CalledProcessError: Command '".as_slice());
                line.extend_from_slice(&cmd_repr);
                line.extend_from_slice(
                    format!(
                        "' returned non-zero exit status {}.",
                        st.code().unwrap_or(-1)
                    )
                    .as_bytes(),
                );
                Err(Fatal::Uncaught(line))
            }
            Err(e) => Err(Fatal::Uncaught(errno_line_str(
                e.raw_os_error().unwrap_or(libc::ENOENT),
                "install",
            ))),
        }
    }

    /// `install_dir`: re-raise only with `--helpers_can_die`,
    /// otherwise log `install_dir failed.` and swallow.
    fn install_dir(&self, dest: &[u8]) -> Result<(), Fatal> {
        let r = match &self.dir {
            InstallRoute::InProcess(p) => self.dir_in_process(p, dest),
            InstallRoute::Subprocess(s) => self.dir_subprocess(s, dest),
        };
        match r {
            Ok(()) => Ok(()),
            Err(Fatal::Uncaught(line)) if !self.opts.helpers_can_die => {
                stderr_write(b"install_dir failed.\n");
                stderr_write(&line);
                stderr_write(b"\n");
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// `_is_install_allowed`, with its same-file warning.
    fn is_install_allowed(
        &self,
        source: &[u8],
        sstat: &std::fs::Metadata,
        dest: &[u8],
    ) -> Result<bool, Fatal> {
        let dstat = match std::fs::symlink_metadata(OsStr::from_bytes(dest)) {
            Ok(m) => m,
            Err(e) => {
                let e = e.raw_os_error().unwrap_or(libc::EIO);
                if e == libc::ENOENT {
                    return Ok(true);
                }
                return Err(Fatal::Uncaught(errno_line(e, dest)));
            }
        };
        if dstat.file_type().is_symlink() {
            return Ok(true);
        }
        if !same_file(sstat, &dstat) {
            return Ok(true);
        }
        if dstat.nlink() > 1 && realpath_best_effort(source) != realpath_best_effort(dest) {
            return Ok(true);
        }
        let mut msg = py_bytes_repr(source);
        msg.extend_from_slice(b" and ");
        msg.extend_from_slice(&py_bytes_repr(dest));
        msg.extend_from_slice(b" are same file.");
        warn(&self.helper(), &msg);
        Ok(false)
    }

    /// Copy the contents into a newly created file (`0o666 & ~umask`,
    /// like Python's `open(dst, "wb")`), then attributes, xattrs,
    /// timestamps. Failures log the `Failed to copy file` first line
    /// plus the final line and return False.
    fn copy_and_set(
        &self,
        parsed: &InstallParsed,
        source: &[u8],
        sstat: &std::fs::Metadata,
        dest: &[u8],
        dest_dir: &[u8],
    ) -> Result<bool, Fatal> {
        let failed = |line: Vec<u8>| -> Result<bool, Fatal> {
            let mut first = Vec::from(b"Failed to copy file: _parsed_options=".as_slice());
            first.extend_from_slice(&parsed.namespace_repr());
            first.extend_from_slice(b", source=");
            first.extend_from_slice(&py_bytes_repr(source));
            first.extend_from_slice(b", dest_dir=");
            first.extend_from_slice(&py_bytes_repr(dest_dir));
            stderr_write(&first);
            stderr_write(b"\n");
            stderr_write(&line);
            stderr_write(b"\n");
            Ok(false)
        };
        self.guard.check_parent(dest).map_err(Fatal::Guard)?;
        let mut src_file = match std::fs::File::open(OsStr::from_bytes(source)) {
            Ok(f) => f,
            Err(e) => {
                return failed(errno_line(e.raw_os_error().unwrap_or(libc::EIO), source));
            }
        };
        let mut dst_file = match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(OsStr::from_bytes(dest))
        {
            Ok(f) => f,
            Err(e) => {
                return failed(errno_line(e.raw_os_error().unwrap_or(libc::EIO), dest));
            }
        };
        if let Err(e) = std::io::copy(&mut src_file, &mut dst_file) {
            return failed(errno_line(e.raw_os_error().unwrap_or(libc::EIO), dest));
        }
        drop(dst_file);
        match self.set_attributes(parsed, dest) {
            Ok(()) => {}
            Err(Fatal::Guard(p)) => return Err(Fatal::Guard(p)),
            Err(Fatal::Uncaught(line)) => return failed(line),
        }
        if self.opts.enable_copy_xattr
            && let Err(line) = copy_xattr(source, dest, &self.opts.xattr_exclude)
        {
            return failed(line);
        }
        if parsed.preserve_timestamps {
            let at = (sstat.atime(), sstat.atime_nsec());
            let mt = (sstat.mtime(), sstat.mtime_nsec());
            self.guard.check_resolved(dest).map_err(Fatal::Guard)?;
            if let Err(line) = c_utime_ns(dest, at, mt) {
                return failed(line);
            }
        }
        Ok(true)
    }

    /// `_InsInProcessInstallRunner.run`.
    fn ins_in_process(
        &self,
        parsed: &InstallParsed,
        source: &[u8],
        dest_dir: &[u8],
    ) -> Result<bool, Fatal> {
        let dest = join_path(dest_dir, &basename_of(source));
        // `stat` follows links; a failure is an uncaught error.
        let sstat = os_stat(source).map_err(|e| Fatal::Uncaught(errno_line(e, source)))?;
        if !self.is_install_allowed(source, &sstat, &dest)? {
            return Ok(false);
        }
        // Unlink first, to emulate `install`. ENOENT is fine; anything
        // else is uncaught (the `raise` outside real's `try`).
        self.guard.check_parent(&dest).map_err(Fatal::Guard)?;
        match c_unlink(&dest) {
            Ok(()) => {}
            Err(e) if e == libc::ENOENT => {}
            Err(e) => return Err(Fatal::Uncaught(errno_line(e, &dest))),
        }
        self.copy_and_set(parsed, source, &sstat, &dest, dest_dir)
    }

    /// `_InsSubprocessInstallRunner.run`: `install <opts> <source>
    /// <dest_dir>` from PATH, stdio inherited, True iff rc 0. A
    /// missing `install` is uncaught (real's `subprocess.call` has no
    /// `try` here).
    fn ins_subprocess(
        &self,
        split: &[Vec<u8>],
        source: &[u8],
        dest_dir: &[u8],
    ) -> Result<bool, Fatal> {
        self.guard.check_parent(dest_dir).map_err(Fatal::Guard)?;
        let mut cmd = std::process::Command::new("install");
        for s in split {
            cmd.arg(OsStr::from_bytes(s));
        }
        cmd.arg(OsStr::from_bytes(source));
        cmd.arg(OsStr::from_bytes(dest_dir));
        match cmd.status() {
            Ok(st) => Ok(st.success()),
            Err(e) => Err(Fatal::Uncaught(errno_line_str(
                e.raw_os_error().unwrap_or(libc::ENOENT),
                "install",
            ))),
        }
    }

    fn install_file(&self, source: &[u8], dest_dir: &[u8]) -> Result<bool, Fatal> {
        match &self.ins {
            InstallRoute::InProcess(p) => self.ins_in_process(p, source, dest_dir),
            InstallRoute::Subprocess(s) => self.ins_subprocess(s, source, dest_dir),
        }
    }

    /// The symlink branch of `_doins`: unlink the dest (rmtree if it
    /// is a dir; other unlink errors are swallowed, as real's bare
    /// `except OSError` does) and re-create the link.
    fn place_symlink(&self, linkto: &[u8], dest: &[u8]) -> Result<(), Fatal> {
        self.guard.check_parent(dest).map_err(Fatal::Guard)?;
        match c_unlink(dest) {
            Ok(()) | Err(libc::ENOENT) => {}
            Err(e) if e == libc::EISDIR => {
                self.guard.check_parent(dest).map_err(Fatal::Guard)?;
                let _ = std::fs::remove_dir_all(OsStr::from_bytes(dest));
            }
            Err(_) => {}
        }
        self.guard.check_parent(dest).map_err(Fatal::Guard)?;
        c_symlink(linkto, dest).map_err(|e| Fatal::Uncaught(errno_line(e, dest)))
    }

    fn symlink_failed_first_line(&self, relpath: &[u8], source_root: &[u8]) -> Vec<u8> {
        let mut first = Vec::from(b"Failed to create symlink: opts=".as_slice());
        first.extend_from_slice(&outer_namespace_repr(&self.opts));
        first.extend_from_slice(b", relpath=");
        first.extend_from_slice(&py_bytes_repr(relpath));
        first.extend_from_slice(b", source_root=");
        first.extend_from_slice(&py_bytes_repr(source_root));
        first
    }

    /// `_doins`: install `source_root/relpath` into `dest/relpath`.
    fn doins_one(&self, relpath: &[u8], source_root: &[u8]) -> Result<bool, Fatal> {
        let source = join_path(source_root, relpath);
        let dest = join_path(&self.dest(), relpath);
        if path_is_link(&source) && self.opts.preserve_symlinks {
            match read_link(&source) {
                Ok(linkto) if !linkto.starts_with(&self.opts.distdir) => {
                    match self.place_symlink(&linkto, &dest) {
                        Ok(()) => return Ok(true),
                        Err(Fatal::Guard(p)) => return Err(Fatal::Guard(p)),
                        Err(Fatal::Uncaught(line)) => {
                            stderr_write(&self.symlink_failed_first_line(relpath, source_root));
                            stderr_write(b"\n");
                            stderr_write(&line);
                            stderr_write(b"\n");
                            return Ok(false);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    stderr_write(&self.symlink_failed_first_line(relpath, source_root));
                    stderr_write(b"\n");
                    stderr_write(&errno_line(e, &source));
                    stderr_write(b"\n");
                    return Ok(false);
                }
            }
        }
        self.install_file(&source, &dirname_of(&dest))
    }

    /// `_install_dir`: `None` = skipped (no `-r`, non-`dodoc` helper).
    fn install_dir_tree(&self, source: &[u8]) -> Result<Option<bool>, Fatal> {
        if !self.opts.recursive {
            if self.opts.helper.as_deref() == Some(b"dodoc".as_slice()) {
                let mut msg = py_bytes_repr(source);
                msg.extend_from_slice(b" is a directory");
                warn(&self.helper(), &msg);
                return Ok(Some(false));
            }
            return Ok(None);
        }
        let source = strip_trailing_slashes(source);
        let source_root = dirname_of(&source);
        let dest_top = join_path(&self.dest(), &basename_of(&source));
        self.install_dir(&dest_top)?;
        let mut relpaths: Vec<Vec<u8>> = Vec::new();
        self.walk(&source, &basename_of(&source), &mut relpaths)?;
        if relpaths.is_empty() {
            // An empty `-r` dir still counts as success.
            return Ok(Some(true));
        }
        let mut success = true;
        for rel in &relpaths {
            if !self.doins_one(rel, &source_root)? {
                success = false;
            }
        }
        Ok(Some(success))
    }

    /// One level of real's `os.walk` (no `followlinks`): symlinked
    /// dirs are queued for `_doins`, real dirs get an `install_dir`
    /// immediately, everything else is queued. A listing error is
    /// silently skipped, as `os.walk` does.
    fn walk(&self, dir: &[u8], rel_here: &[u8], out: &mut Vec<Vec<u8>>) -> Result<(), Fatal> {
        let entries = match std::fs::read_dir(OsStr::from_bytes(dir)) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        let mut dirnames: Vec<Vec<u8>> = Vec::new();
        let mut filenames: Vec<Vec<u8>> = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name().into_vec();
            let full = join_path(dir, &name);
            let ft = match std::fs::symlink_metadata(e.path()) {
                Ok(m) => m.file_type(),
                Err(_) => continue,
            };
            if ft.is_dir() {
                dirnames.push(name);
            } else if ft.is_symlink() {
                // `entry.is_dir()` follows links: a symlink to a dir
                // walks as a dirname, anything else as a filename.
                if os_stat(&full).map(|m| m.is_dir()).unwrap_or(false) {
                    dirnames.push(name);
                } else {
                    filenames.push(name);
                }
            } else {
                filenames.push(name);
            }
        }
        // Real's loop body runs per yielded level, top-down: this
        // level's dirs (link: queued; real dir: `install_dir`), then its
        // files, and only then does `os.walk` descend, skipping
        // symlinked dirs.
        let mut descend: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for name in &dirnames {
            let full = join_path(dir, name);
            let rel = join_path(rel_here, name);
            if path_is_link(&full) {
                out.push(rel);
            } else {
                let dest = join_path(&self.dest(), &rel);
                self.install_dir(&dest)?;
                descend.push((full, rel));
            }
        }
        for name in &filenames {
            out.push(join_path(rel_here, name));
        }
        for (full, rel) in &descend {
            self.walk(full, rel, out)?;
        }
        Ok(())
    }
}

/// `main`'s return rule: `0 if not any_failure and any_success
/// else 1` — an all-skipped run (no `-r` dirs only) is a failure.
fn combine_rc(any_success: bool, any_failure: bool) -> i32 {
    if !any_failure && any_success { 0 } else { 1 }
}

fn main_inner(prog: &str, argv: &[OsString]) -> Result<i32, Fatal> {
    let opts = match parse_outer(prog, argv) {
        OuterParse::Help => return Ok(0),
        OuterParse::Done(c) => return Ok(c),
        OuterParse::Ok(o) => o,
    };
    let Some(dest) = opts.dest.clone() else {
        // `os.fsencode(None)`: real's `TypeError` traceback, rc 1.
        return Err(Fatal::Uncaught(type_error_line(
            "expected str, bytes or os.PathLike object, not NoneType",
        )));
    };
    let helper = helper_display(&opts.helper);
    // `_InstallRunner(opts)`: insoptions first, then diroptions. A
    // strict refusal exits before `--dest` is created (real dies in
    // the constructor, and the oracle's `unknown-strict` case pins the
    // never-created dest).
    let ins = match parse_install_options(prog, &opts.insoptions, opts.strict_option, &helper) {
        InstallOutcome::Route(r) => r,
        InstallOutcome::Printed(c) => return Ok(c),
        InstallOutcome::Uncaught(line) => return Err(Fatal::Uncaught(line)),
    };
    let dir = match parse_install_options(prog, &opts.diroptions, opts.strict_option, &helper) {
        InstallOutcome::Route(r) => r,
        InstallOutcome::Printed(c) => return Ok(c),
        InstallOutcome::Uncaught(line) => return Err(Fatal::Uncaught(line)),
    };
    let ctx = Ctx {
        opts,
        guard: Guard::new(&dest),
        ins,
        dir,
    };
    if !path_is_dir_follow(&dest) {
        ctx.install_dir(&dest)?;
    }
    let mut any_success = false;
    let mut any_failure = false;
    for source in ctx.opts.sources.clone() {
        // `isdir` follows links; a symlinked dir with
        // `--preserve_symlinks` installs as a link.
        let is_dir = path_is_dir_follow(&source);
        let is_link = path_is_link(&source);
        if is_dir && (!ctx.opts.preserve_symlinks || !is_link) {
            match ctx.install_dir_tree(&source)? {
                None => continue,
                Some(true) => any_success = true,
                Some(false) => any_failure = true,
            }
        } else if ctx.doins_one(&basename_of(&source), &dirname_of(&source))? {
            any_success = true;
        } else {
            any_failure = true;
        }
    }
    Ok(combine_rc(any_success, any_failure))
}

/// Entry point: `script` is the `doins.py` element (real's
/// `sys.argv[0]`), `args` everything after it.
pub(crate) fn run(script: &OsString, args: &[OsString]) -> i32 {
    let prog = String::from_utf8_lossy(argv_basename(script)).into_owned();
    match main_inner(&prog, args) {
        Ok(code) => code,
        Err(Fatal::Uncaught(line)) => {
            stderr_write(&line);
            stderr_write(b"\n");
            1
        }
        Err(Fatal::Guard(path)) => {
            let root = match std::env::var_os("D") {
                Some(d) if !d.is_empty() => d.into_vec(),
                _ => b"--dest".to_vec(),
            };
            let mut msg = Vec::from(b"portuale: doins: refusing to write outside ".as_slice());
            msg.extend_from_slice(&root);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(&path);
            msg.push(b'\n');
            stderr_write(&msg);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// The `portuale` binary next to the test binary (child processes).
    /// Needs `cargo build --release -p portuale` first: `cargo test`
    /// does not rebuild it, and these tests spawn it.
    fn portuale_exe() -> PathBuf {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe
    }

    fn helper(args: &[&str]) -> std::process::Output {
        Command::new(portuale_exe())
            .args(["__helper", "python", "doins.py"])
            .args(args)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .expect("portuale __helper spawns")
    }

    /// An argparse error exits 2 with real's usage text (probed live:
    /// `python3 3rdparty/portage/bin/doins.py --bogus` with
    /// `PYTHONPATH=3rdparty/portage/lib`, Portage 3.0.82.2).
    #[test]
    fn argparse_error_exits_2_with_reals_usage_text() {
        let out = helper(&["--bogus"]);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "usage: doins.py [-h] [--recursive] [--preserve_symlinks] [--helpers_can_die]\n\
             \x20               [--distdir DISTDIR] [--insoptions INSOPTIONS]\n\
             \x20               [--diroptions DIROPTIONS] [--strict_option]\n\
             \x20               [--enable_copy_xattr] [--xattr_exclude XATTR_EXCLUDE]\n\
             \x20               [--helper HELPER] [--dest DEST]\n\
             \x20               [sources ...]\n\
             doins.py: error: unrecognized arguments: --bogus\n",
        );
    }

    /// `--help` prints real's help text byte for byte (same probe).
    #[test]
    fn outer_help_matches_real_byte_for_byte() {
        let out = helper(&["--help"]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "usage: doins.py [-h] [--recursive] [--preserve_symlinks] [--helpers_can_die]\n\
             \x20               [--distdir DISTDIR] [--insoptions INSOPTIONS]\n\
             \x20               [--diroptions DIROPTIONS] [--strict_option]\n\
             \x20               [--enable_copy_xattr] [--xattr_exclude XATTR_EXCLUDE]\n\
             \x20               [--helper HELPER] [--dest DEST]\n\
             \x20               [sources ...]\n\
             \n\
             positional arguments:\n\
             \x20 sources               Source file/directory paths to be installed.\n\
             \n\
             options:\n\
             \x20 -h, --help            show this help message and exit\n\
             \x20 --recursive           If set, installs files recursively. Otherwise, just\n\
             \x20                       skips directories.\n\
             \x20 --preserve_symlinks   If set, a symlink will be installed as symlink.\n\
             \x20 --helpers_can_die     If set, die in isolated-functions.sh is enabled.\n\
             \x20                       Specifically this is used to keep compatible dodir's\n\
             \x20                       behavior.\n\
             \x20 --distdir DISTDIR     Path to the actual distdir.\n\
             \x20 --insoptions INSOPTIONS\n\
             \x20                       Options passed to `install` command for installing a\n\
             \x20                       file.\n\
             \x20 --diroptions DIROPTIONS\n\
             \x20                       Options passed to `install` command for installing a\n\
             \x20                       dir.\n\
             \x20 --strict_option       If set True, abort if insoptions/diroptions contains\n\
             \x20                       an option which cannot be interpreted by this script,\n\
             \x20                       instead of fallback to execute `install` command.\n\
             \x20 --enable_copy_xattr   Copies xattrs, if set True\n\
             \x20 --xattr_exclude XATTR_EXCLUDE\n\
             \x20                       White space delimited glob pattern to exclude xattr\n\
             \x20                       copy.Used only if --enable_xattr_copy is set.\n\
             \x20 --helper HELPER       Name of helper.\n\
             \x20 --dest DEST           Destination where the files are installed.\n",
        );
    }

    /// `--insoptions=-h` prints the install-option parser's help (same
    /// probe setup; real exits 0 from `_parse_install_options`).
    #[test]
    fn install_option_help_matches_real_byte_for_byte() {
        let out = helper(&[
            "--helper=doins",
            "--dest=/tmp/doins-probe-dest",
            "--insoptions=-h",
            "--",
            "file.txt",
        ]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "usage: doins.py [-h] [-g GROUP] [-o OWNER] [-m MODE] [-p]\n\
             \n\
             options:\n\
             \x20 -h, --help            show this help message and exit\n\
             \x20 -g, --group GROUP\n\
             \x20 -o, --owner OWNER\n\
             \x20 -m, --mode MODE\n\
             \x20 -p, --preserve-timestamps\n",
        );
    }

    /// A bad owner/group name is an argparse type error: exit 2 with
    /// real's text (same probe setup).
    #[test]
    fn install_option_type_error_exits_2_with_reals_text() {
        let out = helper(&[
            "--helper=doins",
            "--dest=/tmp/doins-probe-dest",
            "--insoptions=-o nosuchuser123",
            "--",
            "file.txt",
        ]);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "usage: doins.py [-h] [-g GROUP] [-o OWNER] [-m MODE] [-p]\n\
             doins.py: error: argument -o/--owner: invalid _parse_user value: 'nosuchuser123'\n",
        );
        let out = helper(&[
            "--helper=doins",
            "--dest=/tmp/doins-probe-dest",
            "--insoptions=-g nosuchgroup123",
            "--",
            "file.txt",
        ]);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).ends_with(
                "doins.py: error: argument -g/--group: invalid _parse_group value: 'nosuchgroup123'\n"
            ),
            "{:?}",
            String::from_utf8_lossy(&out.stderr),
        );
    }

    // ---- install-option forms (probed live, see the task brief) -----

    fn parsed_of(options: &str) -> InstallParsed {
        match parse_install_options("doins.py", options.as_bytes(), false, "doins") {
            InstallOutcome::Route(InstallRoute::InProcess(p)) => p,
            other => panic!(
                "{options:?} should parse in-process, got {}",
                match other {
                    InstallOutcome::Printed(c) => format!("exit {c}"),
                    InstallOutcome::Route(_) => "subprocess".to_string(),
                    InstallOutcome::Uncaught(l) => String::from_utf8_lossy(&l).into_owned(),
                }
            ),
        }
    }

    /// Every argparse form real accepts (probed with a harness
    /// importing real's parser logic).
    #[test]
    fn install_option_forms_match_real() {
        let full = |owner, group, mode, p| InstallParsed {
            owner,
            group,
            mode,
            preserve_timestamps: p,
        };
        for (opts, want) in [
            ("-m0600", full(-1, -1, Some(0o600), false)),
            ("-m 0600", full(-1, -1, Some(0o600), false)),
            ("--mode=0600", full(-1, -1, Some(0o600), false)),
            ("--mode 0600", full(-1, -1, Some(0o600), false)),
            ("--mo 0600", full(-1, -1, Some(0o600), false)),
            ("-p", full(-1, -1, Some(0o755), true)),
            ("-pm0600", full(-1, -1, Some(0o600), true)),
            ("-o 0 -g 0", full(0, 0, Some(0o755), false)),
            ("-o root -g root", full(0, 0, Some(0o755), false)),
            ("--own root", full(0, -1, Some(0o755), false)),
            ("-o0", full(0, -1, Some(0o755), false)),
            ("-g0", full(-1, 0, Some(0o755), false)),
            ("--group=0", full(-1, 0, Some(0o755), false)),
            ("--preserve-timestamps", full(-1, -1, Some(0o755), true)),
            ("-pp", full(-1, -1, Some(0o755), true)),
            ("--mode=0o755", full(-1, -1, Some(0o755), false)),
            ("", full(-1, -1, Some(0o755), false)),
        ] {
            assert_eq!(parsed_of(opts), want, "{opts:?}");
        }
        // `-mp`: `-m` eats `p` as its value, `int('p', 8)` fails, so
        // the mode is None and the run falls back to `install(1)`.
        match parse_install_options("doins.py", b"-mp", false, "doins") {
            InstallOutcome::Route(InstallRoute::Subprocess(split)) => {
                assert_eq!(split, vec![b"-mp".to_vec()]);
            }
            other => panic!(
                "-mp should fall back, got {}",
                match other {
                    InstallOutcome::Printed(c) => format!("exit {c}"),
                    _ => "in-process".to_string(),
                }
            ),
        }
        // Unknown options become `remaining`, in order.
        for (opts, want) in [
            ("-m 0644 -v", vec![b"-v".to_vec()]),
            ("-pv", vec![b"-v".to_vec()]),
            ("-vp", vec![b"-vp".to_vec()]),
            ("-- -v", vec![b"--".to_vec(), b"-v".to_vec()]),
        ] {
            match parse_install_options("doins.py", opts.as_bytes(), false, "doins") {
                InstallOutcome::Route(InstallRoute::Subprocess(split)) => {
                    assert_eq!(
                        split,
                        shlex_split(opts.as_bytes()).unwrap(),
                        "{opts:?}: the fallback keeps the raw split tokens"
                    );
                    let _ = want;
                }
                other => panic!(
                    "{opts:?} should fall back, got {}",
                    match other {
                        InstallOutcome::Printed(c) => format!("exit {c}"),
                        _ => "in-process".to_string(),
                    }
                ),
            }
        }
        // ... with `--strict_option` they refuse instead (exit 1).
        match parse_install_options("doins.py", b"-m 0644 -v", true, "doins") {
            InstallOutcome::Printed(1) => {}
            other => panic!(
                "strict unknown should exit 1, got {}",
                match other {
                    InstallOutcome::Printed(c) => format!("exit {c}"),
                    _ => "a route".to_string(),
                }
            ),
        }
    }

    /// `int(x, 8)` semantics (probed: `int(s, 8)` on the samples).
    #[test]
    fn parse_mode_matches_python_int_with_base_8() {
        for (s, want) in [
            ("755", Some(0o755)),
            ("0755", Some(0o755)),
            (" 755 ", Some(0o755)),
            ("755\n", Some(0o755)),
            ("+755", Some(0o755)),
            ("-755", Some(-0o755)),
            ("0o755", Some(0o755)),
            ("0O755", Some(0o755)),
            ("7_5_5", Some(0o755)),
            ("0600", Some(0o600)),
            ("", None),
            ("  ", None),
            ("89", None),
            ("0x755", None),
            ("foo", None),
            ("p", None),
            ("--", None),
        ] {
            assert_eq!(parse_mode(s), want, "{s:?}");
        }
    }

    /// `/etc/passwd` and `/etc/group` parsing (no NSS): first match
    /// wins, malformed lines are skipped.
    #[test]
    fn id_file_parsing_matches_first_match_wins() {
        let passwd = b"root:x:0:0:root:/root:/bin/bash\n\
            vivo:x:1000:100::/home/vivo:/bin/bash\n\
            badline\n\
            ghost:x:notanumber:1::/:/bin/sh\n";
        assert_eq!(parse_id_file(passwd, "root"), Some(0));
        assert_eq!(parse_id_file(passwd, "vivo"), Some(1000));
        assert_eq!(parse_id_file(passwd, "ghost"), None);
        assert_eq!(parse_id_file(passwd, "nobody"), None);
        let group = b"root:x:0:\nusers:x:100:\nportage:x:250:\n";
        assert_eq!(parse_id_file(group, "root"), Some(0));
        assert_eq!(parse_id_file(group, "users"), Some(100));
        assert_eq!(parse_id_file(group, "portage"), Some(250));
        // A numeric name resolves as a name first (real checks the
        // name before `int()`).
        let numeric = b"1001:x:1001:1001::/:/bin/sh\n";
        assert_eq!(parse_id_file(numeric, "1001"), Some(1001));
    }

    /// The xattr excluder: whitespace-split globs, fnmatch semantics
    /// (probed against `fnmatch.fnmatch` via real's cases).
    #[test]
    fn xattr_excluder_matches_real() {
        assert!(!xattr_excluded("", b"user.keep"));
        assert!(!xattr_excluded("   ", b"user.keep"));
        assert!(xattr_excluded("user.drop", b"user.drop"));
        assert!(!xattr_excluded("user.drop", b"user.keep"));
        // The `make.globals` default exclude list (a phase-defaults
        // oracle case pins this end to end).
        let globals = "bcachefs.* bcachefs_effective.* btrfs.* security.evm security.ima \
            security.selinux system.nfs4_acl user.apache_handler user.Beagle.* \
            user.dublincore.* user.mime_encoding user.xdg.*";
        assert!(xattr_excluded(globals, b"user.Beagle.foo"));
        assert!(xattr_excluded(globals, b"user.xdg.bar"));
        assert!(xattr_excluded(globals, b"security.ima"));
        assert!(xattr_excluded(globals, b"bcachefs.anything.at.all"));
        assert!(!xattr_excluded(globals, b"user.keep"));
        // `*` crosses `/` (posix fnmatch), `?` is one byte, classes
        // and `!` negation work.
        assert!(xattr_excluded("user.*", b"user.a/b"));
        assert!(xattr_excluded("user.???p", b"user.keep"));
        assert!(!xattr_excluded("user.???p", b"user.keeps"));
        assert!(xattr_excluded("user.[a-z]eep", b"user.keep"));
        assert!(xattr_excluded("user.[!a-c]eep", b"user.keep"));
        assert!(!xattr_excluded("user.[!a-c]eep", b"user.beep"));
        // Probed against real `fnmatch.fnmatchcase` (Python 3.14): a
        // backslash is literal, only `!` negates (`^` is a member), a
        // `]` first in a class is literal, `-` last is literal, an
        // inverted range matches nothing, an unterminated `[` is
        // literal.
        assert!(xattr_excluded("a\\*b", b"a\\xb"));
        assert!(!xattr_excluded("a\\*b", b"a*b"));
        assert!(xattr_excluded("[^a]", b"^"));
        assert!(!xattr_excluded("[^a]", b"b"));
        assert!(xattr_excluded("[]a]", b"]"));
        assert!(!xattr_excluded("[!]a]", b"]"));
        assert!(xattr_excluded("[a-]", b"-"));
        assert!(!xattr_excluded("[z-a]", b"m"));
        assert!(xattr_excluded("[a", b"[a"));
        assert!(xattr_excluded("[\\]", b"\\"));
    }

    /// `shlex.split` as real runs it on `--insoptions` (each expected
    /// value probed with Python 3.14's `shlex.split`).
    #[test]
    fn shlex_split_matches_python() {
        let ok = |s: &[u8]| shlex_split(s).unwrap();
        assert_eq!(
            ok(b"-m \"06\\44\""),
            vec![b"-m".to_vec(), b"06\\44".to_vec()]
        );
        assert_eq!(ok(b"-m \"a\\\"b\""), vec![b"-m".to_vec(), b"a\"b".to_vec()]);
        assert_eq!(ok(b"-m \"a\\\\b\""), vec![b"-m".to_vec(), b"a\\b".to_vec()]);
        assert_eq!(ok(b"-m a\\\nb"), vec![b"-m".to_vec(), b"a\nb".to_vec()]);
        assert_eq!(ok(b"-m 'a\\b'"), vec![b"-m".to_vec(), b"a\\b".to_vec()]);
        assert_eq!(ok(b"-m \"\""), vec![b"-m".to_vec(), Vec::new()]);
        assert_eq!(ok(b"-m a\"b c\"d"), vec![b"-m".to_vec(), b"ab cd".to_vec()]);
        assert_eq!(ok(b"-m #x"), vec![b"-m".to_vec(), b"#x".to_vec()]);
        assert_eq!(
            shlex_split(b"-m \"unterminated"),
            Err("No closing quotation")
        );
        assert_eq!(shlex_split(b"-m trailing\\"), Err("No escaped character"));
        assert_eq!(shlex_split(b"-m \"ab\\"), Err("No escaped character"));
    }

    /// An unterminated quote in `--insoptions` is real's uncaught
    /// `ValueError` from `shlex.split`, raised in `_InstallRunner`
    /// before `--dest` exists (probed live: rc 1, last traceback line
    /// `ValueError: No closing quotation`, no dest dir).
    #[test]
    fn unterminated_insoptions_quote_is_reals_value_error() {
        let tmp = TempDir::new("doins-shlex");
        let src = tmp.path().join("f");
        std::fs::write(&src, "x\n").unwrap();
        let dest = tmp.path().join("out");
        let out = helper(&[
            "--insoptions=-m \"0644",
            "--helper=doins",
            &format!("--dest={}", dest.display()),
            "--",
            src.to_str().unwrap(),
        ]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).ends_with("ValueError: No closing quotation\n"),
            "{out:?}"
        );
        assert!(!dest.exists());
    }

    /// The `Failed to copy file` Namespace keeps real's field order
    /// and puts each id in its own field.
    #[test]
    fn install_namespace_repr_names_each_id() {
        let p = InstallParsed {
            owner: 7,
            group: 5,
            mode: Some(0o644),
            preserve_timestamps: true,
        };
        assert_eq!(
            String::from_utf8(p.namespace_repr()).unwrap(),
            "Namespace(group=5, owner=7, mode=420, preserve_timestamps=True)"
        );
    }

    /// `0 if not any_failure and any_success else 1`.
    #[test]
    fn rc_rule() {
        assert_eq!(combine_rc(true, false), 0);
        assert_eq!(combine_rc(false, false), 1);
        assert_eq!(combine_rc(true, true), 1);
        assert_eq!(combine_rc(false, true), 1);
    }

    /// An empty run installs nothing and fails (probed: `doins.py
    /// --helper=doins --dest=...` with no sources exits 1 silently).
    #[test]
    fn empty_sources_exit_1_silently() {
        let tmp = TempDir::new("doins-empty");
        let dest = tmp.path().join("dest");
        let out = helper(&["--helper=doins", &format!("--dest={}", dest.display())]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(out.stderr.is_empty(), "{:?}", out.stderr);
        assert!(dest.is_dir(), "the dest is still created");
    }

    // ---- D7 write guard ----------------------------------------------

    /// The guard root is `$D` when set, else `--dest`; the canonical
    /// parent must lie under it.
    #[test]
    fn guard_root_and_parent_checks() {
        let tmp = TempDir::new("doins-guard");
        let root = tmp.path().join("d");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let g = Guard::from_anchor(&root);
        assert!(
            g.check_parent(root.join("sub").join("f").to_str().unwrap().as_bytes())
                .is_ok()
        );
        assert!(
            g.check_parent(root.join("new").to_str().unwrap().as_bytes())
                .is_ok()
        );
        assert!(g.check_parent(b"/etc/passwd").is_err());
        assert!(
            g.check_parent(tmp.path().join("sibling").to_str().unwrap().as_bytes())
                .is_err()
        );
    }

    /// A symlink inside the guarded tree pointing outside refuses the
    /// write through it, while creating the link itself is an
    /// inside-write and allowed.
    #[test]
    fn guard_refuses_writes_through_a_dest_symlink() {
        let tmp = TempDir::new("doins-guard-link");
        let root = tmp.path().join("d");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let g = Guard::from_anchor(&root);
        let link = root.join("link");
        // Creating/replacing the link itself is fine (its parent is
        // inside).
        assert!(
            g.check_parent(link.to_str().unwrap().as_bytes()).is_ok(),
            "the link itself lives inside"
        );
        // Anything through it is outside.
        assert!(
            g.check_parent(link.join("f").to_str().unwrap().as_bytes())
                .is_err(),
            "writing through the link escapes"
        );
        assert!(
            g.check_resolved(link.to_str().unwrap().as_bytes()).is_err(),
            "mutating the link target escapes"
        );
    }

    /// End to end through a child process: `doins -r` over a dest tree
    /// containing a symlink to the outside is refused (rc 1, the path
    /// is named) and the outside tree is untouched.
    #[test]
    fn guard_refuses_a_recursive_write_through_a_dest_symlink() {
        let shm = PathBuf::from("/dev/shm");
        if !shm.is_dir() {
            eprintln!("skipped: no /dev/shm tmpfs");
            return;
        }
        let tmp = TempDir::new_in(&shm, "doins-guard-e2e");
        let src = tmp.path().join("src");
        let ed = tmp.path().join("ed");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(src.join("tree/sub")).unwrap();
        std::fs::write(src.join("tree/sub/file.txt"), b"data\n").unwrap();
        // The symlink sits where the run would write through it:
        // `doins -r tree` installs `tree/sub/file.txt` via
        // `<dest>/tree/sub`, which already exists here as a link to
        // the outside.
        let dest = ed.join("usr/lib/probe");
        std::fs::create_dir_all(dest.join("tree")).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("tree/sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let out = Command::new(portuale_exe())
            .args(["__helper", "python", "doins.py"])
            .args([
                "--recursive".to_string(),
                "--preserve_symlinks".to_string(),
                "--helper=doins".to_string(),
                format!("--dest={}", dest.display()),
                "--".to_string(),
                "tree".to_string(),
            ])
            .current_dir(&src)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .env("D", &ed)
            .output()
            .expect("portuale __helper spawns");
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("refusing to write outside") && err.contains("sub"),
            "the refusal names the path: {err}"
        );
        assert!(
            std::fs::read_dir(&outside).unwrap().next().is_none(),
            "nothing was written outside"
        );
    }

    // ---- oracle (one test per case dir) -------------------------------

    /// pmtest's `fixtures/helpers/doins/` (the oracle generated by
    /// running the real wrapper; Rust reads it through the `fixtures`
    /// symlink, so no checkout or Python is needed).
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/doins")
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

    /// `materialise.py` ported to Rust (so the test needs no python3):
    /// entries are created in manifest order (parents first), then a
    /// deepest-first pass applies modes and pins every mtime to MTIME.
    fn materialise(manifest: &Path, dest: &Path, src: &Path, dist: &Path) {
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
                b'h' => {
                    assert!(
                        extra.len() == 2 && extra[0] == "=>",
                        "{}: hardlink needs '=> path': {line}",
                        manifest.display()
                    );
                    (Some(unesc(extra[1])), None)
                }
                _ => panic!("{}: bad type: {line}", manifest.display()),
            };
            let rel = unesc(qpath);
            assert!(
                !rel.starts_with(b"/")
                    && rel
                        .split(|b| *b == b'/')
                        .all(|c| !c.is_empty() && c != b"." && c != b".."),
                "{}: path must be relative without . or ..: {line}",
                manifest.display()
            );
            let full = dest.join(OsStr::from_bytes(&rel));
            let parent = full.parent().unwrap().to_path_buf();
            assert!(
                made.contains(&parent),
                "{}: parent of {qpath:?} is not an explicit d entry",
                manifest.display()
            );
            let sub = |t: Vec<u8>| {
                let mut v = t;
                let s = src.as_os_str().as_bytes().to_vec();
                let d = dist.as_os_str().as_bytes().to_vec();
                // `@SRC@`/`@DISTDIR@` substitution (bytes).
                let mut out = Vec::new();
                let mut i = 0;
                while i < v.len() {
                    if v[i..].starts_with(b"@SRC@") {
                        out.extend_from_slice(&s);
                        i += 5;
                    } else if v[i..].starts_with(b"@DISTDIR@") {
                        out.extend_from_slice(&d);
                        i += 9;
                    } else {
                        out.push(v[i]);
                        i += 1;
                    }
                }
                let _ = &mut v;
                out
            };
            match typ {
                b'd' => {
                    std::fs::create_dir(&full).unwrap();
                    made.push(full.clone());
                }
                b'f' => {
                    std::fs::write(&full, content.unwrap_or_default()).unwrap();
                }
                b'l' => {
                    let target = sub(target.unwrap());
                    std::os::unix::fs::symlink(OsStr::from_bytes(&target), &full).unwrap();
                }
                b'h' => {
                    let target = sub(target.unwrap());
                    let other = if target.starts_with(b"/") {
                        PathBuf::from(OsStr::from_bytes(&target))
                    } else {
                        dest.join(OsStr::from_bytes(&target))
                    };
                    std::fs::hard_link(&other, &full).unwrap();
                }
                _ => unreachable!(),
            }
            entries.push((typ, mode, full));
        }
        // Deepest first: modes (never symlinks; hardlinks share their
        // target's inode and are skipped) and the pinned mtime.
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
            if *typ == b'h' {
                continue;
            }
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

    /// Apply a case's `xattrs` file (`<qpath> <name> <hex-value>`) to
    /// the materialised sources.
    fn apply_xattrs(case_dir: &Path, src: &Path) {
        let p = case_dir.join("xattrs");
        if !p.is_file() {
            return;
        }
        for line in std::fs::read_to_string(&p).unwrap().lines() {
            if line.is_empty() {
                continue;
            }
            let t: Vec<&str> = line.split(' ').collect();
            assert_eq!(t.len(), 3, "{}: bad xattrs line: {line}", p.display());
            let path = src.join(OsStr::from_bytes(&unesc(t[0])));
            let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            let n = std::ffi::CString::new(t[1]).unwrap();
            let v = hex_decode(t[2]);
            let r =
                unsafe { libc::setxattr(c.as_ptr(), n.as_ptr(), v.as_ptr().cast(), v.len(), 0) };
            assert_eq!(r, 0, "setxattr {} {}", path.display(), t[1]);
        }
    }

    /// Decode one `args` line (`\\` → `\`, `\n` → newline).
    fn decode_arg(line: &str) -> Vec<u8> {
        let b = line.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 1 < b.len() {
                match b[i + 1] {
                    b'\\' => {
                        out.push(b'\\');
                        i += 2;
                    }
                    b'n' => {
                        out.push(b'\n');
                        i += 2;
                    }
                    _ => {
                        out.push(b'\\');
                        i += 1;
                    }
                }
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    struct Staged {
        tmp: TempDir,
        src: PathBuf,
        ed: PathBuf,
        args: Vec<OsString>,
        envs: Vec<(String, String)>,
    }

    /// Stage a case on `/dev/shm` with `materialise.py`'s semantics
    /// (python3 must not be needed). Placeholders are substituted:
    /// `<SCRATCH>` (the stage), `<CHECKOUT>` never occurs in doins
    /// args (checked: no case's `args` names it), `newins.RAND` (the
    /// wrapper's mktemp suffix, normalised by the generator).
    fn stage_case(case: &str) -> Option<Staged> {
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            eprintln!("skipped {case}: no /dev/shm tmpfs for the oracle");
            return None;
        }
        let case_dir = fixtures().join(case);
        let tmp = TempDir::new_in(shm, &format!("doins-{case}"));
        let top = tmp.path().join(format!("stage-{case}"));
        let (src, ed, t, dist) = (
            top.join("src"),
            top.join("ed"),
            top.join("t"),
            top.join("dist"),
        );
        for d in [&src, &ed, &t, &dist] {
            std::fs::create_dir_all(d).unwrap();
        }
        materialise(&case_dir.join("in.manifest"), &src, &src, &dist);
        if case_dir.join("ed.manifest").is_file() {
            materialise(&case_dir.join("ed.manifest"), &ed, &src, &dist);
        }
        if case_dir.join("dist.manifest").is_file() {
            materialise(&case_dir.join("dist.manifest"), &dist, &src, &dist);
        }
        apply_xattrs(&case_dir, &src);
        let scratch = tmp.path().as_os_str().as_bytes().to_vec();
        let mut newins = false;
        let args: Vec<OsString> = std::fs::read_to_string(case_dir.join("args"))
            .unwrap()
            .lines()
            .map(|l| {
                let b = decode_arg(l);
                let mut out = Vec::new();
                let mut i = 0;
                while i < b.len() {
                    if b[i..].starts_with(b"<SCRATCH>") {
                        out.extend_from_slice(&scratch);
                        i += 9;
                    } else if b[i..].starts_with(b"newins.RAND") {
                        newins = true;
                        out.extend_from_slice(b"newins.test");
                        i += 11;
                    } else {
                        out.push(b[i]);
                        i += 1;
                    }
                }
                OsString::from_vec(out)
            })
            .collect();
        if newins {
            // The `newins` wrapper copies the source to a mktemp dir
            // under `$T` before invoking doins: mirror that `cp` (the
            // installed mode comes from insoptions, not from this
            // copy).
            let dir = t.join("newins.test");
            std::fs::create_dir_all(&dir).unwrap();
            let data = std::fs::read(src.join("tree/file.txt")).unwrap();
            std::fs::write(dir.join("renamed.txt"), &data).unwrap();
        }
        let sub = |v: &str| {
            v.replace("@ED@", ed.to_str().unwrap())
                .replace("@T@", t.to_str().unwrap())
                .replace("@DISTDIR@", dist.to_str().unwrap())
                .replace("@CHECKOUT@", "/nonexistent-checkout")
                .replace("@PYSHIM@", "/nonexistent-pyshim")
        };
        let mut envs = Vec::new();
        for line in std::fs::read_to_string(case_dir.join("env"))
            .unwrap()
            .lines()
        {
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').unwrap();
            // Empty value means unset.
            if v.is_empty() {
                continue;
            }
            envs.push((k.to_string(), sub(v)));
        }
        Some(Staged {
            tmp,
            src,
            ed,
            args,
            envs,
        })
    }

    /// `llistxattr` (no-follow), for dumping symlinks.
    fn llistxattr(path: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
        let c = std::ffi::CString::new(path).map_err(|_| libc::EINVAL)?;
        let n = unsafe { libc::llistxattr(c.as_ptr(), std::ptr::null_mut(), 0) };
        if n < 0 {
            let e = last_errno();
            if e == libc::ENOTSUP || e == libc::EOPNOTSUPP {
                return Ok(Vec::new());
            }
            return Err(e);
        }
        let mut buf = vec![0u8; n as usize];
        let m = unsafe { libc::llistxattr(c.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if m < 0 {
            return Err(last_errno());
        }
        Ok(buf[..m as usize]
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_vec())
            .collect())
    }

    /// `lgetxattr` (no-follow), for dumping symlinks.
    fn lgetxattr(path: &[u8], name: &[u8]) -> Result<Vec<u8>, i32> {
        let c = std::ffi::CString::new(path).map_err(|_| libc::EINVAL)?;
        let n = std::ffi::CString::new(name).map_err(|_| libc::EINVAL)?;
        let len = unsafe { libc::lgetxattr(c.as_ptr(), n.as_ptr(), std::ptr::null_mut(), 0) };
        if len < 0 {
            return Err(last_errno());
        }
        let mut buf = vec![0u8; len as usize];
        let got =
            unsafe { libc::lgetxattr(c.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if got < 0 {
            return Err(last_errno());
        }
        buf.truncate(got as usize);
        Ok(buf)
    }

    /// `%XX` escaping for dump paths (bytes outside
    /// `[A-Za-z0-9._/+-]`, uppercase hex).
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

    /// The D3 tree dump in `out.manifest`'s exact format, with `mtime`
    /// on every regular file (the comparison strips it unless the
    /// oracle line carries one) and `hlink` groups computed (doins.py
    /// never preserves hardlinks, so any group is a regression).
    fn dump_tree(ed: &Path, stage: &Path) -> Vec<String> {
        struct Entry {
            qpath: String,
            line: String,
            is_file: bool,
            key: Option<(u64, u64)>,
        }
        let mut entries: Vec<Entry> = Vec::new();
        fn walk(dir: &Path, ed: &Path, stage: &Path, entries: &mut Vec<Entry>) {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            for n in names {
                let p = dir.join(&n);
                let rel = p.strip_prefix(ed).unwrap();
                let rel_b = rel.as_os_str().as_bytes();
                let qpath = qesc(rel_b);
                let md = std::fs::symlink_metadata(&p).unwrap();
                let xattrs = {
                    let mut xs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
                    // Symlinks are listed without following (like the
                    // oracle's dump): a preserved link must not show
                    // its target's attributes.
                    let is_link = md.file_type().is_symlink();
                    let list = if is_link {
                        llistxattr(p.as_os_str().as_bytes())
                    } else {
                        xattr_list(p.as_os_str().as_bytes())
                    };
                    if let Ok(list) = list {
                        for name in list {
                            // The dump records `user.*` only.
                            if !name.starts_with(b"user.") {
                                continue;
                            }
                            let v = if is_link {
                                lgetxattr(p.as_os_str().as_bytes(), &name)
                            } else {
                                xattr_get(p.as_os_str().as_bytes(), &name)
                            };
                            if let Ok(v) = v {
                                xs.push((name, v));
                            }
                        }
                    }
                    xs.sort();
                    let mut s = String::new();
                    for (name, v) in xs {
                        s.push_str(&format!(
                            " xattr:{}={}",
                            String::from_utf8_lossy(&name),
                            v.iter().map(|b| format!("{b:02x}")).collect::<String>()
                        ));
                    }
                    s
                };
                if md.file_type().is_symlink() {
                    let target = std::fs::read_link(&p).unwrap().into_os_string().into_vec();
                    // Scrub after escaping: the stage path is all
                    // unreserved bytes, so it survives `qesc`
                    // unchanged, while `<SCRATCH>` itself must stay
                    // literal (as the oracle shows it).
                    let scrubbed = qesc(&target).replace(stage.to_str().unwrap(), "<SCRATCH>");
                    entries.push(Entry {
                        qpath: qpath.clone(),
                        line: format!(
                            "l ---- {} {} - - {qpath} -> {scrubbed}{xattrs}",
                            md.uid(),
                            md.gid()
                        ),
                        is_file: false,
                        key: None,
                    });
                } else if md.is_dir() {
                    entries.push(Entry {
                        qpath: qpath.clone(),
                        line: format!(
                            "d {:04o} {} {} - - {qpath}{xattrs}",
                            md.mode() & 0o7777,
                            md.uid(),
                            md.gid()
                        ),
                        is_file: false,
                        key: None,
                    });
                    walk(&p, ed, stage, entries);
                } else if md.is_file() {
                    let data = std::fs::read(&p).unwrap();
                    use sha2::Digest;
                    let hex: String = sha2::Sha256::digest(&data)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    let mtime = md.mtime() * 1_000_000_000 + md.mtime_nsec() as i64;
                    entries.push(Entry {
                        qpath: qpath.clone(),
                        line: format!(
                            "f {:04o} {} {} {} sha256:{hex} {qpath} mtime={mtime}{xattrs}",
                            md.mode() & 0o7777,
                            md.uid(),
                            md.gid(),
                            data.len()
                        ),
                        is_file: true,
                        key: Some((md.dev(), md.ino())),
                    });
                } else {
                    panic!("unexpected file type: {}", p.display());
                }
            }
        }
        if ed.is_dir() {
            walk(ed, ed, stage, &mut entries);
        }
        // hlink groups by (dev, ino) in first-path order.
        let mut order: Vec<(u64, u64)> = Vec::new();
        for e in &entries {
            if e.is_file
                && let Some(k) = e.key
                && !order.contains(&k)
            {
                order.push(k);
            }
        }
        let mut counts = std::collections::HashMap::new();
        for e in &entries {
            if e.is_file
                && let Some(k) = e.key
            {
                *counts.entry(k).or_insert(0) += 1;
            }
        }
        let mut lines: Vec<(String, String)> = entries
            .into_iter()
            .map(|e| {
                let line = if e.is_file
                    && let Some(k) = e.key
                {
                    if counts[&k] > 1 {
                        let n = order.iter().position(|o| *o == k).unwrap();
                        format!("{} hlink=h{n}", e.line)
                    } else {
                        e.line
                    }
                } else {
                    e.line
                };
                (e.qpath, line)
            })
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        lines.into_iter().map(|(_, l)| l).collect()
    }

    /// Read an oracle file, preferring the `*.root.*` variant as root
    /// (every case runs as both user and root; the root files exist
    /// only when the root run differs).
    fn oracle_text(case_dir: &Path, base: &str, ext: &str, as_root: bool) -> String {
        let root_name = format!("{base}.root.{ext}");
        if as_root && case_dir.join(&root_name).is_file() {
            return std::fs::read_to_string(case_dir.join(&root_name)).unwrap();
        }
        let name = format!("{base}.{ext}");
        if case_dir.join(&name).is_file() {
            return std::fs::read_to_string(case_dir.join(&name)).unwrap();
        }
        String::new()
    }

    /// Compare one case against the oracle: rc, the significant
    /// stderr lines, and the dest tree dump. Returns false when
    /// skipped (no `/dev/shm`).
    fn check_oracle(case: &str) -> bool {
        let Some(st) = stage_case(case) else {
            return false;
        };
        let case_dir = fixtures().join(case);
        let as_root = unsafe { libc::geteuid() } == 0;
        let mut cmd = Command::new(portuale_exe());
        cmd.args(["__helper", "python", "doins.py"])
            .args(&st.args)
            .current_dir(&st.src)
            .env_clear()
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python");
        for (k, v) in &st.envs {
            cmd.env(k, v);
        }
        // The oracle ran under umask 022 (the generator's, and the
        // phase's: `ebuild.sh` sets it). Other tests in this binary
        // change the process umask, which a child would inherit, so
        // the child gets 022 explicitly.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::umask(0o022);
                Ok(())
            });
        }
        let out = cmd.output().expect("portuale __helper spawns");
        let rc = out.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let want_rc: i32 = oracle_text(&case_dir, "rc", "txt", as_root)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(rc, want_rc, "{case}: rc, stderr:\n{stderr}");
        // stderr: every significant line of the oracle must appear in
        // order. The die banner (` * ...`) is the bash wrapper's, not
        // doins.py's (the wrapper's `__helpers_die`, which our native
        // helper never prints), so both sides skip it: the comparison
        // drops got lines starting with ` * ` and only looks at
        // oracle lines that are doins.py's own (col-0, non-banner,
        // non-traceback-header).
        let stage_str = st.tmp.path().to_str().unwrap();
        let want_err =
            oracle_text(&case_dir, "stderr", "txt", as_root).replace("<SCRATCH>", stage_str);
        let got: Vec<&str> = stderr.lines().filter(|l| !l.starts_with(" * ")).collect();
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
        // The tree dump. Owner ids are environment-dependent (the
        // oracle records the generator's), so the expected uid/gid
        // columns are rewritten to this process's ids; everything else
        // (modes, contents, links, mtimes, xattrs) is exact. `mtime`
        // is recorded on every file but only compared when the oracle
        // line carries one (only `insopts-preserve` does: `-p` is the
        // sole path that stamps dest files).
        let euid = unsafe { libc::geteuid() };
        let egid = unsafe { libc::getegid() };
        let manifest_name = if as_root && case_dir.join("out.root.manifest").is_file() {
            "out.root.manifest"
        } else {
            "out.manifest"
        };
        let want_tree: Vec<String> = std::fs::read_to_string(case_dir.join(manifest_name))
            .unwrap()
            .lines()
            .map(|l| {
                let mut t: Vec<String> = l.split(' ').map(str::to_string).collect();
                if matches!(t.first().map(String::as_str), Some("f" | "d" | "l")) && t.len() > 3 {
                    t[2] = euid.to_string();
                    t[3] = egid.to_string();
                    t.join(" ")
                } else {
                    l.to_string()
                }
            })
            .collect();
        let mut got_tree = dump_tree(&st.ed, st.tmp.path());
        assert_eq!(
            got_tree.len(),
            want_tree.len(),
            "{case}: tree entry count differs\n got:\n{}\n want:\n{}",
            got_tree.join("\n"),
            want_tree.join("\n")
        );
        for (g, w) in got_tree.iter_mut().zip(want_tree.iter()) {
            if !w.contains("mtime=")
                && let Some(p) = g.find(" mtime=")
            {
                let end = g[p + 7..].find(' ').map(|e| p + 7 + e).unwrap_or(g.len());
                g.replace_range(p..end, "");
            }
            assert_eq!(g, w, "{case}: tree differs");
        }
        eprintln!("oracle {case}: ok");
        true
    }

    macro_rules! oracle_case {
        ($name:ident, $case:literal) => {
            #[test]
            fn $name() {
                check_oracle($case);
            }
        };
    }

    oracle_case!(oracle_files_no_r, "files-no-r");
    oracle_case!(oracle_dirs_only_no_r, "dirs-only-no-r");
    oracle_case!(oracle_recursive, "recursive");
    oracle_case!(oracle_newins, "newins");
    oracle_case!(oracle_insopts_mode, "insopts-mode");
    oracle_case!(oracle_insopts_preserve, "insopts-preserve");
    oracle_case!(oracle_diropts_mode, "diropts-mode");
    oracle_case!(oracle_owner_root, "owner-root");
    oracle_case!(oracle_unknown_lax, "unknown-lax");
    oracle_case!(oracle_unknown_strict, "unknown-strict");
    oracle_case!(oracle_dodoc_r, "dodoc-r");
    oracle_case!(oracle_doheader, "doheader");
    oracle_case!(oracle_doconfd, "doconfd");
    oracle_case!(oracle_xattr_exclude, "xattr-exclude");
    oracle_case!(oracle_missing_source, "missing-source");
    oracle_case!(oracle_die_eapi8, "die-eapi8");
    oracle_case!(oracle_nodie_eapi3, "nodie-eapi3");
    oracle_case!(oracle_distdir_deref, "distdir-deref");
    oracle_case!(oracle_eapi3_copy, "eapi3-copy");
    oracle_case!(oracle_empty_dir_r, "empty-dir-r");
    oracle_case!(oracle_phase_defaults_r, "phase-defaults-r");
    oracle_case!(oracle_phase_defaults_files, "phase-defaults-files");
    oracle_case!(oracle_phase_defaults_distfile, "phase-defaults-distfile");

    // ---- end to end: a real package whose src_install uses doins ----

    /// `dev-libs/packagepkg`'s `src_install` is `insinto` + `doins`
    /// (in-process, through the existing package test machinery).
    fn install_packagepkg(portage_tmpdir: &Path) {
        let ebuild = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild");
        let status = crate::ebuild_phases::run_commands(
            &ebuild,
            &["install"],
            Path::new("/"),
            portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            crate::ebuild_phases::ShellBackend::default(),
            &[],
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");
        let installed = portage_tmpdir
            .join("portage/dev-libs/packagepkg-1.0/image/usr/share/packagepkg/hello.txt");
        let contents = std::fs::read_to_string(&installed)
            .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
        assert_eq!(contents, "hello from packagepkg\n");
    }

    #[test]
    fn doins_installs_packagepkg_files_in_process() {
        let tmp = TempDir::new("doins-packagepkg");
        install_packagepkg(&tmp);
    }

    /// Re-run the in-process install in a child with no checkout and
    /// no real Python, so the native `doins.py` is the only thing that
    /// can install the file (the `gpkg.rs` no-checkout shape).
    #[test]
    fn doins_installs_packagepkg_files_without_the_checkout() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "helpers::doins::tests::doins_installs_packagepkg_files_in_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent/python")
            .output()
            .unwrap();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "rerun: {text}");
        assert!(text.contains("1 passed"), "rerun did not run: {text}");
    }
}
