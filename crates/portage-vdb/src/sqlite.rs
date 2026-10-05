//! The SQLite backend: one database file, WAL, `synchronous=FULL`.
//!
//! The `entry_file` rows are the truth; `owner`, `dep_atom` and `needed`
//! are derived from them and can be rebuilt.
//!
//! Local filesystems only (WAL needs shared memory). A network filesystem
//! is **not** detected here: that would need `statfs` magic numbers and a
//! libc dependency; it is documented, not enforced.
//!
//! Design: `docs/vdb_to_db.md` §4, §7.1.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

mod write;

use crate::dep_cp::UNSURE_CP;
use crate::files::{owner_spellings, owner_wanted};
use crate::loaded::Loaded;
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryFields, EntryFile, EntryImage,
    EntryKey, EntryStat, Error, FileMeta, InstalledDb, METADATA_FILE_FIELDS, MetadataStamp,
    PreservedLibs, PreservedLibsEntry, Result, Snapshot, World, WorldSets, WriteTxn,
    in_metadata_file,
};

/// The schema version this build reads and writes (`meta.schema_version`).
/// Bump it with any incompatible change to [`SCHEMA`]; an existing
/// database with another value is refused, never migrated silently.
pub const SCHEMA_VERSION: u32 = 1;

/// `busy_timeout` in milliseconds: how long a writer waits for the lock.
const BUSY_TIMEOUT_MS: u32 = 5000;

/// The schema, applied in one transaction when the file is created.
///
/// `entry` is keyed by `(category, pf)` (no `pn`/`ver` split); a pending
/// entry is a row with `state = 'merging'` beside the `installed` row of
/// the same key. The `metadata` stamp state is stored per entry, the
/// directory mode is kept, and `counter` is not unique. `preserved_lib` is
/// keyed by the registry key `cp:slot` with a position so the path order
/// survives (an entry with no paths is one row with a NULL `path`, so
/// `files` and `sqlite` round-trip it alike). `world` / `world_sets` keep
/// the written order in `pos`. Paths and file data are BLOB.
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
    UNIQUE (category, pf, state)
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
    path    BLOB,
    PRIMARY KEY (reg_key, pos)
) WITHOUT ROWID;

CREATE TABLE world (
    pos  INTEGER PRIMARY KEY,
    atom TEXT NOT NULL
);

CREATE TABLE world_sets (
    pos  INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

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

/// What the read cache holds for one `PRAGMA data_version` value.
///
/// `data_version` changes when **another connection** commits to the
/// file (a second process, a second `SqliteDb`, or this handle's own
/// [`SqliteTxn`], which runs on its own connection); it costs one read of
/// the WAL index, no page I/O. Every cached read first compares it with
/// the stored value and drops the cache on a difference. Writes that go
/// through *this* connection (only the test seeders do) are not seen and
/// call [`SqliteDb::invalidate`].
///
/// Consistency: each call is served from a state that was one committed
/// generation of the database at validation time; a commit that lands
/// after the check is seen by the next call. Within a resolve with no
/// concurrent commit the data is stable and nothing is re-read.
#[derive(Default)]
struct CacheState {
    /// `data_version` the rest was read under; `None` = nothing cached.
    data_version: Option<i64>,
    generation: Option<u64>,
    full: Option<Arc<Full>>,
}

/// All live entries and their 23 normalised fields, loaded in one read
/// transaction.
struct Full {
    snapshot: Arc<Snapshot>,
    /// `pf` lists per category, in `pf` order.
    by_category: HashMap<String, Vec<String>>,
    /// Categories with a live entry, sorted.
    categories: Vec<String>,
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

/// Every live entry in `(category, pf)` order, with the stored files named
/// in `names` plus `metadata`: one `SELECT` over `entry` and one scan of
/// `entry_file` restricted to those names, joined here by `entry_id`
/// (a per-entry join does a B-tree seek per name and is 4x slower on a
/// tree-sized VDB; the scan skips the large blobs' overflow pages).
fn load_live(conn: &Connection, names: &[&str]) -> rusqlite::Result<Vec<Loaded>> {
    let mut out: Vec<Loaded> = Vec::new();
    let mut by_id: HashMap<i64, usize> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, category, pf, metadata_stamp FROM entry
             WHERE state = 'installed' ORDER BY category, pf",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            by_id.insert(r.get(0)?, out.len());
            out.push(Loaded {
                key: EntryKey::new(r.get::<_, String>(1)?, r.get::<_, String>(2)?),
                stamp: stamp_of(&r.get::<_, String>(3)?),
                files: HashMap::new(),
            });
        }
    }
    let list = names
        .iter()
        .chain(&["metadata"])
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT entry_id, name, data FROM entry_file WHERE name IN ({list})"
    ))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        if let Some(&i) = by_id.get(&r.get::<_, i64>(0)?) {
            out[i].files.insert(r.get(1)?, r.get(2)?);
        }
    }
    Ok(out)
}

