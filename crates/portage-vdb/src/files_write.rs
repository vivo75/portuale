//! `FilesDb`'s write side: the merge's `-MERGING-<pf>` entry, the
//! `counter` store, the preserved-libs registry and the config-protect
//! memory.
//!
//! The code `portuale::ebuild_merge` ran before, moved without changing a
//! syscall. Nothing is atomic and nothing is locked: [`FilesTxn`] applies
//! every call when it is made and [`WriteTxn::commit`] is only an ordering
//! point.
//!
//! The on-disk formats of the registry and the config memory live here
//! too, because the database backends store the parsed values and a
//! converter must read and write the `files` bytes: the registry is
//! Portage's `PreservedLibsRegistry.store()` JSON, the config memory its
//! `grabdict`/`writedict` `"path value\n"` lines.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::files::is_absent;
use crate::{Error, MetadataStamp};

/// The counter store under a root.
// Portage: lib/portage/const.py, CACHE_PATH
const COUNTER_PATH: &str = "var/cache/edb/counter";
/// The preserved-libs registry under a root.
const PRESERVED_LIBS_PATH: &str = "var/lib/portage/preserved_libs_registry";
/// The config-protect memory under a root.
const CONFIG_MEMORY_PATH: &str = "var/lib/portage/config";
/// The world file under a root.
// Portage: lib/portage/const.py, WORLD_FILE
const WORLD_PATH: &str = "var/lib/portage/world";
/// The world-sets file under a root.
const WORLD_SETS_PATH: &str = "var/lib/portage/world_sets";

fn mtime_ns_of(st: &fs::Metadata) -> i128 {
    st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128
}

/// `body` with every `#dir_mtime=` line removed and a final newline.
fn without_stamp(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for line in body.split_inclusive(|&b| b == b'\n') {
        if !line.starts_with(b"#dir_mtime=") {
            out.extend_from_slice(line);
        }
    }
    if !out.is_empty() && out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    out
}

/// `std::fs::create_dir_all` of `path`'s parent, error on the parent.
/// Portage's `lockdir(<vdb>)`: a blocking exclusive `flock(2)` on
/// `<parent>/.<basename>.portage_lockfile`, unlinked again on release.
/// Because a waiter may hold an fd on the removed inode, the acquirer
/// re-checks that the path still names the file it locked and starts over
/// when it does not. Scoped here to one counter tick.
// Portage: lib/portage/locks.py, lockdir
struct VdbLock {
    _file: fs::File,
    path: PathBuf,
}

impl Drop for VdbLock {
    /// Unlink while still holding the lock, then the fd closes and the
    /// lock is released.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A minimal JSON string-literal reader for the registry: narrow by
/// design, not a general parser.
fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{C}'),
                'u' => {
                    let hex: String = (0..4).map(|_| chars.next()).collect::<Option<String>>()?;
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

fn skip_json_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

fn parse_json_string_array(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Option<Vec<String>> {
    skip_json_ws(chars);
    if chars.next()? != '[' {
        return None;
    }
    let mut out = Vec::new();
    skip_json_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Some(out);
    }
    loop {
        skip_json_ws(chars);
        out.push(parse_json_string(chars)?);
        skip_json_ws(chars);
        match chars.next()? {
            ',' => continue,
            ']' => return Some(out),
            _ => return None,
        }
    }
}

fn json_quote(s: &str) -> String {
    // Portage's `json.dumps(..., ensure_ascii=False)` quoting.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
