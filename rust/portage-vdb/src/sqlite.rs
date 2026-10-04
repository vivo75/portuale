//! The SQLite backend (feat#157, backlog #305 S2.3: schema and open;
//! reads are S2.4, writes S2.5). Design: `docs/vdb_to_db.md` §4, §7.1.
//!
//! One database file, WAL, `synchronous=FULL`. The `entry_file` rows are
//! the truth; `owner`, `dep_atom` and `needed` are derived from them and
//! can be rebuilt. `meta.schema_version` ([`SCHEMA_VERSION`]) is
//! independent of `METADATA_FILE_FORMAT_VERSION` (plan §1, Q4).
//!
//! Local filesystems only (WAL needs shared memory, §4). A network
//! filesystem is **not** detected here: that would need `statfs` magic
//! numbers and a libc dependency; it is documented, not enforced.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};

use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryImage, EntryKey, Error, FileMeta,
    InstalledDb, PreservedLibs, Result, Snapshot, World, WorldSets, WriteTxn,
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
    #[allow(dead_code)] // used by the S2.4 reads and S2.5 writes
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
    fn generation(&self) -> Result<u64> {
        unsupported("generation", "S2.4")
    }
    fn category_generation(&self, _category: &str) -> Result<u64> {
        unsupported("category_generation", "S2.4")
    }
    fn entries(&self) -> Result<Vec<EntryKey>> {
        unsupported("entries", "S2.4")
    }
    fn categories(&self) -> Result<Vec<String>> {
        unsupported("categories", "S2.4")
    }
    fn category_entries(&self, _category: &str) -> Result<Vec<String>> {
        unsupported("category_entries", "S2.4")
    }
    fn has_entry(&self, _key: &EntryKey) -> Result<bool> {
        unsupported("has_entry", "S2.4")
    }
    fn aux_get(&self, _key: &EntryKey, _field: &str) -> Result<Option<String>> {
        unsupported("aux_get", "S2.4")
    }
    fn snapshot(&self) -> Result<Arc<Snapshot>> {
        unsupported("snapshot", "S2.4")
    }
    fn list_files(&self, _key: &EntryKey) -> Result<Option<Vec<FileMeta>>> {
        unsupported("list_files", "S2.4")
    }
    fn file_meta(&self, _key: &EntryKey, _name: &str) -> Result<Option<FileMeta>> {
        unsupported("file_meta", "S2.4")
    }
    fn read_file(&self, _key: &EntryKey, _name: &str) -> Result<Option<Vec<u8>>> {
        unsupported("read_file", "S2.4")
    }
    fn read_file_at(
        &self,
        _key: &EntryKey,
        _name: &str,
        _off: u64,
        _len: usize,
    ) -> Result<Option<Vec<u8>>> {
        unsupported("read_file_at", "S2.4")
    }
    fn read_pending_file(&self, _key: &EntryKey, _name: &str) -> Result<Option<Vec<u8>>> {
        unsupported("read_pending_file", "S2.4")
    }
    fn read_file_all(&self, _name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>> {
        unsupported("read_file_all", "S2.4")
    }
    fn entry_image(&self, _key: &EntryKey) -> Result<Option<EntryImage>> {
        unsupported("entry_image", "S2.4")
    }
    fn reverse_dependents(&self, _cp: &str, _classes: &[DepClass]) -> Result<Vec<DepRecord>> {
        unsupported("reverse_dependents", "S2.4")
    }
    fn owners(&self, _paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        unsupported("owners", "S2.4")
    }
    fn world(&self) -> Result<World> {
        unsupported("world", "S2.4")
    }
    fn world_sets(&self) -> Result<WorldSets> {
        unsupported("world_sets", "S2.4")
    }
    fn preserved_libs(&self) -> Result<PreservedLibs> {
        unsupported("preserved_libs", "S2.4")
    }
    fn config_memory(&self) -> Result<ConfigMemory> {
        unsupported("config_memory", "S2.4")
    }
    fn counter(&self) -> Result<Option<Counter>> {
        unsupported("counter", "S2.4")
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
        assert!(matches!(db.entries(), Err(Error::Unsupported(m)) if m.contains("S2.4")));
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
}
