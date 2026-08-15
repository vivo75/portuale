//! `FilesDb`: the historic `var/db/pkg` directory tree.
//!
//! Per-entry and `aux` reads keep the same `openat`/`statx` sequence and
//! the same in-process memo as before. The merge's writes are in
//! `files_write.rs`. Every method not moved yet returns
//! [`Error::Unsupported`] naming the plan step that moves it.

use crate::{Error, MetadataStamp};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

/// The VDB directory under a root, joined without canonicalising.
pub(crate) const VDB_PATH: &str = "var/db/pkg";

/// `st_mtime` of `p` in nanos, 0 when missing or unreadable: the
/// invalidation signal for category-scoped caches (every vdb mutation
/// replaces or removes a package dir).
fn dir_mtime_nanos(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64)
}

/// A read error that means "no such entry or file".
pub(crate) fn is_absent(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// The per-key fallback read: the raw file whitespace-normalised, with an
/// absent file as `""`. Invalid UTF-8 decodes lossy, matching Portage's
/// `errors="replace"`.
// Portage: lib/portage/vartree.py, _aux_get
fn read_vdb_file(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| normalise_aux_bytes(&bytes))
        .unwrap_or_default()
}

/// The bytes of one field file as `aux_get` serves them: lossy UTF-8,
/// whitespace-joined (shared with the SQLite backend).
pub(crate) fn normalise_aux_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `value` is a `SLOT` Portage's `aux_get` keeps instead of
/// translating to `"0"`: `slot(/slot)?` with `slot = [\w][\w+.-]*`. Empty
/// is *not* invalid: a missing/empty `SLOT` stays `""`.
// Portage: lib/portage/versions.py, _valid_slot
fn is_valid_aux_slot(value: &str) -> bool {
    fn is_slot(s: &str) -> bool {
        let mut chars = s.bytes();
        match chars.next() {
            Some(b) if b.is_ascii_alphanumeric() || b == b'_' => {}
            _ => return false,
        }
        chars.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'))
    }
    match value.split_once('/') {
        Some((main, sub)) => is_slot(main) && is_slot(sub),
        None => is_slot(value),
    }
}

/// Portage's invalid-`SLOT` → `"0"` translation, for present (non-empty)
/// values only. The memoised value is the translated one (`"0"` is valid).
// Portage: lib/portage/vartree.py, aux_get
pub(crate) fn translate_aux_slot(key: &str, value: String) -> String {
    if key == "SLOT" && !value.is_empty() && !is_valid_aux_slot(&value) {
        return "0".to_string();
    }
    value
}

/// For `owners` on every backend: each distinct normal form (one leading
/// `/` stripped) of `paths`, with the first input path that has it.
pub(crate) fn owner_wanted<'a>(paths: &[&'a [u8]]) -> HashMap<&'a [u8], &'a [u8]> {
    let mut m: HashMap<&[u8], &[u8]> = HashMap::new();
    for p in paths {
        m.entry(p.strip_prefix(b"/").unwrap_or(p)).or_insert(p);
    }
    m
}
