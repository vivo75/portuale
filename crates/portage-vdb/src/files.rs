//! `FilesDb`: the historic `var/db/pkg` directory tree.
//!
//! Per-entry and `aux` reads keep the same `openat`/`statx` sequence and
//! the same in-process memo as before. The merge's writes are in
//! `files_write.rs`. Every method not moved yet returns
//! [`Error::Unsupported`] naming the plan step that moves it.

use crate::types::DIR_MODE_MASK;
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryKey, Error, METADATA_FILE_FIELDS,
    METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, Result, Snapshot, World, WorldSets,
    in_metadata_file,
};
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

/// Parse and validate a consolidated `metadata` snapshot: `#format=`
/// must match and `#dir_mtime=` must equal the dir mtime, else `None`.
/// A truncated write lacks the stamp (written last) and is rejected.
// Portage: lib/portage/vartree.py, _read_metadata_file
fn read_metadata_file(path: &Path, dir_mtime_ns: i128) -> Option<HashMap<String, String>> {
    let raw = fs::read_to_string(path).ok()?;
    let (result, dir_mtime) = parse_metadata_text(&raw)?;
    if dir_mtime != Some(dir_mtime_ns) {
        return None;
    }
    Some(result)
}

/// The text half of [`read_metadata_file`], shared with the SQLite
/// backend: the parsed fields and the `#dir_mtime=` value when present.
pub(crate) fn parse_metadata_text(raw: &str) -> Option<(HashMap<String, String>, Option<i128>)> {
    let mut result: HashMap<String, String> = HashMap::new();
    let mut version: Option<u32> = None;
    let mut dir_mtime: Option<i128> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(v) = rest.strip_prefix("format=") {
                version = Some(v.parse::<u32>().ok()?);
                if version != Some(METADATA_FILE_FORMAT_VERSION) {
                    return None;
                }
            } else if let Some(v) = rest.strip_prefix("dir_mtime=") {
                dir_mtime = Some(v.parse::<i128>().ok()?);
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            result.insert(k.to_string(), v.to_string());
        }
    }
    version?;
    Some((result, dir_mtime))
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

/// The claims of one entry's `CONTENTS` text: a line whose first two
/// words are a kind (`obj`, `sym`, `dir`, `dev`, `fif`, `bin`) and a path
/// claims that path; one leading `/` is ignored on both sides. Each claim
/// pairs the first of `paths` it matches with `key`.
pub(crate) fn claim_paths(
    text: &str,
    key: &EntryKey,
    wanted: &HashMap<&[u8], &[u8]>,
    out: &mut Vec<(Vec<u8>, EntryKey)>,
) {
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let path = match (words.next(), words.next()) {
            (Some("obj" | "sym" | "dir" | "dev" | "fif" | "bin"), Some(path)) => {
                path.strip_prefix('/').unwrap_or(path)
            }
            _ => continue,
        };
        if let Some(p) = wanted.get(path.as_bytes()) {
            out.push((p.to_vec(), key.clone()));
        }
    }
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

pub(crate) fn todo_step<T>(what: &str, step: &str) -> Result<T> {
    Err(Error::Unsupported(format!(
        "FilesDb::{what} is not implemented yet (feat#157 {step})"
    )))
}
