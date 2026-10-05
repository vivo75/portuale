//! `SqliteDb`'s write side (S2.5): [`SqliteTxn`] and the derived tables.
//!
//! See the crate module doc, "Write transactions", for the model.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

use super::{BUSY_TIMEOUT_MS, SqliteDb, blob, db_err, entry_row, installed_row};
use crate::dep_cp::dep_cp;
use crate::files::{normalise_aux_bytes, translate_aux_slot};
use crate::{
    ConfigMemory, Counter, DepClass, EntryImage, EntryKey, Error, METADATA_FILE_FIELDS,
    METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, Result, World, WorldSets, WriteTxn,
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

/// One write transaction: its own connection, `BEGIN IMMEDIATE` at
/// [`SqliteDb::begin_write`], `COMMIT` in [`WriteTxn::commit`], `ROLLBACK`
/// when dropped without a commit.
pub(crate) struct SqliteTxn<'a> {
    db: &'a SqliteDb,
    conn: Connection,
    /// Something was written: `commit` bumps `meta.generation`.
    dirty: bool,
    done: bool,
}

impl<'a> SqliteTxn<'a> {
    pub(crate) fn begin(db: &'a SqliteDb) -> Result<Self> {
        let path = db.path.as_path();
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .be(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))
            .be(path)?;
        // `foreign_keys` is a no-op inside a transaction: set it first.
        conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE")
            .be(path)?;
        Ok(SqliteTxn {
            db,
            conn,
            dirty: false,
            done: false,
        })
    }

    fn path(&self) -> &Path {
        &self.db.path
    }

    fn pending(&self, key: &EntryKey) -> Result<(i64, String)> {
        entry_row(&self.conn, key, "merging")
            .be(self.path())?
            .ok_or_else(|| Error::Invalid(format!("no pending entry for {key}")))
    }

    fn live(&self, key: &EntryKey) -> Result<(i64, String)> {
        installed_row(&self.conn, key)
            .be(self.path())?
            .ok_or_else(|| Error::Invalid(format!("no such entry {key}")))
    }

    /// Store one file of a pending entry. A file added to an entry whose
    /// stamp is `valid` makes it `stale`, as a new name in the directory
    /// moves its mtime on `files`; overwriting a name does not.
    fn put_pending(&mut self, key: &EntryKey, name: &str, data: &[u8], mode: i64) -> Result<()> {
        let (id, _) = self.pending(key)?;
        let now = now_ns();
        let p = self.db.path.clone();
        let c = &self.conn;
        let existed = c
            .query_row(
                "SELECT 1 FROM entry_file WHERE entry_id = ?1 AND name = ?2",
                params![id, name],
                |_| Ok(()),
            )
            .optional()
            .be(&p)?
            .is_some();
        c.execute(
            "INSERT OR REPLACE INTO entry_file (entry_id, name, data, mode, mtime_ns)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, data, mode, now],
        )
        .be(&p)?;
        if !existed {
            c.execute(
                "UPDATE entry SET dir_mtime_ns = ?2,
                     metadata_stamp = CASE metadata_stamp WHEN 'valid' THEN 'stale'
                                      ELSE metadata_stamp END
                 WHERE id = ?1",
                params![id, now],
            )
            .be(&p)?;
        }
        self.dirty = true;
        Ok(())
    }

    fn set_hwm_if_larger(&self, counter: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE meta SET value = ?1
                 WHERE key = 'counter_hwm' AND CAST(value AS INTEGER) < ?1",
                [counter],
            )
            .be(self.path())?;
        Ok(())
    }

    fn hwm(&self) -> Result<i64> {
        let v: String = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'counter_hwm'",
                [],
                |r| r.get(0),
            )
            .be(self.path())?;
        v.parse().map_err(|_| {
            Error::Corrupt(format!(
                "{}: meta.counter_hwm is not a number",
                self.path().display()
            ))
        })
    }

    fn delete_installed(&self, key: &EntryKey) -> Result<usize> {
        self.conn
            .execute(
                "DELETE FROM entry WHERE category = ?1 AND pf = ?2 AND state = 'installed'",
                params![key.category, key.pf],
            )
            .be(self.path())
    }
}

impl Drop for SqliteTxn<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