/// [`load_live`] for the entries `dep_atom` ties to `cp` (or to the unsure
/// marker) in one of `classes`: the candidate ids come from the index, and
/// each candidate's named files from a primary-key seek.
fn load_dependents(
    conn: &Connection,
    names: &[&str],
    cp: &str,
    classes: &[DepClass],
) -> rusqlite::Result<Vec<Loaded>> {
    let class_list = classes
        .iter()
        .map(|c| format!("'{}'", c.field()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT id, category, pf, metadata_stamp FROM entry
         WHERE state = 'installed' AND id IN
           (SELECT entry_id FROM dep_atom
            WHERE cp IN (?1, '{UNSURE_CP}') AND class IN ({class_list}))
         ORDER BY category, pf"
    ))?;
    let mut cands: Vec<(i64, Loaded)> = Vec::new();
    let mut rows = stmt.query([cp])?;
    while let Some(r) = rows.next()? {
        cands.push((
            r.get(0)?,
            Loaded {
                key: EntryKey::new(r.get::<_, String>(1)?, r.get::<_, String>(2)?),
                stamp: stamp_of(&r.get::<_, String>(3)?),
                files: HashMap::new(),
            },
        ));
    }
    let list = names
        .iter()
        .chain(&["metadata"])
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut files = conn.prepare(&format!(
        "SELECT name, data FROM entry_file WHERE entry_id = ?1 AND name IN ({list})"
    ))?;
    for (id, l) in &mut cands {
        let mut rows = files.query([*id])?;
        while let Some(r) = rows.next()? {
            l.files.insert(r.get(0)?, r.get(1)?);
        }
    }
    Ok(cands.into_iter().map(|(_, l)| l).collect())
}

fn data_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA data_version", [], |r| r.get(0))
}

fn no_generation(path: &Path) -> Error {
    Error::Corrupt(format!(
        "{}: meta.generation missing or not a number",
        path.display()
    ))
}

/// `meta.generation`; `None` when missing or not a number.
fn generation_of(conn: &Connection) -> rusqlite::Result<Option<u64>> {
    let g: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'generation'", [], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(g.and_then(|g| g.parse().ok()))
}

