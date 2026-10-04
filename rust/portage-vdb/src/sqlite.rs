//! The SQLite backend (feat#157, backlog #305 S2.3: schema and open;
//! S2.4: the read side; writes are S2.5). Design: `docs/vdb_to_db.md` §4, §7.1.
//!
//! One database file, WAL, `synchronous=FULL`. The `entry_file` rows are
//! the truth; `owner`, `dep_atom` and `needed` are derived from them and
//! can be rebuilt. `meta.schema_version` ([`SCHEMA_VERSION`]) is
//! independent of `METADATA_FILE_FORMAT_VERSION` (plan §1, Q4).
//!
//! Local filesystems only (WAL needs shared memory, §4). A network
//! filesystem is **not** detected here: that would need `statfs` magic
//! numbers and a libc dependency; it is documented, not enforced.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

use crate::files::{claim_paths, normalise_aux_bytes, parse_metadata_text, translate_aux_slot};
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryFields, EntryFile, EntryImage,
    EntryKey, Error, FileMeta, InstalledDb, METADATA_FILE_FIELDS, MetadataStamp, PreservedLibs,
    PreservedLibsEntry, Result, Snapshot, World, WorldSets, WriteTxn, in_metadata_file,
};

/// The schema version this build reads and writes (`meta.schema_version`).
/// Bump it with any incompatible change to [`SCHEMA`]; an existing
/// database with another value is refused, never migrated silently.
pub const SCHEMA_VERSION: u32 = 1;

/// `busy_timeout` in milliseconds: how long a writer waits for the lock.
const BUSY_TIMEOUT_MS: u32 = 5000;

/// The schema, applied in one transaction when the file is created.
///
/// Differences from `vdb_to_db.md` §7.1, all from the current interface
/// (lib.rs module doc): `entry` is keyed by `(category, pf)` (item 1; no
/// `pn`/`ver` split), a pending entry is a row with `state = 'merging'`
/// (item 3), the `metadata` stamp state is stored per entry (item 12),
/// the directory mode is kept, and `counter` is not unique (a corpus can
/// hold duplicates). `owner` and `needed` carry the keys the S2.4 reads
/// need; `preserved_lib` is keyed by the registry key `cp:slot` with a
/// position so the path order survives (an entry with no paths is not
/// stored, as real's `store()` drops it). Paths and file data are BLOB.
pub const SCHEMA: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE entry (
    id             INTEGER PRIMARY KEY,
    category       TEXT NOT NULL,
    pf             TEXT NOT NULL,
    state          TEXT NOT NULL CHECK (state IN ('merging', 'installed')),
    counter        INTEGER,
    slot           TEXT,
    subslot        TEXT,
    repo           TEXT,
    metadata_stamp TEXT NOT NULL DEFAULT 'absent'
                   CHECK (metadata_stamp IN ('absent', 'valid', 'stale')),
    dir_mode       INTEGER NOT NULL DEFAULT 493,
    dir_mtime_ns   INTEGER NOT NULL DEFAULT 0,
    UNIQUE (category, pf)
);
CREATE INDEX entry_counter ON entry (counter);

CREATE TABLE entry_file (
    entry_id INTEGER NOT NULL REFERENCES entry (id) ON DELETE CASCADE,
    name     TEXT NOT NULL,
    data     BLOB NOT NULL,
    mode     INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL,
    PRIMARY KEY (entry_id, name)
) WITHOUT ROWID;

CREATE TABLE owner (
    entry_id INTEGER NOT NULL REFERENCES entry (id) ON DELETE CASCADE,
    seq      INTEGER NOT NULL,
    path     BLOB NOT NULL,
    kind     TEXT NOT NULL,
    md5      TEXT,
    mtime    INTEGER,
    target   BLOB,
    PRIMARY KEY (entry_id, seq)
) WITHOUT ROWID;
CREATE INDEX owner_path ON owner (path);

CREATE TABLE dep_atom (
    entry_id INTEGER NOT NULL REFERENCES entry (id) ON DELETE CASCADE,
    class    TEXT NOT NULL
             CHECK (class IN ('DEPEND', 'RDEPEND', 'BDEPEND', 'PDEPEND', 'IDEPEND')),
    cp       TEXT NOT NULL,
    atom     TEXT NOT NULL
);
CREATE INDEX dep_atom_cp ON dep_atom (cp);
CREATE INDEX dep_atom_entry ON dep_atom (entry_id);

CREATE TABLE needed (
    entry_id INTEGER NOT NULL REFERENCES entry (id) ON DELETE CASCADE,
    arch     TEXT NOT NULL,
    obj      BLOB NOT NULL,
    soname   TEXT,
    rpath    TEXT,
    needed   TEXT
);
CREATE INDEX needed_entry ON needed (entry_id);

CREATE TABLE preserved_lib (
    reg_key TEXT NOT NULL,
    pos     INTEGER NOT NULL,
    cpv     TEXT NOT NULL,
    counter TEXT NOT NULL,
    path    BLOB NOT NULL,
    PRIMARY KEY (reg_key, pos)
) WITHOUT ROWID;

CREATE TABLE world (
    atom TEXT PRIMARY KEY
) WITHOUT ROWID;

CREATE TABLE world_sets (
    name TEXT PRIMARY KEY
) WITHOUT ROWID;

CREATE TABLE config_memory (
    path BLOB PRIMARY KEY,
    md5  TEXT NOT NULL
) WITHOUT ROWID;
";

/// Every table [`SCHEMA`] creates.
#[cfg(test)]
const TABLES: &[&str] = &[
    "meta",
    "entry",
    "entry_file",
    "owner",
    "dep_atom",
    "needed",
    "preserved_lib",
    "world",
    "world_sets",
    "config_memory",
];

fn db_err(path: &Path, e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::NotADatabase => {
            Error::Corrupt(format!("{}: not a SQLite database", path.display()))
        }
        e => Error::Backend(format!("{}: {e}", path.display())),
    }
}

/// The SQLite backend. One file, one connection behind a mutex.
pub struct SqliteDb {
    path: PathBuf,
    conn: Mutex<Connection>,
    readonly: bool,
}

impl std::fmt::Debug for SqliteDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteDb")
            .field("path", &self.path)
            .field("readonly", &self.readonly)
            .finish()
    }
}