impl WriteTxn for SqliteTxn<'_> {
    /// Drops a stale `merging` row of `key` (cascade) and inserts a fresh
    /// one; an `installed` row of the same key is left alone.
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()> {
        let p = self.db.path.clone();
        self.conn
            .execute(
                "DELETE FROM entry WHERE category = ?1 AND pf = ?2 AND state = 'merging'",
                params![key.category, key.pf],
            )
            .be(&p)?;
        self.conn
            .execute(
                "INSERT INTO entry (category, pf, state, dir_mtime_ns)
                 VALUES (?1, ?2, 'merging', ?3)",
                params![key.category, key.pf, now_ns()],
            )
            .be(&p)?;
        self.dirty = true;
        Ok(())
    }

    /// Deletes the `merging` row (its files cascade); an `installed` row
    /// of the same key is left alone.
    fn discard_pending(&mut self, key: &EntryKey) -> Result<()> {
        let n = self
            .conn
            .execute(
                "DELETE FROM entry WHERE category = ?1 AND pf = ?2 AND state = 'merging'",
                params![key.category, key.pf],
            )
            .be(self.path())?;
        if n == 0 {
            return Err(Error::Invalid(format!("no pending entry for {key}")));
        }
        self.dirty = true;
        Ok(())
    }

    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        self.put_pending(key, name, data, FILE_MODE)
    }

    /// Reads `src` whole; its permission bits come with it (the mtime does
    /// not, as with `std::fs::copy`). The error names `src`.
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()> {
        self.pending(key)?;
        let data = std::fs::read(src).map_err(|e| Error::io(src, e))?;
        let mode = std::fs::metadata(src)
            .map_err(|e| Error::io(src, e))?
            .mode();
        self.put_pending(key, name, &data, i64::from(mode))
    }

    /// `meta.counter_hwm + 1`, stored in the same transaction: atomic,
    /// unlike `files`. A database that never ticked starts at `0`.
    fn next_counter(&mut self) -> Result<Counter> {
        let next = self.hwm()?.max(-1) + 1;
        self.conn
            .execute(
                "UPDATE meta SET value = ?1 WHERE key = 'counter_hwm'",
                [next],
            )
            .be(self.path())?;
        self.dirty = true;
        Ok(Counter(next))
    }

    /// The `FilesTxn::seal_entry` bytes: every [`METADATA_FILE_FIELDS`]
    /// file that is UTF-8, whitespace-joined, sorted, under `#format=1`,
    /// then `#dir_mtime=<entry.dir_mtime_ns>`. The stamp state becomes
    /// `valid` (there is no directory mtime to drift; the state is what
    /// counts). No-op when the entry holds no field file.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()> {
        let (id, _) = self.pending(key)?;
        let p = self.db.path.clone();
        let mut data: Vec<(String, String)> = Vec::new();
        for &field in METADATA_FILE_FIELDS {
            let Some(raw) = blob(&self.conn, id, field).be(&p)? else {
                continue;
            };
            let Ok(text) = String::from_utf8(raw) else {
                continue;
            };
            data.push((
                field.to_string(),
                text.split_whitespace().collect::<Vec<_>>().join(" "),
            ));
        }
        if data.is_empty() {
            return Ok(());
        }
        data.sort();
        let now = now_ns();
        let mut body = format!("#format={METADATA_FILE_FORMAT_VERSION}\n");
        for (k, v) in &data {
            body.push_str(&format!("{k}={v}\n"));
        }
        body.push_str(&format!("#dir_mtime={now}\n"));
        self.conn
            .execute(
                "INSERT OR REPLACE INTO entry_file (entry_id, name, data, mode, mtime_ns)
                 VALUES (?1, 'metadata', ?2, ?3, ?4)",
                params![id, body.as_bytes(), FILE_MODE, now],
            )
            .be(&p)?;
        self.conn
            .execute(
                "UPDATE entry SET metadata_stamp = 'valid', dir_mtime_ns = ?2 WHERE id = ?1",
                params![id, now],
            )
            .be(&p)?;
        self.dirty = true;
        Ok(())
    }

    /// Deletes the `installed` row of the same key (cascade), flips the
    /// `merging` row to `installed` and derives the columns and the three
    /// index tables, raising `counter_hwm` to the entry's `COUNTER` when
    /// larger.
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()> {
        let (id, _) = self.pending(key)?;
        let p = self.db.path.clone();
        self.delete_installed(key)?;
        self.conn
            .execute("UPDATE entry SET state = 'installed' WHERE id = ?1", [id])
            .be(&p)?;
        let counter = derive_all(&self.conn, id).be(&p)?;
        if let Some(c) = counter {
            self.set_hwm_if_larger(c)?;
        }
        self.dirty = true;
        Ok(())
    }

    /// The image as given (files, modes, mtimes, directory mode and
    /// mtime, stamp state); no `metadata` file is added and the counter
    /// store is not touched (the converter calls `set_counter`).
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()> {
        let p = self.db.path.clone();
        let int = |v: i128, what: &str| {
            i64::try_from(v)
                .map_err(|_| Error::Invalid(format!("{}: {what} out of range", image.key)))
        };
        let dir_mtime = int(image.dir_mtime_ns, "dir_mtime_ns")?;
        let mut files = Vec::with_capacity(image.files.len());
        for f in &image.files {
            files.push((
                &f.meta.name,
                &f.data,
                f.meta.mode,
                int(f.meta.mtime_ns, "mtime_ns")?,
            ));
        }
        self.delete_installed(&image.key)?;
        self.conn
            .execute(
                "INSERT INTO entry (category, pf, state, metadata_stamp, dir_mode, dir_mtime_ns)
                 VALUES (?1, ?2, 'installed', ?3, ?4, ?5)",
                params![
                    image.key.category,
                    image.key.pf,
                    stamp_str(image.metadata_stamp),
                    image.dir_mode,
                    dir_mtime
                ],
            )
            .be(&p)?;
        let id = self.conn.last_insert_rowid();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "INSERT OR REPLACE INTO entry_file (entry_id, name, data, mode, mtime_ns)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .be(&p)?;
            for (name, data, mode, mtime) in files {
                stmt.execute(params![id, name, data, mode, mtime]).be(&p)?;
            }
        }
        derive_all(&self.conn, id).be(&p)?;
        self.dirty = true;
        Ok(())
    }

    fn delete_entry(&mut self, key: &EntryKey) -> Result<()> {
        if self.delete_installed(key)? == 0 {
            return Err(Error::Invalid(format!("no such entry {key}")));
        }
        self.dirty = true;
        Ok(())
    }

    /// In place: the file keeps its mode, and the stamp state, the
    /// directory mtime and every other file are untouched. The derived
    /// tables follow the file (`CONTENTS`, `NEEDED.ELF.2`, the `*DEPEND`
    /// fields, `SLOT`, `repository`, `COUNTER`).
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        let (id, _) = self.live(key)?;
        let p = self.db.path.clone();
        let mode: Option<i64> = self
            .conn
            .query_row(
                "SELECT mode FROM entry_file WHERE entry_id = ?1 AND name = ?2",
                params![id, name],
                |r| r.get(0),
            )
            .optional()
            .be(&p)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO entry_file (entry_id, name, data, mode, mtime_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, data, mode.unwrap_or(FILE_MODE), now_ns()],
            )
            .be(&p)?;
        derive_for_file(&self.conn, id, name).be(&p)?;
        self.dirty = true;
        Ok(())
    }

    fn set_world(&mut self, world: &World) -> Result<()> {
        let p = self.db.path.clone();
        self.conn.execute("DELETE FROM world", []).be(&p)?;
        let mut stmt = self
            .conn
            .prepare("INSERT INTO world (pos, atom) VALUES (?1, ?2)")
            .be(&p)?;
        for (i, atom) in world.atoms.iter().enumerate() {
            stmt.execute(params![i as i64, atom]).be(&p)?;
        }
        drop(stmt);
        self.dirty = true;
        Ok(())
    }

    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()> {
        let p = self.db.path.clone();
        self.conn.execute("DELETE FROM world_sets", []).be(&p)?;
        let mut stmt = self
            .conn
            .prepare("INSERT INTO world_sets (pos, name) VALUES (?1, ?2)")
            .be(&p)?;
        for (i, name) in sets.sets.iter().enumerate() {
            stmt.execute(params![i as i64, name]).be(&p)?;
        }
        drop(stmt);
        self.dirty = true;
        Ok(())
    }

    /// Nothing at all when `entries == loaded`. An entry without paths is
    /// one row with a NULL `path`.
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()> {
        if libs.entries == libs.loaded {
            return Ok(());
        }
        let p = self.db.path.clone();
        self.conn.execute("DELETE FROM preserved_lib", []).be(&p)?;
        let mut stmt = self
            .conn
            .prepare("INSERT INTO preserved_lib VALUES (?1, ?2, ?3, ?4, ?5)")
            .be(&p)?;
        for (reg_key, e) in &libs.entries {
            if e.paths.is_empty() {
                stmt.execute(params![reg_key, 0_i64, e.cpv, e.counter, None::<Vec<u8>>])
                    .be(&p)?;
            }
            for (i, path) in e.paths.iter().enumerate() {
                stmt.execute(params![
                    reg_key,
                    i as i64,
                    e.cpv,
                    e.counter,
                    path.as_bytes()
                ])
                .be(&p)?;
            }
        }
        drop(stmt);
        self.dirty = true;
        Ok(())
    }

    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()> {
        let p = self.db.path.clone();
        self.conn.execute("DELETE FROM config_memory", []).be(&p)?;
        let mut stmt = self
            .conn
            .prepare("INSERT INTO config_memory (path, md5) VALUES (?1, ?2)")
            .be(&p)?;
        for (path, md5) in &memory.entries {
            stmt.execute(params![path.as_bytes(), md5]).be(&p)?;
        }
        drop(stmt);
        self.dirty = true;
        Ok(())
    }

    fn set_counter(&mut self, counter: Counter) -> Result<()> {
        self.conn
            .execute(
                "UPDATE meta SET value = ?1 WHERE key = 'counter_hwm'",
                [counter.0],
            )
            .be(self.path())?;
        self.dirty = true;
        Ok(())
    }

    fn set_import_mark(&mut self, generation: u64, source: &str) -> Result<()> {
        let p = self.db.path.clone();
        let gen_str = generation.to_string();
        // Delete existing entries first to avoid conflicts
        self.conn
            .execute("DELETE FROM meta WHERE key IN ('imported_files_generation', 'imported_files_source')", [])
            .be(&p)?;
        // Insert the new values
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES ('imported_files_generation', ?1)",
                [gen_str.as_str()],
            )
            .be(&p)?;
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES ('imported_files_source', ?1)",
                [source],
            )
            .be(&p)?;
        self.dirty = true;
        Ok(())
    }

    /// Bumps `meta.generation` when anything was written, then `COMMIT`.
    /// On any failure the drop rolls back.
    fn commit(mut self: Box<Self>) -> Result<()> {
        let p = self.db.path.clone();
        if self.dirty {
            self.conn
                .execute(
                    "UPDATE meta SET value = CAST(value AS INTEGER) + 1 WHERE key = 'generation'",
                    [],
                )
                .be(&p)?;
        }
        self.conn.execute_batch("COMMIT").be(&p)?;
        self.done = true;
        Ok(())
    }
}