/// One read transaction: `data_version` (read inside it, so it matches
/// the data), `generation` and every live entry with its normalised
/// fields (one `SELECT` over `entry` joined to `entry_file` restricted to
/// the 23 field names and `metadata`).
fn load_full(conn: &Connection, path: &Path) -> Result<(i64, u64, Full)> {
    let wrap = |e| db_err(path, e);
    let (dv, generation, loaded) = (|| {
        let tx = conn.unchecked_transaction()?;
        let dv = data_version(&tx)?;
        let generation = generation_of(&tx)?;
        let loaded = load_live(&tx, METADATA_FILE_FIELDS)?;
        Ok((dv, generation, loaded))
    })()
    .map_err(wrap)?;
    let generation = generation.ok_or_else(|| no_generation(path))?;
    let mut by_category: HashMap<String, Vec<String>> = HashMap::new();
    let mut categories: Vec<String> = Vec::new();
    let entries = loaded
        .into_iter()
        .map(|l| {
            match by_category.get_mut(&l.key.category) {
                Some(v) => v.push(l.key.pf.clone()),
                None => {
                    categories.push(l.key.category.clone());
                    by_category.insert(l.key.category.clone(), vec![l.key.pf.clone()]);
                }
            }
            let snap = l.snapshot();
            let fields = EntryFields::from_pairs(
                METADATA_FILE_FIELDS
                    .iter()
                    .map(|&f| (f, l.field(snap.as_ref(), f))),
            );
            (l.key, Arc::new(fields))
        })
        .collect();
    Ok((
        dv,
        generation,
        Full {
            snapshot: Arc::new(Snapshot::new(generation, entries)),
            by_category,
            categories,
        },
    ))
}

fn file_meta_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileMeta> {
    Ok(FileMeta {
        name: r.get(0)?,
        len: r.get::<_, i64>(1)?.max(0) as u64,
        mode: to_u32(r.get(2)?)?,
        mtime_ns: i128::from(r.get::<_, i64>(3)?),
    })
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

    // ------------------------------------------------------------------
    // The read side against `FilesDb`: the database is seeded with raw SQL
    // from what a `FilesDb` (written through its own `WriteTxn`) shows,
    // and every read is compared.

    use crate::{FilesDb, METADATA_FILE_FIELDS};

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

    const BAD: &[u8] = b"caf\xe9 x\n";

    /// `(seq, path, kind, md5, mtime, target)` of the owner rows of `k`.
    type OwnerRow = (
        i64,
        Vec<u8>,
        String,
        Option<String>,
        Option<i64>,
        Option<Vec<u8>>,
    );

    type Columns = (Option<String>, Option<String>, Option<String>, Option<i64>);

    const CONTENTS_A: &[u8] = b"dir /usr\nobj /usr/bin/x d41d8cd98f00b204e9800998ecf8427e 1700\n\
sym /usr/bin/y -> x 1701\nfoo /usr/z\ndev /dev/n\n";
    const NEEDED_A: &[u8] =
        b"X86_64;/usr/lib/liba.so;liba.so.1;/opt/a:/usr/lib;libc.so.6,libb.so\n\
bad line\nX86_64;/usr/bin/x;;  -  ;liba.so.1\n";

    fn a_files() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("SLOT", b"2/3.4\n"),
            ("repository", b"gentoo\n"),
            ("COUNTER", b"41\n"),
            ("CONTENTS", CONTENTS_A),
            ("NEEDED.ELF.2", NEEDED_A),
            (
                "RDEPEND",
                b"!<dev-libs/old-2 ssl? ( >=dev-libs/ssl-1.1:0=[static] ) || ( a/b c/d )\n",
            ),
            ("DEPEND", b"dev-libs/ssl\n"),
            ("BDEPEND", b""),
            ("USE", b"ssl\n"),
        ]
    }

    // ------------------------------------------------------------------
    // The read cache, validated by `PRAGMA data_version`.

    /// What the cached reads show, for comparison with `FilesDb`.
    fn cached_view(db: &impl InstalledDb, keys: &[EntryKey]) -> String {
        let mut out = format!(
            "{:?} {:?}\n",
            db.entries().unwrap(),
            db.categories().unwrap()
        );
        for k in keys {
            out += &format!(
                "{} {:?} {:?}\n",
                db.has_entry(k).unwrap(),
                db.category_entries(&k.category).unwrap(),
                METADATA_FILE_FIELDS
                    .iter()
                    .map(|f| db.aux_get(k, f).unwrap())
                    .collect::<Vec<_>>()
            );
        }
        out
    }
}