impl SqliteDb {
    /// Open `path` read-write, creating and initialising it when missing
    /// (an empty existing file counts as missing). Sets WAL,
    /// `synchronous=FULL`, a 5 s `busy_timeout` and `foreign_keys=ON`.
    /// An existing database whose `meta.schema_version` is not
    /// [`SCHEMA_VERSION`] is [`Error::Corrupt`]; a file that is not a
    /// database likewise.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| db_err(&path, e))?;
        let wrap = |e| db_err(&path, e);
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))
            .map_err(wrap)?;
        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(wrap)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::Invalid(format!(
                "{}: cannot enable WAL (journal_mode is {mode}); local filesystem required",
                path.display()
            )));
        }
        conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
            .map_err(wrap)?;
        let has_tables: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table')",
                [],
                |r| r.get(0),
            )
            .map_err(wrap)?;
        if has_tables {
            check_version(&conn, &path)?;
        } else {
            create(&conn, &path)?;
        }
        Ok(SqliteDb {
            path,
            conn: Mutex::new(conn),
            readonly: false,
        })
    }

    /// Open an existing database read-only. A missing file is
    /// [`Error::Io`] (`NotFound`); the schema version is checked as in
    /// [`SqliteDb::open`]. Nothing is created or modified.
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Err(e) = std::fs::metadata(&path) {
            return Err(Error::io(&path, e));
        }
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| db_err(&path, e))?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))
            .and_then(|()| conn.execute_batch("PRAGMA foreign_keys=ON;"))
            .map_err(|e| db_err(&path, e))?;
        check_version(&conn, &path)?;
        Ok(SqliteDb {
            path,
            conn: Mutex::new(conn),
            readonly: true,
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this handle was opened with [`SqliteDb::open_readonly`].
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }
}

fn create(conn: &Connection, path: &Path) -> Result<()> {
    let wrap = |e| db_err(path, e);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let tx = conn.unchecked_transaction().map_err(wrap)?;
    tx.execute_batch(SCHEMA).map_err(wrap)?;
    // counter_hwm: highest counter handed out; -1 = none yet (the value
    // `next_counter` starts from on `files`).
    for (k, v) in [
        ("schema_version", SCHEMA_VERSION.to_string()),
        ("generation", "0".to_string()),
        ("counter_hwm", "-1".to_string()),
        ("created_at", now.to_string()),
    ] {
        tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", (k, v))
            .map_err(wrap)?;
    }
    tx.commit().map_err(wrap)
}

fn check_version(conn: &Connection, path: &Path) -> Result<()> {
    let found: Option<String> = match conn.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |r| r.get(0),
    ) {
        Ok(v) => Some(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        // No `meta` table: a foreign database.
        Err(rusqlite::Error::SqliteFailure(_, Some(m))) if m.contains("no such table") => None,
        Err(e) => return Err(db_err(path, e)),
    };
    match found {
        Some(v) if v == SCHEMA_VERSION.to_string() => Ok(()),
        Some(v) => Err(Error::Corrupt(format!(
            "{}: schema_version {v}, this build supports {SCHEMA_VERSION}",
            path.display()
        ))),
        None => Err(Error::Corrupt(format!(
            "{}: not a portuale VDB (no meta.schema_version)",
            path.display()
        ))),
    }
}

fn unsupported<T>(what: &str, step: &str) -> Result<T> {
    Err(Error::Unsupported(format!(
        "SqliteDb::{what} (plan step {step})"
    )))
}

fn stamp_of(s: &str) -> MetadataStamp {
    match s {
        "valid" => MetadataStamp::Valid,
        "stale" => MetadataStamp::Stale,
        _ => MetadataStamp::Absent,
    }
}

/// A SQL `TEXT` column holding a path or value that the interface types
/// as `String`: lossy, like `files` would decode a hand-written file.
fn lossy(b: Vec<u8>) -> String {
    String::from_utf8_lossy(&b).into_owned()
}

fn to_u32(v: i64) -> rusqlite::Result<u32> {
    u32::try_from(v).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(e))
    })
}

/// The `installed` row of `key`: `(id, metadata_stamp)`.
fn installed_row(conn: &Connection, key: &EntryKey) -> rusqlite::Result<Option<(i64, String)>> {
    entry_row(conn, key, "installed")
}

fn entry_row(
    conn: &Connection,
    key: &EntryKey,
    state: &str,
) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT id, metadata_stamp FROM entry WHERE category = ?1 AND pf = ?2 AND state = ?3",
        params![key.category, key.pf, state],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

fn blob(conn: &Connection, id: i64, name: &str) -> rusqlite::Result<Option<Vec<u8>>> {
    conn.query_row(
        "SELECT data FROM entry_file WHERE entry_id = ?1 AND name = ?2",
        params![id, name],
        |r| r.get(0),
    )
    .optional()
}

/// One live entry with the files an `aux_get` of some fields needs.
struct Loaded {
    key: EntryKey,
    stamp: MetadataStamp,
    files: HashMap<String, Vec<u8>>,
}

impl Loaded {
    /// The validated snapshot, as `files` reads it: only when the stored
    /// stamp is `valid`, a `metadata` file is stored, it is UTF-8 and its
    /// `#format=` is the supported one.
    fn snapshot(&self) -> Option<HashMap<String, String>> {
        if self.stamp != MetadataStamp::Valid {
            return None;
        }
        let text = std::str::from_utf8(self.files.get("metadata")?).ok()?;
        parse_metadata_text(text).map(|(m, _)| m)
    }

    /// `FilesDb::aux_get_field` for one in-set field: a validated snapshot
    /// is complete (a missing field is `""`, the stored field file is not
    /// consulted); otherwise the field file, whitespace-joined, lossy
    /// UTF-8, absent as `""`; then the invalid-`SLOT` translation.
    fn field(&self, snap: Option<&HashMap<String, String>>, field: &str) -> String {
        let v = match snap {
            Some(m) => m.get(field).cloned().unwrap_or_default(),
            None => self
                .files
                .get(field)
                .map(|b| normalise_aux_bytes(b))
                .unwrap_or_default(),
        };
        translate_aux_slot(field, v)
    }
}

