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
