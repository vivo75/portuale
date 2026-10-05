//! `SqliteDb`'s write side: [`SqliteTxn`] and the derived tables.
//!
//! See the crate module doc, "Write transactions", for the model.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

use super::{BUSY_TIMEOUT_MS, blob, db_err, entry_row, installed_row};
use crate::dep_cp::dep_index_key;
use crate::files::{normalise_aux_bytes, translate_aux_slot};
use crate::{
    ConfigMemory, Counter, DepClass, EntryImage, EntryKey, Error, IndexCounts,
    METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, Result,
    World, WorldSets, WriteTxn,
};

/// `S_IFREG | 0644`: what `files` shows for a file written with the
/// default umask.
const FILE_MODE: i64 = 0o100_644;

const DEP_CLASSES: [DepClass; 5] = [
    DepClass::Depend,
    DepClass::Rdepend,
    DepClass::Bdepend,
    DepClass::Pdepend,
    DepClass::Idepend,
];

trait Be<T> {
    fn be(self, path: &Path) -> Result<T>;
}

impl<T> Be<T> for rusqlite::Result<T> {
    fn be(self, path: &Path) -> Result<T> {
        self.map_err(|e| db_err(path, e))
    }
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

fn stamp_str(s: MetadataStamp) -> &'static str {
    match s {
        MetadataStamp::Absent => "absent",
        MetadataStamp::Valid => "valid",
        MetadataStamp::Stale => "stale",
    }
}

/// `slot`, `subslot`, `repo`, `counter` of the entry row from its `SLOT`,
/// `repository` and `COUNTER` files (NULL when the file is missing or, for
/// the counter, not a number). `SLOT` is read as `aux_get` serves it
/// (invalid becomes `0`); a slot without `/` has the slot as subslot.
fn derive_columns(c: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    let text = |name: &str| -> rusqlite::Result<Option<String>> {
        Ok(blob(c, id, name)?
            .map(|b| normalise_aux_bytes(&b))
            .filter(|s| !s.is_empty()))
    };
    let (slot, subslot) = match text("SLOT")?.map(|s| translate_aux_slot("SLOT", s)) {
        Some(s) => match s.split_once('/') {
            Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
            None => (Some(s.clone()), Some(s)),
        },
        None => (None, None),
    };
    let repo = text("repository")?;
    let counter = blob(c, id, "COUNTER")?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<i64>().ok());
    c.execute(
        "UPDATE entry SET slot = ?2, subslot = ?3, repo = ?4, counter = ?5 WHERE id = ?1",
        params![id, slot, subslot, repo, counter],
    )?;
    Ok(counter)
}

/// `owner` from `CONTENTS`, with the line rule of `files::claim_paths`:
/// whitespace-separated words, the first a recorded kind (`obj`, `sym`,
/// `dir`, `dev`, `fif`, `bin`), the second the path (stored as given,
/// leading `/` included). `obj` adds `md5` and `mtime`, `sym` (`path ->
/// target mtime`) `target` and `mtime`. `seq` counts recorded lines, so
/// `CONTENTS` order survives. A `CONTENTS` that is not UTF-8 contributes
/// no rows, as it owns nothing on `files`.
fn derive_owner(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM owner WHERE entry_id = ?1", [id])?;
    let Some(text) = blob(c, id, "CONTENTS")?.and_then(|b| String::from_utf8(b).ok()) else {
        return Ok(());
    };
    let mut stmt = c.prepare(
        "INSERT INTO owner (entry_id, seq, path, kind, md5, mtime, target)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut seq = 0_i64;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (kind, path) = match (words.next(), words.next()) {
            (Some(k @ ("obj" | "sym" | "dir" | "dev" | "fif" | "bin")), Some(path)) => (k, path),
            _ => continue,
        };
        let rest: Vec<&str> = words.collect();
        let (md5, mtime, target) = match kind {
            "obj" => (
                rest.first().copied(),
                rest.get(1).and_then(|m| m.parse::<i64>().ok()),
                None,
            ),
            "sym" if rest.first() == Some(&"->") => (
                None,
                rest.get(2).and_then(|m| m.parse::<i64>().ok()),
                rest.get(1).copied(),
            ),
            _ => (None, None, None),
        };
        stmt.execute(params![
            id,
            seq,
            path.as_bytes(),
            kind,
            md5,
            mtime,
            target.map(str::as_bytes)
        ])?;
        seq += 1;
    }
    Ok(())
}

/// `needed` from `NEEDED.ELF.2` (`arch;obj;soname;rpath;needed[;...]`):
/// a line with fewer than five `;` fields is skipped, `obj` is bytes, the other columns are the field text (lossy
/// UTF-8), with the `"  -  "` no-rpath sentinel stored as `""`. `rpath`
/// stays colon-joined and `needed` comma-joined, as in the file.
fn derive_needed(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM needed WHERE entry_id = ?1", [id])?;
    let Some(raw) = blob(c, id, "NEEDED.ELF.2")? else {
        return Ok(());
    };
    let mut stmt = c.prepare(
        "INSERT INTO needed (entry_id, arch, obj, soname, rpath, needed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    let lossy = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    for line in raw.split(|&b| b == b'\n') {
        let f: Vec<&[u8]> = line.split(|&b| b == b';').collect();
        if f.len() < 5 {
            continue;
        }
        let rpath = if f[3] == b"  -  " {
            String::new()
        } else {
            lossy(f[3])
        };
        stmt.execute(params![
            id,
            lossy(f[0]),
            f[1],
            lossy(f[2]),
            rpath,
            lossy(f[4])
        ])?;
    }
    Ok(())
}