/// Every live entry in `(category, pf)` order, with the stored files named
/// in `names` plus `metadata`.
fn load_live(conn: &Connection, names: &[&str]) -> rusqlite::Result<Vec<Loaded>> {
    let list = names
        .iter()
        .chain(&["metadata"])
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT e.category, e.pf, e.metadata_stamp, f.name, f.data
         FROM entry e LEFT JOIN entry_file f ON f.entry_id = e.id AND f.name IN ({list})
         WHERE e.state = 'installed' ORDER BY e.category, e.pf"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out: Vec<Loaded> = Vec::new();
    while let Some(r) = rows.next()? {
        let key = EntryKey::new(r.get::<_, String>(0)?, r.get::<_, String>(1)?);
        if out.last().is_none_or(|l| l.key != key) {
            out.push(Loaded {
                key,
                stamp: stamp_of(&r.get::<_, String>(2)?),
                files: HashMap::new(),
            });
        }
        if let Some(name) = r.get::<_, Option<String>>(3)? {
            let data: Vec<u8> = r.get(4)?;
            if let Some(l) = out.last_mut() {
                l.files.insert(name, data);
            }
        }
    }
    Ok(out)
}

fn file_meta_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileMeta> {
    Ok(FileMeta {
        name: r.get(0)?,
        len: r.get::<_, i64>(1)?.max(0) as u64,
        mode: to_u32(r.get(2)?)?,
        mtime_ns: i128::from(r.get::<_, i64>(3)?),
    })
}

impl SqliteDb {
    /// Run `f` on the connection; engine errors become [`Error::Backend`].
    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| {
            Error::Backend(format!(
                "{}: connection mutex poisoned",
                self.path.display()
            ))
        })?;
        f(&conn).map_err(|e| db_err(&self.path, e))
    }

    fn meta_value(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .optional()
        })
    }
}

impl InstalledDb for SqliteDb {
    fn kind(&self) -> BackendKind {
        BackendKind::Sqlite
    }
    fn vdb_dir(&self) -> Option<PathBuf> {
        None
    }
    fn entry_path(&self, _key: &EntryKey) -> Option<PathBuf> {
        None
    }

    /// `meta.generation`: +1 on every committed write (S2.5).
    fn generation(&self) -> Result<u64> {
        let v = self.meta_value("generation")?;
        v.as_deref().and_then(|v| v.parse().ok()).ok_or_else(|| {
            Error::Corrupt(format!(
                "{}: meta.generation missing or not a number",
                self.path.display()
            ))
        })
    }

    /// The global generation, whatever the category: it changes on every
    /// commit, hence whenever an entry of `category` changes. It also
    /// changes for commits that touch only other categories, so a
    /// category-keyed cache is invalidated more often than strictly
    /// needed, never less. A per-category counter would cost a column or
    /// table kept in step by every write; not worth it until a profile
    /// asks. (`files` returns `0` for a missing category directory; a
    /// database has no such notion and returns the global value.)
    fn category_generation(&self, _category: &str) -> Result<u64> {
        self.generation()
    }