// ------------------------------------------------------------- derived data

/// Refill every derived part of entry `id`; returns its `COUNTER`.
fn derive_all(c: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    let counter = derive_columns(c, id)?;
    derive_owner(c, id)?;
    derive_needed(c, id)?;
    derive_dep_atom(c, id)?;
    Ok(counter)
}

/// Refill what depends on the file `name` of entry `id`.
fn derive_for_file(c: &Connection, id: i64, name: &str) -> rusqlite::Result<()> {
    match name {
        "CONTENTS" => derive_owner(c, id)?,
        "NEEDED.ELF.2" => derive_needed(c, id)?,
        "SLOT" | "repository" | "COUNTER" => {
            derive_columns(c, id)?;
        }
        n if DEP_CLASSES.iter().any(|d| d.field() == n) => derive_dep_atom(c, id)?,
        _ => {}
    }
    Ok(())
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

/// `needed` from `NEEDED.ELF.2` (`arch;obj;soname;rpath;needed[;...]`,
/// real `NeededEntry.parse`): a line with fewer than five `;` fields is
/// skipped, `obj` is bytes, the other columns are the field text (lossy
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

/// `dep_atom`: one row per distinct `(class, cp, token)` of the five
/// `*DEPEND` fields, from [`dep_cp`]. A prefilter, see there.
fn derive_dep_atom(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM dep_atom WHERE entry_id = ?1", [id])?;
    let mut stmt =
        c.prepare("INSERT INTO dep_atom (entry_id, class, cp, atom) VALUES (?1, ?2, ?3, ?4)")?;
    for class in DEP_CLASSES {
        let Some(raw) = blob(c, id, class.field())? else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        let rows: BTreeSet<(&str, &str)> = text
            .split_whitespace()
            .filter_map(|tok| dep_cp(tok).map(|cp| (cp, tok)))
            .collect();
        for (cp, tok) in rows {
            stmt.execute(params![id, class.field(), cp, tok])?;
        }
    }
    Ok(())
}