    fn entries(&self) -> Result<Vec<EntryKey>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT category, pf FROM entry WHERE state = 'installed' ORDER BY category, pf",
            )?;
            stmt.query_map([], |r| {
                Ok(EntryKey::new(
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                ))
            })?
            .collect()
        })
    }

    /// Categories that hold at least one live entry (`files` also lists a
    /// category directory that is empty or holds only a pending entry).
    fn categories(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT category FROM entry WHERE state = 'installed' ORDER BY category",
            )?;
            stmt.query_map([], |r| r.get(0))?.collect()
        })
    }

    fn category_entries(&self, category: &str) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT pf FROM entry WHERE state = 'installed' AND category = ?1 ORDER BY pf",
            )?;
            stmt.query_map([category], |r| r.get(0))?.collect()
        })
    }

    fn has_entry(&self, key: &EntryKey) -> Result<bool> {
        self.with(|c| Ok(installed_row(c, key)?.is_some()))
    }

    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>> {
        if !in_metadata_file(field) {
            return Err(Error::Invalid(format!(
                "aux_get key {field:?} is not one of the 23 metadata fields (use read_file)"
            )));
        }
        self.with(|c| {
            let Some((id, stamp)) = installed_row(c, key)? else {
                return Ok(None);
            };
            let mut l = Loaded {
                key: key.clone(),
                stamp: stamp_of(&stamp),
                files: HashMap::new(),
            };
            if l.stamp == MetadataStamp::Valid
                && let Some(m) = blob(c, id, "metadata")?
            {
                l.files.insert("metadata".to_string(), m);
            }
            let snap = l.snapshot();
            if snap.is_none()
                && let Some(d) = blob(c, id, field)?
            {
                l.files.insert(field.to_string(), d);
            }
            Ok(Some(l.field(snap.as_ref(), field)))
        })
    }

    /// Built from the stored field files under the `aux_get` rules (the
    /// 23 normalised fields, snapshot rule per stored stamp), in one read
    /// transaction together with `generation`.
    fn snapshot(&self) -> Result<Arc<Snapshot>> {
        let (generation, loaded) = self.with(|c| {
            let tx = c.unchecked_transaction()?;
            let generation: Option<String> = tx
                .query_row("SELECT value FROM meta WHERE key = 'generation'", [], |r| {
                    r.get(0)
                })
                .optional()?;
            let loaded = load_live(&tx, METADATA_FILE_FIELDS)?;
            Ok((generation, loaded))
        })?;
        let generation = generation.and_then(|g| g.parse().ok()).ok_or_else(|| {
            Error::Corrupt(format!(
                "{}: meta.generation missing or not a number",
                self.path.display()
            ))
        })?;
        let entries = loaded
            .into_iter()
            .map(|l| {
                let snap = l.snapshot();
                let fields = EntryFields::from_pairs(
                    METADATA_FILE_FIELDS
                        .iter()
                        .map(|&f| (f, l.field(snap.as_ref(), f))),
                );
                (l.key, Arc::new(fields))
            })
            .collect();
        Ok(Arc::new(Snapshot::new(generation, entries)))
    }

    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>> {
        self.with(|c| {
            let Some((id, _)) = installed_row(c, key)? else {
                return Ok(None);
            };
            let mut stmt = c.prepare(
                "SELECT name, length(data), mode, mtime_ns FROM entry_file
                 WHERE entry_id = ?1 ORDER BY name",
            )?;
            let v = stmt
                .query_map([id], file_meta_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some(v))
        })
    }

    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>> {
        self.with(|c| {
            let Some((id, _)) = installed_row(c, key)? else {
                return Ok(None);
            };
            c.query_row(
                "SELECT name, length(data), mode, mtime_ns FROM entry_file
                 WHERE entry_id = ?1 AND name = ?2",
                params![id, name],
                file_meta_row,
            )
            .optional()
        })
    }

    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        self.with(|c| match installed_row(c, key)? {
            Some((id, _)) => blob(c, id, name),
            None => Ok(None),
        })
    }

    /// `substr` on the blob: `Some(empty)` at or past the end, short at
    /// the end, `None` for a missing entry or file. (SQLite reads the blob
    /// to cut it; incremental blob I/O would avoid that and needs
    /// rusqlite's `blob` feature, left for S7.)
    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>> {
        // A blob is at most 1e9 bytes (SQLITE_MAX_LENGTH) and `substr` works in
        // C `int`, so clamp to 2^30 instead of letting it truncate.
        let start = off.min(1 << 30) as i64 + 1;
        let len = len.min(1 << 30) as i64;
        self.with(|c| {
            let Some((id, _)) = installed_row(c, key)? else {
                return Ok(None);
            };
            c.query_row(
                "SELECT substr(data, ?3, ?4) FROM entry_file WHERE entry_id = ?1 AND name = ?2",
                params![id, name, start, len],
                // substr of an empty blob is NULL.
                |r| Ok(r.get::<_, Option<Vec<u8>>>(0)?.unwrap_or_default()),
            )
            .optional()
        })
    }

    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        self.with(|c| match entry_row(c, key, "merging")? {
            Some((id, _)) => blob(c, id, name),
            None => Ok(None),
        })
    }

    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT e.category, e.pf, f.data
                 FROM entry e LEFT JOIN entry_file f ON f.entry_id = e.id AND f.name = ?1
                 WHERE e.state = 'installed' ORDER BY e.category, e.pf",
            )?;
            stmt.query_map([name], |r| {
                Ok((
                    EntryKey::new(r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                    r.get::<_, Option<Vec<u8>>>(2)?,
                ))
            })?
            .collect()
        })
    }

    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT id, metadata_stamp, dir_mode, dir_mtime_ns FROM entry
                     WHERE category = ?1 AND pf = ?2 AND state = 'installed'",
                    params![key.category, key.pf],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((id, stamp, dir_mode, dir_mtime_ns)) = row else {
                return Ok(None);
            };
            let mut stmt = c.prepare(
                "SELECT name, length(data), mode, mtime_ns, data FROM entry_file
                 WHERE entry_id = ?1 ORDER BY name",
            )?;
            let files = stmt
                .query_map([id], |r| {
                    Ok(EntryFile {
                        meta: file_meta_row(r)?,
                        data: r.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some(EntryImage {
                key: key.clone(),
                files,
                dir_mode: to_u32(dir_mode)?,
                dir_mtime_ns: i128::from(dir_mtime_ns),
                metadata_stamp: stamp_of(&stamp),
            }))
        })
    }

    /// Every live entry, in `(category, pf)` order, with its `USE` and the
    /// requested classes normalised like `aux_get` (the superset rule of
    /// the module doc, item 8; the `dep_atom` index narrows it in S8).
    fn reverse_dependents(&self, _cp: &str, classes: &[DepClass]) -> Result<Vec<DepRecord>> {
        let mut names: Vec<&str> = vec!["USE"];
        names.extend(classes.iter().map(|c| c.field()));
        let loaded = self.with(|c| load_live(c, &names))?;
        Ok(loaded
            .into_iter()
            .map(|l| {
                let snap = l.snapshot();
                DepRecord {
                    use_flags: l.field(snap.as_ref(), "USE"),
                    deps: classes
                        .iter()
                        .map(|&c| (c, l.field(snap.as_ref(), c.field())))
                        .collect(),
                    key: l.key,
                }
            })
            .collect())
    }

    /// Computed from the stored `CONTENTS` files with the rule of
    /// `FilesDb::owners` (shared code), in `(category, pf)` order, so the
    /// result is identical to `files`: a missing or non-UTF-8 `CONTENTS`
    /// owns nothing. The derived `owner` table (filled by S2.5, indexed
    /// in S8) is not read yet.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        let mut out = Vec::new();
        if paths.is_empty() {
            return Ok(out);
        }
        for (key, data) in self.read_file_all("CONTENTS")? {
            if let Some(text) = data.and_then(|b| String::from_utf8(b).ok()) {
                claim_paths(&text, &key, paths, &mut out);
            }
        }
        Ok(out)
    }

    /// Sorted by atom (the table key); the writers sort and de-duplicate
    /// anyway, so this equals the `files` order for anything they wrote.
    fn world(&self) -> Result<World> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT atom FROM world ORDER BY atom")?;
            Ok(World {
                atoms: stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?,
            })
        })
    }

    fn world_sets(&self) -> Result<WorldSets> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT name FROM world_sets ORDER BY name")?;
            Ok(WorldSets {
                sets: stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?,
            })
        })
    }

    fn preserved_libs(&self) -> Result<PreservedLibs> {
        let entries = self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT reg_key, cpv, counter, path FROM preserved_lib ORDER BY reg_key, pos",
            )?;
            let mut rows = stmt.query([])?;
            let mut m: BTreeMap<String, PreservedLibsEntry> = BTreeMap::new();
            while let Some(r) = rows.next()? {
                let e = m
                    .entry(r.get::<_, String>(0)?)
                    .or_insert_with(|| PreservedLibsEntry {
                        cpv: String::new(),
                        counter: String::new(),
                        paths: Vec::new(),
                    });
                if e.paths.is_empty() {
                    e.cpv = r.get(1)?;
                    e.counter = r.get(2)?;
                }
                e.paths.push(lossy(r.get(3)?));
            }
            Ok(m)
        })?;
        Ok(PreservedLibs {
            loaded: entries.clone(),
            entries,
        })
    }

    fn config_memory(&self) -> Result<ConfigMemory> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT path, md5 FROM config_memory")?;
            let entries = stmt
                .query_map([], |r| Ok((lossy(r.get(0)?), r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ConfigMemory { entries })
        })
    }

    /// `meta.counter_hwm`; `-1` (nothing handed out yet) is `None`, like a
    /// missing `counter` file on `files`.
    fn counter(&self) -> Result<Option<Counter>> {
        let v = self.meta_value("counter_hwm")?;
        match v.as_deref().map(str::parse::<i64>) {
            Some(Ok(n)) if n >= 0 => Ok(Some(Counter(n))),
            Some(Ok(_)) | None => Ok(None),
            Some(Err(_)) => Err(Error::Corrupt(format!(
                "{}: meta.counter_hwm is not a number",
                self.path.display()
            ))),
        }
    }

    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>> {
        unsupported("begin_write", "S2.5")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let d = std::env::temp_dir().join(format!(
                "portage-vdb-sqlite-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }
        fn db(&self) -> PathBuf {
            self.0.join("vdb.sqlite")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn meta(db: &SqliteDb, key: &str) -> Option<String> {
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .ok()
    }

    #[test]
    fn fresh_database_has_meta_and_tables() {
        let t = Tmp::new();
        let db = SqliteDb::open(t.db()).unwrap();
        assert_eq!(meta(&db, "schema_version").unwrap(), "1");
        assert_eq!(meta(&db, "generation").unwrap(), "0");
        assert_eq!(meta(&db, "counter_hwm").unwrap(), "-1");
        assert!(meta(&db, "created_at").unwrap().parse::<u64>().is_ok());
        let conn = db.conn.lock().unwrap();
        for table in TABLES {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
        for idx in ["owner_path", "dep_atom_cp"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing index {idx}");
        }
        drop(conn);
        assert_eq!(db.kind(), BackendKind::Sqlite);
        assert!(db.vdb_dir().is_none());
        assert!(db.entry_path(&EntryKey::new("a", "b-1")).is_none());
        assert!(db.entries().unwrap().is_empty());
        assert!(matches!(db.begin_write(), Err(Error::Unsupported(m)) if m.contains("S2.5")));
    }

    #[test]
    fn pragmas_are_set() {
        let t = Tmp::new();
        let db = SqliteDb::open(t.db()).unwrap();
        let conn = db.conn.lock().unwrap();
        let q = |p: &str| -> i64 { conn.query_row(p, [], |r| r.get(0)).unwrap() };
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(q("PRAGMA synchronous"), 2); // FULL
        assert_eq!(q("PRAGMA foreign_keys"), 1);
        assert_eq!(q("PRAGMA busy_timeout"), i64::from(BUSY_TIMEOUT_MS));
    }

    #[test]
    fn reopen_keeps_data() {
        let t = Tmp::new();
        {
            let db = SqliteDb::open(t.db()).unwrap();
            db.conn
                .lock()
                .unwrap()
                .execute("UPDATE meta SET value='7' WHERE key='generation'", [])
                .unwrap();
        }
        let db = SqliteDb::open(t.db()).unwrap();
        assert_eq!(meta(&db, "generation").unwrap(), "7");
        let ro = SqliteDb::open_readonly(t.db()).unwrap();
        assert!(ro.is_readonly());
        assert_eq!(meta(&ro, "generation").unwrap(), "7");
    }

    #[test]
    fn schema_version_mismatch_is_refused() {
        let t = Tmp::new();
        {
            let db = SqliteDb::open(t.db()).unwrap();
            db.conn
                .lock()
                .unwrap()
                .execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
                .unwrap();
        }
        for r in [SqliteDb::open(t.db()), SqliteDb::open_readonly(t.db())] {
            match r {
                Err(Error::Corrupt(m)) => {
                    assert!(m.contains("schema_version 999"), "{m}");
                    assert!(m.contains("supports 1"), "{m}");
                }
                other => panic!("expected Corrupt, got {other:?}"),
            }
        }
    }

    #[test]
    fn foreign_database_is_refused() {
        let t = Tmp::new();
        Connection::open(t.db())
            .unwrap()
            .execute_batch("CREATE TABLE x (a)")
            .unwrap();
        assert!(matches!(SqliteDb::open(t.db()), Err(Error::Corrupt(_))));
        let junk = t.0.join("junk");
        std::fs::write(&junk, b"this is not a sqlite database at all, just text").unwrap();
        assert!(matches!(SqliteDb::open(&junk), Err(Error::Corrupt(_))));
    }

    #[test]
    fn readonly_missing_file_fails_and_creates_nothing() {
        let t = Tmp::new();
        match SqliteDb::open_readonly(t.db()) {
            Err(Error::Io { source, .. }) => {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound)
            }
            other => panic!("expected Io NotFound, got {other:?}"),
        }
        assert!(!t.db().exists());
    }

    #[test]
    fn foreign_keys_cascade() {
        let t = Tmp::new();
        let db = SqliteDb::open(t.db()).unwrap();
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO entry (id, category, pf, state) VALUES (1, 'a', 'b-1', 'installed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entry_file VALUES (1, 'SLOT', x'30', 420, 0)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM entry WHERE id = 1", []).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM entry_file", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        assert!(
            conn.execute("INSERT INTO entry_file VALUES (9, 'x', x'', 0, 0)", [])
                .is_err()
        );
    }

    // ------------------------------------------------------------------
    // S2.4: the read side against `FilesDb`. Writes are S2.5, so the
    // database is seeded with raw SQL from what a `FilesDb` (written
    // through its own `WriteTxn`) shows, and every read is compared.

    use crate::{FilesDb, METADATA_FILE_FIELDS};

    impl SqliteDb {
        fn seed_entry(
            &self,
            key: &EntryKey,
            state: &str,
            stamp: &str,
            files: &[(String, Vec<u8>, u32, i64)],
        ) {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO entry (category, pf, state, metadata_stamp, dir_mode, dir_mtime_ns)
                 VALUES (?1, ?2, ?3, ?4, 493, 1234)",
                params![key.category, key.pf, state, stamp],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            for (name, data, mode, mtime) in files {
                conn.execute(
                    "INSERT INTO entry_file VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, name, data, mode, mtime],
                )
                .unwrap();
            }
        }

        fn seed_sql(&self, sql: &str, p: impl rusqlite::Params) {
            self.conn.lock().unwrap().execute(sql, p).unwrap();
        }
    }

    fn put_files(db: &FilesDb, k: &EntryKey, files: &[(&str, &[u8])], seal: bool) {
        let mut t = db.begin_write().unwrap();
        t.begin_entry(k).unwrap();
        for (n, d) in files {
            t.put_entry_file(k, n, d).unwrap();
        }
        if seal {
            t.seal_entry(k).unwrap();
        }
        t.finish_entry(k).unwrap();
        t.commit().unwrap();
    }

    /// The files of `k` as `fdb` shows them, as seed rows.
    fn rows_of(fdb: &FilesDb, k: &EntryKey, pending: bool) -> Vec<(String, Vec<u8>, u32, i64)> {
        let metas = if pending {
            // The pending directory is not a live entry: list it by hand.
            let dir = fdb
                .vdb_dir()
                .unwrap()
                .join(&k.category)
                .join(format!("-MERGING-{}", k.pf));
            let mut v: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            v.sort();
            v.into_iter()
                .map(|n| FileMeta {
                    name: n,
                    len: 0,
                    mode: 0o644,
                    mtime_ns: 99,
                })
                .collect()
        } else {
            fdb.list_files(k).unwrap().unwrap()
        };
        metas
            .into_iter()
            .map(|m| {
                let data = if pending {
                    fdb.read_pending_file(k, &m.name).unwrap().unwrap()
                } else {
                    fdb.read_file(k, &m.name).unwrap().unwrap()
                };
                (m.name, data, m.mode, i64::try_from(m.mtime_ns).unwrap())
            })
            .collect()
    }

    /// Read `name` as `files` shows the live and the pending file.
    const PROBE_FILES: &[&str] = &[
        "CONTENTS",
        "SLOT",
        "EMPTY",
        "BIN",
        "NUL",
        "metadata",
        "NEEDED.ELF.2",
        "nope",
        "extra",
        "DESCRIPTION",
        "RDEPEND",
    ];

    fn assert_same_reads(fdb: &FilesDb, sdb: &SqliteDb, keys: &[EntryKey]) {
        assert_eq!(sdb.entries().unwrap(), fdb.entries().unwrap(), "entries");
        assert_eq!(
            sdb.categories().unwrap(),
            fdb.categories().unwrap(),
            "categories"
        );
        let mut cats: Vec<String> = keys.iter().map(|k| k.category.clone()).collect();
        cats.push("nope-cat".into());
        for c in &cats {
            assert_eq!(
                sdb.category_entries(c).unwrap(),
                fdb.category_entries(c).unwrap(),
                "category_entries {c}"
            );
        }
        for k in keys {
            assert_eq!(
                sdb.has_entry(k).unwrap(),
                fdb.has_entry(k).unwrap(),
                "has_entry {k}"
            );
            assert_eq!(
                sdb.list_files(k).unwrap(),
                fdb.list_files(k).unwrap(),
                "list_files {k}"
            );
            for f in METADATA_FILE_FIELDS {
                assert_eq!(
                    sdb.aux_get(k, f).unwrap(),
                    fdb.aux_get(k, f).unwrap(),
                    "aux_get {k} {f}"
                );
            }
            for n in PROBE_FILES {
                assert_eq!(
                    sdb.read_file(k, n).unwrap(),
                    fdb.read_file(k, n).unwrap(),
                    "read_file {k} {n}"
                );
                assert_eq!(
                    sdb.file_meta(k, n).unwrap(),
                    fdb.file_meta(k, n).unwrap(),
                    "file_meta {k} {n}"
                );
                assert_eq!(
                    sdb.read_pending_file(k, n).unwrap(),
                    fdb.read_pending_file(k, n).unwrap(),
                    "read_pending_file {k} {n}"
                );
            }
        }
        for n in ["CONTENTS", "NEEDED.ELF.2", "nope"] {
            assert_eq!(
                sdb.read_file_all(n).unwrap(),
                fdb.read_file_all(n).unwrap(),
                "read_file_all {n}"
            );
        }
        let q: &[&[u8]] = &[
            b"/usr/bin/x",
            b"usr/bin/y",
            b"/usr",
            b"/usr/z",
            b"/nope",
            b"/usr/bin/x",
        ];
        assert_eq!(sdb.owners(q).unwrap(), fdb.owners(q).unwrap(), "owners");
        assert_eq!(sdb.owners(&[]).unwrap(), fdb.owners(&[]).unwrap());
        assert_eq!(sdb.world().unwrap(), fdb.world().unwrap(), "world");
        assert_eq!(
            sdb.world_sets().unwrap(),
            fdb.world_sets().unwrap(),
            "world_sets"
        );
        assert_eq!(
            sdb.preserved_libs().unwrap(),
            fdb.preserved_libs().unwrap(),
            "preserved_libs"
        );
        assert_eq!(
            sdb.config_memory().unwrap(),
            fdb.config_memory().unwrap(),
            "config_memory"
        );
        assert_eq!(sdb.counter().unwrap(), fdb.counter().unwrap(), "counter");
        for bad in ["CONTENTS", "NEEDED.ELF.2"] {
            let k = &keys[0];
            assert!(matches!(sdb.aux_get(k, bad), Err(Error::Invalid(_))));
            assert!(matches!(fdb.aux_get(k, bad), Err(Error::Invalid(_))));
        }
    }

    const BAD: &[u8] = b"caf\xe9 x\n";

    struct Fixture {
        _t: Tmp,
        fdb: FilesDb,
        sdb: SqliteDb,
        keys: Vec<EntryKey>,
    }

    fn fixture() -> Fixture {
        let t = Tmp::new();
        let root = t.0.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let fdb = FilesDb::new(&root);
        let sdb = SqliteDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        let b = EntryKey::new("dev-libs", "b-1");
        let c = EntryKey::new("app-misc", "c-1");
        let d = EntryKey::new("app-misc", "d-1");
        let z = EntryKey::new("sys-apps", "z-1");
        let e = EntryKey::new("sys-apps", "e-1");
        let p = EntryKey::new("dev-libs", "p-1");
        let owned: &[u8] = b"dir /usr\nobj /usr/bin/x abc 1\nsym /usr/bin/y -> x 1\nfoo /usr/z\n";
        // a: sealed (valid stamp); a non-UTF-8 field is dropped from the
        // snapshot, so aux_get serves "" (the pinned oddity).
        put_files(
            &fdb,
            &a,
            &[
                ("SLOT", b"0/1.2\n"),
                ("RDEPEND", b"x\n  y\n"),
                ("DESCRIPTION", BAD),
                ("USE", b"a  b\n"),
                ("EAPI", b"8\n"),
                ("repository", b"gentoo\n"),
                ("EMPTY", b""),
                ("NUL", b"a\0b"),
                ("CONTENTS", owned),
            ],
            true,
        );
        // b: the same fields, never sealed (stamp absent): lossy text;
        // invalid SLOT -> "0".
        put_files(
            &fdb,
            &b,
            &[
                ("SLOT", b"!bad\n"),
                ("DESCRIPTION", BAD),
                ("RDEPEND", b"q\n"),
                ("BIN", BAD),
                ("CONTENTS", b"obj /usr/bin/x def 2\n"),
            ],
            false,
        );
        // c: sealed, then changed: stale stamp. The field file differs
        // from the snapshot, and CONTENTS is not UTF-8 (owns nothing).
        {
            let mut t = fdb.begin_write().unwrap();
            t.begin_entry(&c).unwrap();
            t.put_entry_file(&c, "SLOT", b"3\n").unwrap();
            t.put_entry_file(&c, "RDEPEND", b"old\n").unwrap();
            t.put_entry_file(&c, "CONTENTS", b"obj /usr/bin/x \xff\xfe\n")
                .unwrap();
            t.seal_entry(&c).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
            t.put_entry_file(&c, "RDEPEND", b"new  value\n").unwrap();
            t.put_entry_file(&c, "extra", b"e").unwrap();
            t.finish_entry(&c).unwrap();
            t.commit().unwrap();
        }
        assert_eq!(
            fdb.aux_get(&c, "RDEPEND").unwrap().as_deref(),
            Some("new value")
        );
        // d: no files at all. z: only a SLOT with a subslot.
        put_files(&fdb, &d, &[], false);
        put_files(
            &fdb,
            &z,
            &[("SLOT", b"2/3.4\n"), ("NEEDED.ELF.2", b"x\n")],
            false,
        );
        // e: sealed with no field files: no metadata file at all.
        put_files(&fdb, &e, &[("CONTENTS", b""), ("NEEDED.ELF.2", b"")], true);
        // p: pending only, in a category that also has live entries.
        {
            let mut t = fdb.begin_write().unwrap();
            t.begin_entry(&p).unwrap();
            t.put_entry_file(&p, "CONTENTS", b"obj /usr/bin/x ghi 3\n")
                .unwrap();
            t.put_entry_file(&p, "SLOT", b"9\n").unwrap();
            t.put_entry_file(&p, "NEEDED.ELF.2", b"p\n").unwrap();
            t.commit().unwrap();
        }
        // D4 stores through the files writer.
        {
            let mut t = fdb.begin_write().unwrap();
            t.set_world(&World {
                atoms: vec!["app-misc/c".into(), "dev-libs/a".into()],
            })
            .unwrap();
            t.set_world_sets(&WorldSets {
                sets: vec!["selected".into(), "system".into()],
            })
            .unwrap();
            let mut m = BTreeMap::new();
            m.insert(
                "dev-libs/a:0".to_string(),
                PreservedLibsEntry {
                    cpv: "dev-libs/a-1".into(),
                    counter: "7".into(),
                    paths: vec!["/usr/lib/z.so.1".into(), "/usr/lib/a.so.1".into()],
                },
            );
            m.insert(
                "sys-apps/z:2".to_string(),
                PreservedLibsEntry {
                    cpv: "sys-apps/z-1".into(),
                    counter: "8".into(),
                    paths: vec!["/lib/q.so".into()],
                },
            );
            t.set_preserved_libs(&PreservedLibs {
                entries: m,
                loaded: BTreeMap::new(),
            })
            .unwrap();
            let mut cm = BTreeMap::new();
            cm.insert(
                "/etc/a.conf".to_string(),
                "d41d8cd98f00b204e9800998ecf8427e".to_string(),
            );
            cm.insert(
                "/etc/b.conf".to_string(),
                "0cc175b9c0f1b6a831c399e269772661".to_string(),
            );
            t.set_config_memory(&ConfigMemory { entries: cm }).unwrap();
            t.next_counter().unwrap();
            t.next_counter().unwrap();
            t.commit().unwrap();
        }

        // Seed the database with the same logical content.
        for (k, stamp) in [
            (&a, "valid"),
            (&b, "absent"),
            (&c, "stale"),
            (&d, "absent"),
            (&z, "absent"),
            (&e, "absent"),
        ] {
            sdb.seed_entry(k, "installed", stamp, &rows_of(&fdb, k, false));
        }
        sdb.seed_entry(&p, "merging", "absent", &rows_of(&fdb, &p, true));
        assert!(fdb.read_file(&a, "metadata").unwrap().is_some());
        assert!(fdb.read_file(&e, "metadata").unwrap().is_none());
        for atom in fdb.world().unwrap().atoms {
            sdb.seed_sql("INSERT INTO world VALUES (?1)", [atom]);
        }
        for n in fdb.world_sets().unwrap().sets {
            sdb.seed_sql("INSERT INTO world_sets VALUES (?1)", [n]);
        }
        for (k, e) in fdb.preserved_libs().unwrap().entries {
            for (i, path) in e.paths.iter().enumerate() {
                sdb.seed_sql(
                    "INSERT INTO preserved_lib VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![k, i as i64, e.cpv, e.counter, path.as_bytes()],
                );
            }
        }
        for (k, v) in fdb.config_memory().unwrap().entries {
            sdb.seed_sql(
                "INSERT INTO config_memory VALUES (?1, ?2)",
                params![k.as_bytes(), v],
            );
        }
        let hwm = fdb.counter().unwrap().unwrap().0;
        sdb.seed_sql(
            "UPDATE meta SET value = ?1 WHERE key = 'counter_hwm'",
            [hwm.to_string()],
        );
        Fixture {
            _t: t,
            fdb,
            sdb,
            keys: vec![a, b, c, d, z, e, p, EntryKey::new("dev-libs", "zz-9")],
        }
    }

    #[test]
    fn every_read_matches_filesdb() {
        let f = fixture();
        assert_same_reads(&f.fdb, &f.sdb, &f.keys);
        // The pending entry is invisible to live reads, visible to its own.
        let p = &f.keys[6];
        assert!(!f.sdb.has_entry(p).unwrap());
        assert_eq!(f.sdb.read_file(p, "SLOT").unwrap(), None);
        assert_eq!(f.sdb.aux_get(p, "SLOT").unwrap(), None);
        assert_eq!(
            f.sdb.read_pending_file(p, "SLOT").unwrap().as_deref(),
            Some(&b"9\n"[..])
        );
        assert_eq!(f.sdb.read_pending_file(&f.keys[0], "SLOT").unwrap(), None);
        // The cases the brief names, spelled out.
        let a = &f.keys[0];
        assert_eq!(
            f.sdb.read_file(a, "EMPTY").unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            f.sdb.read_file(a, "NUL").unwrap().as_deref(),
            Some(&b"a\0b"[..])
        );
        assert_eq!(
            f.sdb.aux_get(a, "DESCRIPTION").unwrap().as_deref(),
            Some("")
        );
        let b = &f.keys[1];
        assert_eq!(
            f.sdb.aux_get(b, "DESCRIPTION").unwrap().as_deref(),
            Some("caf\u{fffd} x")
        );
        assert_eq!(f.sdb.aux_get(b, "SLOT").unwrap().as_deref(), Some("0"));
        assert_eq!(f.sdb.read_file(b, "BIN").unwrap().as_deref(), Some(BAD));
        let c = &f.keys[2];
        assert_eq!(
            f.sdb.aux_get(c, "RDEPEND").unwrap().as_deref(),
            Some("new value")
        );
        assert_eq!(f.sdb.aux_get(&f.keys[7], "SLOT").unwrap(), None);
        // owners really find things (a: dir, obj, sym; b: obj).
        let got = f
            .sdb
            .owners(&[b"/usr/bin/x".as_slice(), b"/usr".as_slice()])
            .unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(f.sdb.list_files(&f.keys[3]).unwrap(), Some(vec![]));
    }

    #[test]
    fn empty_database_matches_an_empty_filesdb() {
        let t = Tmp::new();
        let root = t.0.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let fdb = FilesDb::new(&root);
        let sdb = SqliteDb::open(t.db()).unwrap();
        assert_same_reads(&fdb, &sdb, &[EntryKey::new("a", "b-1")]);
        assert_eq!(sdb.generation().unwrap(), 0);
        assert_eq!(sdb.counter().unwrap(), None);
        assert!(sdb.snapshot().unwrap().entries.is_empty());
    }

    #[test]
    fn generation_follows_meta_and_category_generation_follows_it() {
        let t = Tmp::new();
        let sdb = SqliteDb::open(t.db()).unwrap();
        assert_eq!(sdb.generation().unwrap(), 0);
        assert_eq!(sdb.category_generation("a").unwrap(), 0);
        sdb.seed_sql("UPDATE meta SET value = '41' WHERE key = 'generation'", []);
        assert_eq!(sdb.generation().unwrap(), 41);
        assert_eq!(sdb.category_generation("a").unwrap(), 41);
        assert_eq!(sdb.category_generation("other").unwrap(), 41);
        sdb.seed_sql("UPDATE meta SET value = 'x' WHERE key = 'generation'", []);
        assert!(matches!(sdb.generation(), Err(Error::Corrupt(_))));
    }

    #[test]
    fn read_file_at_cuts_the_blob() {
        let t = Tmp::new();
        let sdb = SqliteDb::open(t.db()).unwrap();
        let k = EntryKey::new("dev-libs", "a-1");
        sdb.seed_entry(
            &k,
            "installed",
            "absent",
            &[
                ("DATA".into(), b"0123456789".to_vec(), 420, 0),
                ("EMPTY".into(), vec![], 420, 0),
            ],
        );
        let at = |off, len| sdb.read_file_at(&k, "DATA", off, len).unwrap();
        assert_eq!(at(0, 4).as_deref(), Some(&b"0123"[..]));
        assert_eq!(at(2, 3).as_deref(), Some(&b"234"[..]));
        assert_eq!(at(8, 10).as_deref(), Some(&b"89"[..]));
        assert_eq!(at(10, 4).as_deref(), Some(&b""[..]));
        assert_eq!(at(u64::MAX, 4).as_deref(), Some(&b""[..]));
        assert_eq!(at(0, 0).as_deref(), Some(&b""[..]));
        assert_eq!(at(0, usize::MAX).as_deref(), Some(&b"0123456789"[..]));
        assert_eq!(
            sdb.read_file_at(&k, "EMPTY", 0, 8).unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(sdb.read_file_at(&k, "NOPE", 0, 4).unwrap(), None);
        assert_eq!(
            sdb.read_file_at(&EntryKey::new("dev-libs", "zz-1"), "DATA", 0, 4)
                .unwrap(),
            None
        );
    }

    #[test]
    fn snapshot_entry_image_and_reverse_dependents_follow_the_rows() {
        let f = fixture();
        sdb_generation(&f.sdb, 5);
        let snap = f.sdb.snapshot().unwrap();
        assert_eq!(snap.generation, 5);
        assert_eq!(snap.entries.len(), 6, "live entries only");
        for (k, fields) in &snap.entries {
            for field in METADATA_FILE_FIELDS {
                assert_eq!(
                    fields.get(field).map(str::to_string),
                    f.fdb.aux_get(k, field).unwrap(),
                    "snapshot {k} {field}"
                );
            }
        }
        assert!(
            snap.get(&f.keys[6]).is_none(),
            "pending entry is not in the snapshot"
        );

        let img = f.sdb.entry_image(&f.keys[2]).unwrap().unwrap();
        assert_eq!(img.metadata_stamp, MetadataStamp::Stale);
        assert_eq!((img.dir_mode, img.dir_mtime_ns), (493, 1234));
        let want = f.fdb.list_files(&f.keys[2]).unwrap().unwrap();
        assert_eq!(
            img.files.iter().map(|x| x.meta.clone()).collect::<Vec<_>>(),
            want
        );
        for file in &img.files {
            assert_eq!(
                f.fdb
                    .read_file(&f.keys[2], &file.meta.name)
                    .unwrap()
                    .unwrap(),
                file.data
            );
        }
        assert_eq!(
            f.sdb
                .entry_image(&f.keys[0])
                .unwrap()
                .unwrap()
                .metadata_stamp,
            MetadataStamp::Valid
        );
        assert!(f.sdb.entry_image(&f.keys[6]).unwrap().is_none());
        assert!(f.sdb.entry_image(&f.keys[7]).unwrap().is_none());

        let recs = f
            .sdb
            .reverse_dependents("x/y", &[DepClass::Rdepend, DepClass::Depend])
            .unwrap();
        assert_eq!(recs.len(), 6);
        let ra = recs.iter().find(|r| r.key == f.keys[0]).unwrap();
        assert_eq!(ra.use_flags, "a b");
        assert_eq!(
            ra.deps,
            vec![
                (DepClass::Rdepend, "x y".to_string()),
                (DepClass::Depend, String::new())
            ]
        );
    }

    fn sdb_generation(db: &SqliteDb, n: u64) {
        db.seed_sql(
            "UPDATE meta SET value = ?1 WHERE key = 'generation'",
            [n.to_string()],
        );
    }

    #[test]
    fn empty_preserved_libs_and_a_readonly_handle_read_too() {
        let f = fixture();
        let ro = SqliteDb::open_readonly(f.sdb.path()).unwrap();
        assert_same_reads(&f.fdb, &ro, &f.keys);
    }
}
