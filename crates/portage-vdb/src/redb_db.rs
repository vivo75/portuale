//! The redb backend (feat#157, backlog #305 S5.2: tables, open and the
//! read side; S5.3: writes). Design: `docs/vdb_to_db.md` §4, §7.2.
//!
//! One redb file, typed tables, one write transaction per commit (S5.3).
//! The file tables are the truth; `owner`, `dep_atom` and `needed` are
//! derived from them and are filled by the writer (S5.3), like on sqlite.
//! `meta.schema_version` ([`SCHEMA_VERSION`]) is independent of
//! `METADATA_FILE_FORMAT_VERSION` and of the sqlite schema version.
//!
//! # One process only
//!
//! redb takes an exclusive file lock for a read-write handle and refuses a
//! second open of the file, in this or another process
//! (`DatabaseError::DatabaseAlreadyOpen`; opening it unsafely corrupts the
//! data). Both [`RedbDb::open`] and [`RedbDb::open_readonly`] map that to
//! [`Error::Busy`], which names the file and says that redb allows one
//! process at a time. A read-only handle (`redb::ReadOnlyDatabase`) takes a
//! shared lock: several read-only handles may coexist, none with a
//! read-write one. A file that was not closed cleanly cannot be opened
//! read-only (redb has to repair it, which needs write access); that is
//! [`Error::Backend`] telling the caller to open it read-write once.
//!
//! # Tables (all keys compare as bytes, so lists come out in the order
//! `FilesDb`'s sorted directory reads give)
//!
//! | table | kind | key | value |
//! |---|---|---|---|
//! | `entry` | table | `(state: u8, category, pf)`; state `0` = merging, `1` = installed | `EntryRec` |
//! | `entry_by_id` | table | `id: u64` | `(state: u8, category, pf)` |
//! | `entry_by_counter` | table | `counter: i64` | `id: u64` |
//! | `entry_file_meta` | table | `(id, name)` | `FileRec` (20 bytes) |
//! | `entry_file_chunk` | table | `(id, name, chunk: u32)` | up to [`CHUNK`] bytes |
//! | `owner` | multimap | path bytes | row, see `write.rs` |
//! | `dep_atom` | multimap | `cp` | row, see `write.rs` |
//! | `needed` | multimap | `id: u64` | row, see `write.rs` |
//! | `world`, `world_sets` | table | `pos: u32` | atom / set name |
//! | `preserved_lib` | table | registry key `cp:slot` | `PlibRec` |
//! | `config_memory` | table | path | md5 |
//! | `meta` | table | name | 8-byte little-endian integer (text for `imported_files_source`) |
//!
//! Differences from `vdb_to_db.md` §7.2, all from the current interface
//! (lib.rs module doc) and from sqlite's model:
//!
//! - `entry` is keyed by `(state, category, pf)`: the interface has no
//!   `pn`/`ver` split (item 1), and a pending entry is its own row beside a
//!   live row of the same key (item 3, sqlite's `UNIQUE (category, pf,
//!   state)`). State first, so "every live entry in order" and "every
//!   pending entry" are single range scans.
//! - `entry_by_counter` is not unique in sqlite's sense: a corpus can hold
//!   duplicate counters, so the writer (S5.3) keeps the first claimant; no
//!   read uses it.
//! - Entry ids are never stored in `meta`: the next id is the last key of
//!   `entry_by_id` plus one.
//! - `world` / `world_sets` are keyed by position (the written order is
//!   kept), and `preserved_lib` is one record per registry key holding the
//!   path list (a multimap would sort the paths and lose their order).
//! - The `metadata` stamp state, the directory mode and the directory mtime
//!   live in `EntryRec`, as sqlite's `entry` columns.
//!
//! Values are fixed binary encodings (little-endian integers, `u32`-length
//! prefixed strings), written and read by the small `Enc` / `Dec` helpers
//! below; there is no serde dependency.
//!
//! - `EntryRec`: `id u64`, `counter` (flag `u8`, `i64`), `stamp u8`
//!   (0 absent, 1 valid, 2 stale), `dir_mode u32`, `dir_mtime_ns i64`, then
//!   `slot`, `subslot`, `repo` strings. The three strings and the counter
//!   are the extracted columns sqlite has; reads do not use them.
//! - `FileRec`: `len u64`, `mode u32`, `mtime_ns i64`.
//! - Entry file contents are split in chunks of [`CHUNK`] (64 KiB); a file
//!   of `len` bytes has `ceil(len / CHUNK)` chunks `0..n` (an empty file has
//!   none), every chunk but the last full. `read_file_at` loads only the
//!   chunks the range touches, and a missing or short chunk is
//!   [`Error::Corrupt`].
//! - `PlibRec`: `cpv`, `counter` strings, `n u32`, then `n` path strings.
//!
//! # Reads and the cache
//!
//! Every read has the semantics of [`crate::SqliteDb`] (and so of `FilesDb`,
//! the reference): only `installed` rows are live, `read_pending_file`
//! reads the `merging` row, `aux_get` follows the stored stamp (the shared
//! rule in `loaded.rs`), `owners` is computed from the stored `CONTENTS`
//! with `FilesDb`'s line rule, `category_generation` is the global
//! `generation`, `counter` is `meta.counter_hwm` with `-1` reading as
//! `None`. Known difference, as sqlite: `categories` lists only categories
//! with a live entry.
//!
//! Each call is one redb read transaction (a snapshot, concurrent with a
//! writer). The in-process cache holds, for one `meta.generation`, the
//! listing (`entries`, `categories`, `category_entries`) and the
//! normalised 23 fields of every live entry (`aux_get`, `snapshot`). **It
//! is validated by comparing `generation`, read inside the same read
//! transaction as the data, on every cached call (one point lookup).**
//! redb is single-process, so the only writer of the file is this handle's
//! own commits; the S5.3 writer bumps `generation` on every effective
//! commit (the policy of sqlite), so nothing else needs to invalidate the
//! cache. A read-only handle never sees a commit (nobody can hold the file
//! read-write while it is open). `has_entry`, `list_files`, `file_meta`,
//! `read_file*` and the D4 stores read the tables directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{
    Database, MultimapTableDefinition, ReadOnlyDatabase, ReadOnlyTable, ReadTransaction,
    ReadableDatabase, ReadableTable, TableDefinition,
};

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
/// Bump it with any incompatible change to the tables or encodings; an
/// existing database with another value is refused, never migrated.
pub const SCHEMA_VERSION: u32 = 1;

/// Size of one stored chunk of file data (design §4: FUSE offset reads load
/// one chunk, not the whole value).
pub const CHUNK: usize = 64 * 1024;

/// `entry` state byte: a pending (`merging`) entry.
const PENDING: u8 = 0;
/// `entry` state byte: a live (`installed`) entry.
const INSTALLED: u8 = 1;

type EntryTable = TableDefinition<'static, (u8, &'static str, &'static str), &'static [u8]>;
type ById = TableDefinition<'static, u64, (u8, &'static str, &'static str)>;

const ENTRY: EntryTable = TableDefinition::new("entry");
const ENTRY_BY_ID: ById = TableDefinition::new("entry_by_id");
const ENTRY_BY_COUNTER: TableDefinition<i64, u64> = TableDefinition::new("entry_by_counter");
const ENTRY_FILE_META: TableDefinition<(u64, &str), &[u8]> =
    TableDefinition::new("entry_file_meta");
const ENTRY_FILE_CHUNK: TableDefinition<(u64, &str, u32), &[u8]> =
    TableDefinition::new("entry_file_chunk");
const OWNER: MultimapTableDefinition<&[u8], &[u8]> = MultimapTableDefinition::new("owner");
const DEP_ATOM: MultimapTableDefinition<&str, &[u8]> = MultimapTableDefinition::new("dep_atom");
const NEEDED: MultimapTableDefinition<u64, &[u8]> = MultimapTableDefinition::new("needed");
const WORLD: TableDefinition<u32, &str> = TableDefinition::new("world");
const WORLD_SETS: TableDefinition<u32, &str> = TableDefinition::new("world_sets");
const PRESERVED_LIB: TableDefinition<&str, &[u8]> = TableDefinition::new("preserved_lib");
const CONFIG_MEMORY: TableDefinition<&str, &str> = TableDefinition::new("config_memory");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

/// A redb failure inside a read. `redb::Error` is the one type every redb
/// error converts to; [`rerr`] turns it into this crate's [`Error`] at the
/// call boundary (so `?` works on any redb call and on [`corrupt`]).
type R<T> = std::result::Result<T, redb::Error>;

fn corrupt(what: impl Into<String>) -> redb::Error {
    redb::Error::Corrupted(what.into())
}

/// Map a redb error to this crate's, naming `path`.
fn rerr(path: &Path, e: redb::Error) -> Error {
    match e {
        redb::Error::DatabaseAlreadyOpen => Error::Busy {
            path: path.to_path_buf(),
        },
        redb::Error::Corrupted(m) => Error::Corrupt(format!("{}: {m}", path.display())),
        redb::Error::UpgradeRequired(v) => Error::Corrupt(format!(
            "{}: redb file format {v} needs a manual upgrade",
            path.display()
        )),
        redb::Error::RepairAborted => Error::Backend(format!(
            "{}: the file was not closed cleanly and needs a repair; \
             open it read-write once (a read-only open cannot repair it)",
            path.display()
        )),
        // `Not a redb database: magic number mismatch` is InvalidData.
        redb::Error::Io(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            Error::Corrupt(format!("{}: {e}", path.display()))
        }
        redb::Error::Io(e) => Error::io(path, e),
        e => Error::Backend(format!("{}: {e}", path.display())),
    }
}

// ---------------------------------------------------------------------
// Value encodings.

/// Little-endian writer for the record encodings.
#[derive(Default)]
struct Enc(Vec<u8>);

impl Enc {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i64(mut self, v: i64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(mut self, v: &[u8]) -> Self {
        self = self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
    fn str(self, v: &str) -> Self {
        self.bytes(v.as_bytes())
    }
    /// Flag byte, then the value when present.
    fn opt_str(self, v: Option<&str>) -> Self {
        match v {
            Some(s) => self.u8(1).str(s),
            None => self.u8(0),
        }
    }
    fn opt_i64(self, v: Option<i64>) -> Self {
        match v {
            Some(n) => self.u8(1).i64(n),
            None => self.u8(0),
        }
    }
}

/// Reader for the record encodings; every getter is `None` on short or
/// invalid data.
struct Dec<'a>(&'a [u8]);

impl<'a> Dec<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

/// The `entry` value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryRec {
    id: u64,
    counter: Option<i64>,
    stamp: MetadataStamp,
    dir_mode: u32,
    dir_mtime_ns: i64,
    slot: String,
    subslot: String,
    repo: String,
}

fn stamp_byte(s: MetadataStamp) -> u8 {
    match s {
        MetadataStamp::Absent => 0,
        MetadataStamp::Valid => 1,
        MetadataStamp::Stale => 2,
    }
}

impl EntryRec {
    fn encode(&self) -> Vec<u8> {
        let e = Enc::default().u64(self.id);
        let e = match self.counter {
            Some(c) => e.u8(1).i64(c),
            None => e.u8(0).i64(0),
        };
        e.u8(stamp_byte(self.stamp))
            .u32(self.dir_mode)
            .i64(self.dir_mtime_ns)
            .str(&self.slot)
            .str(&self.subslot)
            .str(&self.repo)
            .0
    }

    fn decode(b: &[u8]) -> R<Self> {
        let mut d = Dec(b);
        let rec = (|| {
            let id = d.u64()?;
            let (has, c) = (d.u8()?, d.i64()?);
            let stamp = match d.u8()? {
                0 => MetadataStamp::Absent,
                1 => MetadataStamp::Valid,
                2 => MetadataStamp::Stale,
                _ => return None,
            };
            Some(EntryRec {
                id,
                counter: (has == 1).then_some(c),
                stamp,
                dir_mode: d.u32()?,
                dir_mtime_ns: d.i64()?,
                slot: d.str()?,
                subslot: d.str()?,
                repo: d.str()?,
            })
        })();
        match rec {
            Some(r) if d.done() => Ok(r),
            _ => Err(corrupt("bad entry record")),
        }
    }
}

/// The `entry_file_meta` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileRec {
    len: u64,
    mode: u32,
    mtime_ns: i64,
}

impl FileRec {
    fn encode(&self) -> Vec<u8> {
        Enc::default()
            .u64(self.len)
            .u32(self.mode)
            .i64(self.mtime_ns)
            .0
    }

    fn decode(b: &[u8]) -> R<Self> {
        let mut d = Dec(b);
        let rec = (|| {
            Some(FileRec {
                len: d.u64()?,
                mode: d.u32()?,
                mtime_ns: d.i64()?,
            })
        })();
        match rec {
            Some(r) if d.done() => Ok(r),
            _ => Err(corrupt("bad file record")),
        }
    }
}

/// The `preserved_lib` value.
fn decode_plib(b: &[u8]) -> R<PreservedLibsEntry> {
    let mut d = Dec(b);
    let rec = (|| {
        let (cpv, counter) = (d.str()?, d.str()?);
        let n = d.u32()?;
        let mut paths = Vec::new();
        for _ in 0..n {
            paths.push(d.str()?);
        }
        Some(PreservedLibsEntry {
            cpv,
            counter,
            paths,
        })
    })();
    match rec {
        Some(r) if d.done() => Ok(r),
        _ => Err(corrupt("bad preserved_lib record")),
    }
}

fn meta_u64(b: &[u8], what: &str) -> R<u64> {
    <[u8; 8]>::try_from(b)
        .map(u64::from_le_bytes)
        .map_err(|_| corrupt(format!("meta.{what} is not an 8-byte number")))
}

// ---------------------------------------------------------------------
// The handle.

enum Handle {
    Rw(Database),
    Ro(ReadOnlyDatabase),
}

impl Handle {
    fn begin_read(&self) -> std::result::Result<ReadTransaction, redb::TransactionError> {
        match self {
            Handle::Rw(d) => d.begin_read(),
            Handle::Ro(d) => d.begin_read(),
        }
    }
}

/// The redb backend. One file, one handle; see the module doc for the
/// one-process rule.
pub struct RedbDb {
    path: PathBuf,
    handle: Handle,
    /// The in-process read cache ([`CacheState`]).
    cache: Mutex<CacheState>,
    /// The thread that holds the open write transaction, if any (see
    /// `begin_write`).
    writer: Mutex<Option<std::thread::ThreadId>>,
}

/// What the read cache holds for one `meta.generation` value.
#[derive(Default)]
struct CacheState {
    generation: Option<u64>,
    listing: Option<Arc<Listing>>,
    full: Option<Arc<Snapshot>>,
}

/// Every live entry in `(category, pf)` order with what the other reads
/// need to find its files.
#[derive(Default)]
struct Listing {
    entries: Vec<EntryKey>,
    /// `(id, stamp)` of `entries[i]`.
    rows: Vec<(u64, MetadataStamp)>,
    /// `pf` lists per category, in `pf` order.
    by_category: HashMap<String, Vec<String>>,
    /// Categories with a live entry, sorted.
    categories: Vec<String>,
}

impl std::fmt::Debug for RedbDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedbDb")
            .field("path", &self.path)
            .field("readonly", &self.is_readonly())
            .finish()
    }
}

impl RedbDb {
    /// Open `path` read-write, creating and seeding it when missing (an
    /// empty existing file counts as missing): `schema_version`,
    /// `generation` 0, `counter_hwm` -1 and `created_at` in `meta`, and
    /// every table. An existing database whose `meta.schema_version` is not
    /// [`SCHEMA_VERSION`], a redb file with other tables and a file that is
    /// not a redb database are [`Error::Corrupt`]. A file already open in
    /// this or another process is [`Error::Busy`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let wrap = |e: redb::DatabaseError| rerr(&path, e.into());
        let db = Database::create(&path).map_err(wrap)?;
        let fresh = (|| -> R<bool> {
            let txn = db.begin_read()?;
            Ok(txn.list_tables()?.next().is_none() && txn.list_multimap_tables()?.next().is_none())
        })()
        .map_err(|e| rerr(&path, e))?;
        let handle = Handle::Rw(db);
        if fresh {
            seed(&handle, &path)?;
        } else {
            check_version(&handle, &path)?;
        }
        Ok(Self::with_handle(path, handle))
    }

    /// Open an existing database read-only (`redb::ReadOnlyDatabase`: a
    /// shared lock, so it fails with [`Error::Busy`] while a read-write
    /// handle holds the file, and any number of read-only handles may
    /// coexist). A missing file is [`Error::Io`] (`NotFound`); the schema
    /// version is checked as in [`RedbDb::open`]. Nothing is created or
    /// modified; `begin_write` is refused.
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Err(e) = std::fs::metadata(&path) {
            return Err(Error::io(&path, e));
        }
        let db = ReadOnlyDatabase::open(&path).map_err(|e| rerr(&path, e.into()))?;
        let handle = Handle::Ro(db);
        check_version(&handle, &path)?;
        Ok(Self::with_handle(path, handle))
    }

    fn with_handle(path: PathBuf, handle: Handle) -> Self {
        RedbDb {
            path,
            handle,
            cache: Mutex::new(CacheState::default()),
            writer: Mutex::new(None),
        }
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this handle was opened with [`RedbDb::open_readonly`].
    pub fn is_readonly(&self) -> bool {
        matches!(self.handle, Handle::Ro(_))
    }

    /// Run `f` in one read transaction; redb errors become this crate's.
    fn read<T>(&self, f: impl FnOnce(&Rd) -> R<T>) -> Result<T> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let rd = Rd::new(&txn).map_err(wrap)?;
        f(&rd).map_err(wrap)
    }

    /// Drop the read cache. Not needed after a commit that bumps
    /// `generation` (the cache is validated against it); for tests that
    /// write tables raw.
    #[cfg(test)]
    fn invalidate(&self) {
        *self.cache.lock().unwrap() = CacheState::default();
    }

    /// The validated cache: `(generation, listing, snapshot)`; the
    /// snapshot is loaded only when `want_full`. See the module doc.
    fn cached(&self, want_full: bool) -> Result<(u64, Arc<Listing>, Option<Arc<Snapshot>>)> {
        let wrap = |e| rerr(&self.path, e);
        let mut st = self.cache.lock().map_err(|_| {
            Error::Backend(format!("{}: cache mutex poisoned", self.path.display()))
        })?;
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let rd = Rd::new(&txn).map_err(wrap)?;
        let generation = rd.generation(&txn).map_err(wrap)?;
        if st.generation != Some(generation) {
            *st = CacheState::default();
        }
        if st.listing.is_none() {
            st.listing = Some(Arc::new(rd.listing().map_err(wrap)?));
        }
        let listing = st.listing.clone().unwrap_or_default();
        if want_full && st.full.is_none() {
            st.full = Some(Arc::new(rd.snapshot(generation, &listing).map_err(wrap)?));
        }
        st.generation = Some(generation);
        Ok((generation, listing, st.full.clone()))
    }
}

/// Create every table and write the `meta` seed, in one transaction.
fn seed(handle: &Handle, path: &Path) -> Result<()> {
    let Handle::Rw(db) = handle else {
        return Err(Error::Invalid(format!(
            "{}: opened read-only",
            path.display()
        )));
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (|| -> R<()> {
        let txn = db.begin_write()?;
        txn.open_table(ENTRY)?;
        txn.open_table(ENTRY_BY_ID)?;
        txn.open_table(ENTRY_BY_COUNTER)?;
        txn.open_table(ENTRY_FILE_META)?;
        txn.open_table(ENTRY_FILE_CHUNK)?;
        txn.open_multimap_table(OWNER)?;
        txn.open_multimap_table(DEP_ATOM)?;
        txn.open_multimap_table(NEEDED)?;
        txn.open_table(WORLD)?;
        txn.open_table(WORLD_SETS)?;
        txn.open_table(PRESERVED_LIB)?;
        txn.open_table(CONFIG_MEMORY)?;
        {
            let mut meta = txn.open_table(META)?;
            // counter_hwm: highest counter handed out; -1 = none yet.
            meta.insert(
                "schema_version",
                u64::from(SCHEMA_VERSION).to_le_bytes().as_slice(),
            )?;
            meta.insert("generation", 0u64.to_le_bytes().as_slice())?;
            meta.insert("counter_hwm", (-1i64).to_le_bytes().as_slice())?;
            meta.insert("created_at", now.to_le_bytes().as_slice())?;
        }
        txn.commit()?;
        Ok(())
    })()
    .map_err(|e| rerr(path, e))
}

fn check_version(handle: &Handle, path: &Path) -> Result<()> {
    let foreign = || {
        Error::Corrupt(format!(
            "{}: not a portuale VDB (no meta.schema_version)",
            path.display()
        ))
    };
    let txn = handle.begin_read().map_err(|e| rerr(path, e.into()))?;
    let meta = match txn.open_table(META) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Err(foreign()),
        Err(e) => return Err(rerr(path, e.into())),
    };
    let found = meta
        .get("schema_version")
        .map_err(|e| rerr(path, e.into()))?
        .map(|g| meta_u64(g.value(), "schema_version"))
        .transpose()
        .map_err(|e| rerr(path, e))?;
    match found {
        Some(v) if v == u64::from(SCHEMA_VERSION) => Ok(()),
        Some(v) => Err(Error::Corrupt(format!(
            "{}: schema_version {v}, this build supports {SCHEMA_VERSION}",
            path.display()
        ))),
        None => Err(foreign()),
    }
}

mod write;

// ---------------------------------------------------------------------
// Reads inside one transaction.

/// The three tables most reads need, opened once per read transaction.
struct Rd {
    entry: ReadOnlyTable<(u8, &'static str, &'static str), &'static [u8]>,
    fmeta: ReadOnlyTable<(u64, &'static str), &'static [u8]>,
    chunk: ReadOnlyTable<(u64, &'static str, u32), &'static [u8]>,
}

impl Rd {
    fn new(txn: &ReadTransaction) -> R<Self> {
        Ok(Rd {
            entry: txn.open_table(ENTRY)?,
            fmeta: txn.open_table(ENTRY_FILE_META)?,
            chunk: txn.open_table(ENTRY_FILE_CHUNK)?,
        })
    }

    /// `meta.generation`, read in `txn` (the same snapshot as the data).
    fn generation(&self, txn: &ReadTransaction) -> R<u64> {
        let meta = txn.open_table(META)?;
        match meta.get("generation")? {
            Some(g) => meta_u64(g.value(), "generation"),
            None => Err(corrupt("meta.generation missing")),
        }
    }

    fn rec(&self, state: u8, key: &EntryKey) -> R<Option<EntryRec>> {
        self.entry
            .get((state, key.category.as_str(), key.pf.as_str()))?
            .map(|g| EntryRec::decode(g.value()))
            .transpose()
    }

    /// Every row of `state`, in `(category, pf)` order.
    fn rows(&self, state: u8) -> R<Vec<(EntryKey, EntryRec)>> {
        let mut out = Vec::new();
        for item in self.entry.range((state, "", "")..(state + 1, "", ""))? {
            let (k, v) = item?;
            let (_, category, pf) = k.value();
            out.push((EntryKey::new(category, pf), EntryRec::decode(v.value())?));
        }
        Ok(out)
    }

    fn listing(&self) -> R<Listing> {
        let mut l = Listing::default();
        for (key, rec) in self.rows(INSTALLED)? {
            match l.by_category.get_mut(&key.category) {
                Some(v) => v.push(key.pf.clone()),
                None => {
                    l.categories.push(key.category.clone());
                    l.by_category
                        .insert(key.category.clone(), vec![key.pf.clone()]);
                }
            }
            l.rows.push((rec.id, rec.stamp));
            l.entries.push(key);
        }
        Ok(l)
    }

    fn file_rec(&self, id: u64, name: &str) -> R<Option<FileRec>> {
        self.fmeta
            .get((id, name))?
            .map(|g| FileRec::decode(g.value()))
            .transpose()
    }

    /// The files of entry `id`, by name.
    fn file_metas(&self, id: u64) -> R<Vec<(String, FileRec)>> {
        let mut out = Vec::new();
        for item in self.fmeta.range((id, "")..)? {
            let (k, v) = item?;
            let (kid, name) = k.value();
            if kid != id {
                break;
            }
            out.push((name.to_owned(), FileRec::decode(v.value())?));
        }
        Ok(out)
    }

    /// Chunks `first..=last` of a file of `len` bytes, concatenated; every
    /// one must exist and have its exact size.
    fn chunks(&self, id: u64, name: &str, len: u64, first: u32, last: u32) -> R<Vec<u8>> {
        let mut out = Vec::new();
        let mut next = first;
        for item in self.chunk.range((id, name, first)..=(id, name, last))? {
            let (k, v) = item?;
            let (_, _, n) = k.value();
            let data = v.value();
            let want = chunk_len(len, n);
            if n != next || data.len() != want {
                return Err(corrupt(format!(
                    "entry {id} file {name:?}: chunk {n} has {} bytes, expected {want} (chunk {next} due)",
                    data.len()
                )));
            }
            out.extend_from_slice(data);
            next = n.wrapping_add(1);
        }
        if u64::from(next) != u64::from(last) + 1 {
            return Err(corrupt(format!(
                "entry {id} file {name:?}: chunk {next} is missing"
            )));
        }
        Ok(out)
    }

    /// The whole file `name` of entry `id`, `None` when it has no record.
    fn file(&self, id: u64, name: &str) -> R<Option<Vec<u8>>> {
        let Some(rec) = self.file_rec(id, name)? else {
            return Ok(None);
        };
        if rec.len == 0 {
            return Ok(Some(Vec::new()));
        }
        let last = chunk_count(rec.len) - 1;
        self.chunks(id, name, rec.len, 0, last).map(Some)
    }

    /// Up to `n` bytes from `off`: only the chunks that range touches.
    fn file_at(&self, id: u64, name: &str, off: u64, n: usize) -> R<Option<Vec<u8>>> {
        let Some(rec) = self.file_rec(id, name)? else {
            return Ok(None);
        };
        let end = off.saturating_add(n as u64).min(rec.len);
        if off >= end {
            return Ok(Some(Vec::new()));
        }
        let first = (off / CHUNK as u64) as u32;
        let last = ((end - 1) / CHUNK as u64) as u32;
        let data = self.chunks(id, name, rec.len, first, last)?;
        let skip = (off - u64::from(first) * CHUNK as u64) as usize;
        Ok(Some(data[skip..skip + (end - off) as usize].to_vec()))
    }

    /// The stored files of `id` named in `names` plus `metadata`.
    fn load_named(&self, id: u64, names: &[&str]) -> R<HashMap<String, Vec<u8>>> {
        let mut files = HashMap::new();
        for (name, _) in self.file_metas(id)? {
            if (name == "metadata" || names.contains(&name.as_str()))
                && let Some(data) = self.file(id, &name)?
            {
                files.insert(name, data);
            }
        }
        Ok(files)
    }

    /// The 23 normalised fields of every entry of `listing`.
    fn snapshot(&self, generation: u64, listing: &Listing) -> R<Snapshot> {
        let mut entries = Vec::with_capacity(listing.entries.len());
        for (key, &(id, stamp)) in listing.entries.iter().zip(&listing.rows) {
            let l = Loaded {
                key: key.clone(),
                stamp,
                files: self.load_named(id, METADATA_FILE_FIELDS)?,
            };
            let snap = l.snapshot();
            let fields = EntryFields::from_pairs(
                METADATA_FILE_FIELDS
                    .iter()
                    .map(|&f| (f, l.field(snap.as_ref(), f))),
            );
            entries.push((l.key, Arc::new(fields)));
        }
        Ok(Snapshot::new(generation, entries))
    }
}

/// Chunks of a file of `len` bytes.
fn chunk_count(len: u64) -> u32 {
    len.div_ceil(CHUNK as u64) as u32
}

/// Size of chunk `n` of a file of `len` bytes (0 past the end).
fn chunk_len(len: u64, n: u32) -> usize {
    let start = u64::from(n) * CHUNK as u64;
    len.saturating_sub(start).min(CHUNK as u64) as usize
}

fn file_meta_of(name: &str, rec: FileRec) -> FileMeta {
    FileMeta {
        name: name.to_owned(),
        len: rec.len,
        mode: rec.mode,
        mtime_ns: i128::from(rec.mtime_ns),
    }
}

impl InstalledDb for RedbDb {
    fn kind(&self) -> BackendKind {
        BackendKind::Redb
    }
    fn vdb_dir(&self) -> Option<PathBuf> {
        None
    }
    fn entry_path(&self, _key: &EntryKey) -> Option<PathBuf> {
        None
    }

    /// `meta.generation`: +1 on every effective commit (S5.3). One point
    /// lookup.
    fn generation(&self) -> Result<u64> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let rd = Rd::new(&txn).map_err(wrap)?;
        rd.generation(&txn).map_err(wrap)
    }

    /// The global generation, as sqlite: it moves on every commit, so a
    /// category-keyed cache is invalidated more often than needed, never
    /// less.
    fn category_generation(&self, _category: &str) -> Result<u64> {
        self.generation()
    }

    fn entries(&self) -> Result<Vec<EntryKey>> {
        Ok(self.cached(false)?.1.entries.clone())
    }

    /// Categories that hold at least one live entry (`files` also lists a
    /// category directory that is empty or holds only a pending entry).
    fn categories(&self) -> Result<Vec<String>> {
        Ok(self.cached(false)?.1.categories.clone())
    }

    fn category_entries(&self, category: &str) -> Result<Vec<String>> {
        Ok(self
            .cached(false)?
            .1
            .by_category
            .get(category)
            .cloned()
            .unwrap_or_default())
    }

    fn pending_entries(&self) -> Result<Vec<EntryKey>> {
        self.read(|rd| Ok(rd.rows(PENDING)?.into_iter().map(|(k, _)| k).collect()))
    }

    fn has_entry(&self, key: &EntryKey) -> Result<bool> {
        self.read(|rd| Ok(rd.rec(INSTALLED, key)?.is_some()))
    }

    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>> {
        if !in_metadata_file(field) {
            return Err(Error::Invalid(format!(
                "aux_get key {field:?} is not one of the 23 metadata fields (use read_file)"
            )));
        }
        let full = self.cached(true)?.2;
        Ok(full
            .and_then(|s| s.get(key).cloned())
            .and_then(|f| f.get(field).map(str::to_owned)))
    }

    /// The cached snapshot (the 23 normalised fields, snapshot rule per
    /// stored stamp), taken in one read transaction with `generation`.
    fn snapshot(&self) -> Result<Arc<Snapshot>> {
        self.cached(true)?
            .2
            .ok_or_else(|| Error::Backend("read cache not loaded".into()))
    }

    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>> {
        self.read(|rd| {
            let Some(rec) = rd.rec(INSTALLED, key)? else {
                return Ok(None);
            };
            Ok(Some(
                rd.file_metas(rec.id)?
                    .into_iter()
                    .map(|(n, r)| file_meta_of(&n, r))
                    .collect(),
            ))
        })
    }

    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>> {
        self.read(|rd| {
            let Some(rec) = rd.rec(INSTALLED, key)? else {
                return Ok(None);
            };
            Ok(rd.file_rec(rec.id, name)?.map(|r| file_meta_of(name, r)))
        })
    }

    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        self.read(|rd| match rd.rec(INSTALLED, key)? {
            Some(rec) => rd.file(rec.id, name),
            None => Ok(None),
        })
    }

    /// Loads only the chunks `[off, off + len)` touches: `Some(empty)` at
    /// or past the end, short at the end, `None` for a missing entry or
    /// file.
    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>> {
        self.read(|rd| match rd.rec(INSTALLED, key)? {
            Some(rec) => rd.file_at(rec.id, name, off, len),
            None => Ok(None),
        })
    }

    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        self.read(|rd| match rd.rec(PENDING, key)? {
            Some(rec) => rd.file(rec.id, name),
            None => Ok(None),
        })
    }

    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>> {
        self.read(|rd| {
            let mut out = Vec::new();
            for (key, rec) in rd.rows(INSTALLED)? {
                out.push((key, rd.file(rec.id, name)?));
            }
            Ok(out)
        })
    }

    fn entry_stat(&self, key: &EntryKey) -> Result<Option<EntryStat>> {
        self.read(|rd| {
            let Some(rec) = rd.rec(INSTALLED, key)? else {
                return Ok(None);
            };
            let files = rd
                .file_metas(rec.id)?
                .into_iter()
                .map(|(name, frec)| file_meta_of(&name, frec))
                .collect();
            Ok(Some(EntryStat {
                files,
                dir_mode: rec.dir_mode,
                dir_mtime_ns: i128::from(rec.dir_mtime_ns),
                metadata_stamp: rec.stamp,
            }))
        })
    }

    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>> {
        self.read(|rd| {
            let Some(rec) = rd.rec(INSTALLED, key)? else {
                return Ok(None);
            };
            let mut files = Vec::new();
            for (name, frec) in rd.file_metas(rec.id)? {
                let data = rd
                    .file(rec.id, &name)?
                    .ok_or_else(|| corrupt(format!("file {name:?} vanished")))?;
                files.push(EntryFile {
                    meta: file_meta_of(&name, frec),
                    data,
                });
            }
            Ok(Some(EntryImage {
                key: key.clone(),
                files,
                dir_mode: rec.dir_mode,
                dir_mtime_ns: i128::from(rec.dir_mtime_ns),
                metadata_stamp: rec.stamp,
            }))
        })
    }

    /// The live entries whose `dep_atom` rows name `cp` in one of `classes`,
    /// or carry the unsure marker (`cp = ""`, see `dep_cp::dep_index_key`),
    /// in `(category, pf)` order, with their `USE` and the requested classes
    /// normalised like `aux_get`. A superset of the real dependents (lib.rs
    /// module doc, item 8); an entry none of whose tokens is, or might be,
    /// `cp` is not read (S8.1).
    fn reverse_dependents(&self, cp: &str, classes: &[DepClass]) -> Result<Vec<DepRecord>> {
        if classes.is_empty() {
            return Ok(Vec::new());
        }
        let mut names: Vec<&str> = vec!["USE"];
        names.extend(classes.iter().map(|c| c.field()));
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let loaded = (|| -> R<Vec<Loaded>> {
            let rd = Rd::new(&txn)?;
            let want: Vec<u8> = classes
                .iter()
                .filter_map(|c| write::DEP_CLASSES.iter().position(|d| d == c))
                .map(|i| i as u8)
                .collect();
            let mut ids = std::collections::HashSet::new();
            let dep = txn.open_multimap_table(DEP_ATOM)?;
            for key in [cp, UNSURE_CP] {
                for v in dep.get(key)? {
                    let v = v?;
                    let v = v.value();
                    if v.len() < 9 {
                        return Err(corrupt("dep_atom row too short"));
                    }
                    if want.contains(&v[8]) {
                        ids.insert(u64::from_le_bytes(v[..8].try_into().expect("8 bytes")));
                    }
                }
            }
            let mut out = Vec::new();
            for (key, rec) in rd.rows(INSTALLED)? {
                if ids.contains(&rec.id) {
                    out.push(Loaded {
                        files: rd.load_named(rec.id, &names)?,
                        stamp: rec.stamp,
                        key,
                    });
                }
            }
            Ok(out)
        })()
        .map_err(wrap)?;
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

    /// From the `owner` multimap (S8.2): an indexed lookup of each distinct
    /// path (both spellings `CONTENTS` can have, see `owner_spellings`),
    /// with the rule of `FilesDb::owners` (`claim_paths`): one leading `/`
    /// ignored on both sides, the first matching input path reported, the
    /// entries in `(category, pf)` order and, within one, `CONTENTS` order
    /// (`seq`). Only live entries count (a pending entry has no rows).
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        let wanted = owner_wanted(paths);
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let mut hits: Vec<(String, String, u32, &[u8])> = Vec::new();
        (|| -> R<()> {
            let owner = txn.open_multimap_table(OWNER)?;
            let by_id = txn.open_table(ENTRY_BY_ID)?;
            for (&norm, &first) in &wanted {
                for raw in owner_spellings(norm) {
                    for v in owner.get(raw.as_slice())? {
                        let v = v?;
                        let v = v.value();
                        if v.len() < 12 {
                            return Err(corrupt("owner row too short"));
                        }
                        let id = u64::from_le_bytes(v[..8].try_into().expect("8 bytes"));
                        let seq = u32::from_le_bytes(v[8..12].try_into().expect("4 bytes"));
                        if let Some(g) = by_id.get(id)? {
                            let (state, category, pf) = g.value();
                            if state == INSTALLED {
                                hits.push((category.to_owned(), pf.to_owned(), seq, first));
                            }
                        }
                    }
                }
            }
            Ok(())
        })()
        .map_err(wrap)?;
        hits.sort();
        Ok(hits
            .into_iter()
            .map(|(category, pf, _, first)| (first.to_vec(), EntryKey::new(category, pf)))
            .collect())
    }

    /// In the order written (`pos`), like `files`.
    fn world(&self) -> Result<World> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let atoms = read_list(&txn, WORLD).map_err(wrap)?;
        Ok(World { atoms })
    }

    fn world_sets(&self) -> Result<WorldSets> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let sets = read_list(&txn, WORLD_SETS).map_err(wrap)?;
        Ok(WorldSets { sets })
    }

    fn preserved_libs(&self) -> Result<PreservedLibs> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let entries = (|| -> R<_> {
            let t = txn.open_table(PRESERVED_LIB)?;
            let mut m = std::collections::BTreeMap::new();
            for item in t.iter()? {
                let (k, v) = item?;
                m.insert(k.value().to_owned(), decode_plib(v.value())?);
            }
            Ok(m)
        })()
        .map_err(wrap)?;
        Ok(PreservedLibs {
            loaded: entries.clone(),
            entries,
        })
    }

    fn config_memory(&self) -> Result<ConfigMemory> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        let entries = (|| -> R<_> {
            let t = txn.open_table(CONFIG_MEMORY)?;
            let mut m = std::collections::BTreeMap::new();
            for item in t.iter()? {
                let (k, v) = item?;
                m.insert(k.value().to_owned(), v.value().to_owned());
            }
            Ok(m)
        })()
        .map_err(wrap)?;
        Ok(ConfigMemory { entries })
    }

    /// `meta.counter_hwm`; `-1` (nothing handed out yet) is `None`, like a
    /// missing `counter` file on `files`.
    fn counter(&self) -> Result<Option<Counter>> {
        let v = self.meta_get("counter_hwm")?;
        match v {
            None => Ok(None),
            Some(b) => {
                let n = meta_u64(&b, "counter_hwm")
                    .map(|u| u as i64)
                    .map_err(|e| rerr(&self.path, e))?;
                Ok((n >= 0).then_some(Counter(n)))
            }
        }
    }

    /// The generation and source path of the files VDB this database was
    /// converted from; `None` when never converted.
    fn import_mark(&self) -> Result<Option<(u64, String)>> {
        let g = self.meta_get("imported_files_generation")?;
        let s = self.meta_get("imported_files_source")?;
        match (g, s) {
            (Some(g), Some(s)) => {
                let g =
                    meta_u64(&g, "imported_files_generation").map_err(|e| rerr(&self.path, e))?;
                Ok(Some((g, String::from_utf8_lossy(&s).into_owned())))
            }
            _ => Ok(None),
        }
    }

    /// One redb `WriteTransaction` ([`write::RedbTxn`]). redb allows one
    /// write transaction per `Database`: another thread's `begin_write`
    /// blocks until the open one commits or is dropped (redb's own wait).
    /// A second `begin_write` on the thread that already holds one would
    /// wait for itself forever, so it is refused with [`Error::Invalid`]
    /// instead of deadlocking.
    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>> {
        Ok(Box::new(write::RedbTxn::begin(self)?))
    }
}

impl RedbDb {
    /// A `meta` value; `None` when the key is absent.
    fn meta_get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let wrap = |e| rerr(&self.path, e);
        let txn = self.handle.begin_read().map_err(|e| wrap(e.into()))?;
        (|| -> R<_> {
            let meta = txn.open_table(META)?;
            Ok(meta.get(key)?.map(|g| g.value().to_vec()))
        })()
        .map_err(wrap)
    }
}

/// The values of a `pos`-keyed list table, in `pos` order.
fn read_list(txn: &ReadTransaction, def: TableDefinition<u32, &str>) -> R<Vec<String>> {
    let t = txn.open_table(def)?;
    let mut out = Vec::new();
    for item in t.iter()? {
        out.push(item?.1.value().to_owned());
    }
    Ok(out)
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::FilesDb;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let d = std::env::temp_dir().join(format!(
                "portage-vdb-redb-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }
        fn db(&self) -> PathBuf {
            self.0.join("vdb.redb")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rw(db: &RedbDb) -> &Database {
        match &db.handle {
            Handle::Rw(d) => d,
            Handle::Ro(_) => panic!("read-only handle"),
        }
    }

    type Rows = Vec<(String, Vec<u8>, u32, i64)>;

    /// Raw table writers for the tests (the real writer is S5.3).
    impl RedbDb {
        fn seed_entry(&self, key: &EntryKey, state: u8, stamp: MetadataStamp, files: &Rows) {
            let txn = rw(self).begin_write().unwrap();
            let id: u64;
            {
                let mut by_id = txn.open_table(ENTRY_BY_ID).unwrap();
                id = by_id.last().unwrap().map_or(1, |(k, _)| k.value() + 1);
                by_id
                    .insert(id, (state, key.category.as_str(), key.pf.as_str()))
                    .unwrap();
                let rec = EntryRec {
                    id,
                    counter: None,
                    stamp,
                    dir_mode: 493,
                    dir_mtime_ns: 1234,
                    slot: String::new(),
                    subslot: String::new(),
                    repo: String::new(),
                };
                txn.open_table(ENTRY)
                    .unwrap()
                    .insert(
                        (state, key.category.as_str(), key.pf.as_str()),
                        rec.encode().as_slice(),
                    )
                    .unwrap();
                let mut fmeta = txn.open_table(ENTRY_FILE_META).unwrap();
                let mut chunks = txn.open_table(ENTRY_FILE_CHUNK).unwrap();
                for (name, data, mode, mtime) in files {
                    let frec = FileRec {
                        len: data.len() as u64,
                        mode: *mode,
                        mtime_ns: *mtime,
                    };
                    fmeta
                        .insert((id, name.as_str()), frec.encode().as_slice())
                        .unwrap();
                    for (i, c) in data.chunks(CHUNK).enumerate() {
                        chunks.insert((id, name.as_str(), i as u32), c).unwrap();
                    }
                }
            }
            if state == INSTALLED {
                // The owner / dep_atom rows of a live entry, as a real write
                // fills them (the columns stay as seeded).
                for (name, ..) in files {
                    if matches!(name.as_str(), "CONTENTS")
                        || write::DEP_CLASSES.iter().any(|d| d.field() == name)
                    {
                        write::index_file_for_test(&txn, key, id, name);
                    }
                }
            }
            txn.commit().unwrap();
            self.invalidate();
        }

        fn seed_meta(&self, key: &str, v: &[u8]) {
            let txn = rw(self).begin_write().unwrap();
            txn.open_table(META).unwrap().insert(key, v).unwrap();
            txn.commit().unwrap();
            self.invalidate();
        }

        fn seed_list(&self, def: TableDefinition<u32, &str>, items: &[String]) {
            let txn = rw(self).begin_write().unwrap();
            {
                let mut t = txn.open_table(def).unwrap();
                for (i, s) in items.iter().enumerate() {
                    t.insert(i as u32, s.as_str()).unwrap();
                }
            }
            txn.commit().unwrap();
        }

        fn seed_plib(&self, key: &str, e: &PreservedLibsEntry) {
            let mut enc = Enc::default()
                .str(&e.cpv)
                .str(&e.counter)
                .u32(e.paths.len() as u32);
            for p in &e.paths {
                enc = enc.str(p);
            }
            let txn = rw(self).begin_write().unwrap();
            txn.open_table(PRESERVED_LIB)
                .unwrap()
                .insert(key, enc.0.as_slice())
                .unwrap();
            txn.commit().unwrap();
        }

        fn seed_config(&self, k: &str, v: &str) {
            let txn = rw(self).begin_write().unwrap();
            txn.open_table(CONFIG_MEMORY).unwrap().insert(k, v).unwrap();
            txn.commit().unwrap();
        }

        fn meta_num(&self, key: &str) -> u64 {
            meta_u64(&self.meta_get(key).unwrap().unwrap(), key).unwrap()
        }
    }

    // --- open, reopen, schema --------------------------------------

    /// Every row of the three index multimaps, as `(table, key, value)`
    /// bytes, sorted.
    fn index_dump(db: &RedbDb) -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
        let txn = rw(db).begin_read().unwrap();
        let mut out = Vec::new();
        for item in txn.open_multimap_table(OWNER).unwrap().iter().unwrap() {
            let (k, vs) = item.unwrap();
            for v in vs {
                out.push(("owner", k.value().to_vec(), v.unwrap().value().to_vec()));
            }
        }
        for item in txn.open_multimap_table(DEP_ATOM).unwrap().iter().unwrap() {
            let (k, vs) = item.unwrap();
            for v in vs {
                let key = k.value().as_bytes().to_vec();
                out.push(("dep_atom", key, v.unwrap().value().to_vec()));
            }
        }
        for item in txn.open_multimap_table(NEEDED).unwrap().iter().unwrap() {
            let (k, vs) = item.unwrap();
            for v in vs {
                let key = k.value().to_le_bytes().to_vec();
                out.push(("needed", key, v.unwrap().value().to_vec()));
            }
        }
        out.sort();
        out
    }

    /// S8.3: damaged index rows (a missing owner, a foreign owner, the
    /// unsure `dep_atom` marker an older build never wrote (R18), a junk
    /// `needed` row) give wrong answers until `rebuild_index` restores
    /// exactly the rows the merge wrote.
    #[test]
    fn rebuild_index_repairs_damaged_rows() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&a).unwrap();
        tx.put_entry_file(
            &a,
            "CONTENTS",
            b"obj /usr/bin/a abc 1\nobj /usr/bin/b def 2\n",
        )
        .unwrap();
        tx.put_entry_file(&a, "RDEPEND", b"dev-libs/* dev-libs/o\n")
            .unwrap();
        tx.put_entry_file(&a, "NEEDED.ELF.2", b"X86_64;/usr/bin/a;;  -  ;libc.so.6\n")
            .unwrap();
        tx.finish_entry(&a).unwrap();
        tx.commit().unwrap();
        let good = index_dump(&db);
        {
            let txn = rw(&db).begin_write().unwrap();
            {
                let mut o = txn.open_multimap_table(OWNER).unwrap();
                // A well-formed row of `a`, filed under a path it never owned.
                let row = o
                    .get(b"/usr/bin/a".as_slice())
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .value()
                    .to_vec();
                o.remove_all(b"/usr/bin/b".as_slice()).unwrap();
                o.insert(b"/etc/passwd".as_slice(), row.as_slice()).unwrap();
                txn.open_multimap_table(DEP_ATOM)
                    .unwrap()
                    .remove_all("")
                    .unwrap();
                txn.open_multimap_table(NEEDED)
                    .unwrap()
                    .insert(1, b"junk".as_slice())
                    .unwrap();
            }
            txn.commit().unwrap();
            db.invalidate();
        }
        assert_ne!(index_dump(&db), good);
        let owners = |p: &[u8]| db.owners(&[p]).unwrap().len();
        let rdeps = || {
            db.reverse_dependents("dev-libs/zz", &[DepClass::Rdepend])
                .unwrap()
                .len()
        };
        assert_eq!(
            (owners(b"/usr/bin/b"), owners(b"/etc/passwd"), rdeps()),
            (0, 1, 0)
        );
        let mut tx = db.begin_write().unwrap();
        let counts = tx.rebuild_index().unwrap();
        tx.commit().unwrap();
        assert_eq!(
            counts,
            crate::IndexCounts {
                entries: 1,
                owner: 2,
                dep_atom: 2,
                needed: 1
            }
        );
        assert_eq!(index_dump(&db), good);
        assert_eq!(
            (owners(b"/usr/bin/b"), owners(b"/etc/passwd"), rdeps()),
            (1, 0, 1)
        );
    }

    #[test]
    fn fresh_database_has_meta_and_tables() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        assert_eq!(db.meta_num("schema_version"), u64::from(SCHEMA_VERSION));
        assert_eq!(db.meta_num("generation"), 0);
        assert_eq!(db.meta_num("counter_hwm") as i64, -1);
        assert!(db.meta_num("created_at") > 0);
        let txn = db.handle.begin_read().unwrap();
        let mut names: Vec<String> = txn
            .list_tables()
            .unwrap()
            .map(|h| redb::TableHandle::name(&h).to_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "config_memory",
                "entry",
                "entry_by_counter",
                "entry_by_id",
                "entry_file_chunk",
                "entry_file_meta",
                "meta",
                "preserved_lib",
                "world",
                "world_sets"
            ]
        );
        let mut multi: Vec<String> = txn
            .list_multimap_tables()
            .unwrap()
            .map(|h| redb::MultimapTableHandle::name(&h).to_owned())
            .collect();
        multi.sort();
        assert_eq!(multi, ["dep_atom", "needed", "owner"]);
        drop(txn);
        assert_eq!(db.kind(), BackendKind::Redb);
        assert!(db.vdb_dir().is_none());
        assert!(db.entry_path(&EntryKey::new("a", "b-1")).is_none());
        assert!(db.entries().unwrap().is_empty());
        assert_eq!(db.generation().unwrap(), 0);
        assert_eq!(db.counter().unwrap(), None);
    }

    #[test]
    fn reopen_keeps_data_and_readonly_reads_it() {
        let t = Tmp::new();
        {
            let db = RedbDb::open(t.db()).unwrap();
            db.seed_meta("generation", &7u64.to_le_bytes());
        }
        let db = RedbDb::open(t.db()).unwrap();
        assert_eq!(db.generation().unwrap(), 7);
        drop(db);
        let ro = RedbDb::open_readonly(t.db()).unwrap();
        assert!(ro.is_readonly());
        assert_eq!(ro.generation().unwrap(), 7);
        assert!(matches!(ro.begin_write(), Err(Error::Invalid(_))));
        // Several read-only handles may coexist.
        let ro2 = RedbDb::open_readonly(t.db()).unwrap();
        assert_eq!(ro2.generation().unwrap(), 7);
    }

    #[test]
    fn schema_version_mismatch_is_refused() {
        let t = Tmp::new();
        {
            let db = RedbDb::open(t.db()).unwrap();
            db.seed_meta("schema_version", &999u64.to_le_bytes());
        }
        for r in [RedbDb::open(t.db()), RedbDb::open_readonly(t.db())] {
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
    fn foreign_files_are_refused() {
        let t = Tmp::new();
        // A redb file with some other table.
        {
            let db = Database::create(t.db()).unwrap();
            let txn = db.begin_write().unwrap();
            txn.open_table(TableDefinition::<&str, &str>::new("x"))
                .unwrap();
            txn.commit().unwrap();
        }
        assert!(matches!(RedbDb::open(t.db()), Err(Error::Corrupt(_))));
        assert!(matches!(
            RedbDb::open_readonly(t.db()),
            Err(Error::Corrupt(_))
        ));
        // Not a redb file at all.
        let junk = t.0.join("junk");
        std::fs::write(&junk, vec![b'x'; 8192]).unwrap();
        let err = RedbDb::open(&junk).unwrap_err();
        assert!(matches!(err, Error::Corrupt(_)), "{err:?}");
        // An empty existing file counts as missing.
        let empty = t.0.join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(RedbDb::open(&empty).unwrap().generation().unwrap(), 0);
    }

    #[test]
    fn readonly_missing_file_fails_and_creates_nothing() {
        let t = Tmp::new();
        match RedbDb::open_readonly(t.db()) {
            Err(Error::Io { source, .. }) => {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound)
            }
            other => panic!("expected Io NotFound, got {other:?}"),
        }
        assert!(!t.db().exists());
    }

    #[test]
    fn a_second_open_while_the_first_is_alive_is_busy() {
        let t = Tmp::new();
        let first = RedbDb::open(t.db()).unwrap();
        for r in [RedbDb::open(t.db()), RedbDb::open_readonly(t.db())] {
            match r {
                Err(e @ Error::Busy { .. }) => {
                    let m = e.to_string();
                    assert!(m.contains("vdb.redb"), "{m}");
                    assert!(m.contains("one process"), "{m}");
                }
                other => panic!("expected Busy, got {other:?}"),
            }
        }
        // The first handle is unharmed, and the file opens again once it
        // is dropped.
        assert_eq!(first.generation().unwrap(), 0);
        drop(first);
        assert!(RedbDb::open(t.db()).is_ok());
    }

    // --- every read against FilesDb ---------------------------------

    use crate::METADATA_FILE_FIELDS as FIELDS;

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
    fn rows_of(fdb: &FilesDb, k: &EntryKey, pending: bool) -> Rows {
        let metas = if pending {
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

    struct Fixture {
        _t: Tmp,
        fdb: FilesDb,
        rdb: RedbDb,
        keys: Vec<EntryKey>,
    }

    fn fixture() -> Fixture {
        let t = Tmp::new();
        let root = t.0.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let fdb = FilesDb::new(&root);
        let rdb = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        let b = EntryKey::new("dev-libs", "b-1");
        let c = EntryKey::new("app-misc", "c-1");
        let d = EntryKey::new("app-misc", "d-1");
        let z = EntryKey::new("sys-apps", "z-1");
        let e = EntryKey::new("sys-apps", "e-1");
        let p = EntryKey::new("dev-libs", "p-1");
        let owned: &[u8] = b"dir /usr\nobj /usr/bin/x abc 1\nsym /usr/bin/y -> x 1\nfoo /usr/z\n";
        // a: sealed (valid stamp); a non-UTF-8 field is dropped from the
        // snapshot, so aux_get serves "".
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
        // b: never sealed (stamp absent): lossy text; invalid SLOT -> "0".
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
        // c: sealed, then changed: stale stamp; CONTENTS is not UTF-8.
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
        // d: no files at all. z: a SLOT with a subslot.
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
        let mut preserved = BTreeMap::new();
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
            preserved.insert(
                "dev-libs/a:0".to_string(),
                PreservedLibsEntry {
                    cpv: "dev-libs/a-1".into(),
                    counter: "7".into(),
                    paths: vec!["/usr/lib/z.so.1".into(), "/usr/lib/a.so.1".into()],
                },
            );
            preserved.insert(
                "sys-apps/z:2".to_string(),
                PreservedLibsEntry {
                    cpv: "sys-apps/z-1".into(),
                    counter: "8".into(),
                    paths: vec!["/lib/q.so".into()],
                },
            );
            t.set_preserved_libs(&PreservedLibs {
                entries: preserved,
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
            (&a, MetadataStamp::Valid),
            (&b, MetadataStamp::Absent),
            (&c, MetadataStamp::Stale),
            (&d, MetadataStamp::Absent),
            (&z, MetadataStamp::Absent),
            (&e, MetadataStamp::Absent),
        ] {
            rdb.seed_entry(k, INSTALLED, stamp, &rows_of(&fdb, k, false));
        }
        rdb.seed_entry(&p, PENDING, MetadataStamp::Absent, &rows_of(&fdb, &p, true));
        assert!(fdb.read_file(&a, "metadata").unwrap().is_some());
        assert!(fdb.read_file(&e, "metadata").unwrap().is_none());
        rdb.seed_list(WORLD, &fdb.world().unwrap().atoms);
        rdb.seed_list(WORLD_SETS, &fdb.world_sets().unwrap().sets);
        for (k, e) in fdb.preserved_libs().unwrap().entries {
            rdb.seed_plib(&k, &e);
        }
        for (k, v) in fdb.config_memory().unwrap().entries {
            rdb.seed_config(&k, &v);
        }
        let hwm = fdb.counter().unwrap().unwrap().0;
        rdb.seed_meta("counter_hwm", &hwm.to_le_bytes());
        Fixture {
            _t: t,
            fdb,
            rdb,
            keys: vec![a, b, c, d, z, e, p, EntryKey::new("dev-libs", "zz-9")],
        }
    }

    #[test]
    fn every_read_matches_filesdb() {
        let f = fixture();
        crate::test_support::assert_same_reads(&f.fdb, &f.rdb, &f.keys);
        // The pending entry is invisible to live reads, visible to its own.
        let p = &f.keys[6];
        assert!(!f.rdb.has_entry(p).unwrap());
        assert_eq!(f.rdb.read_file(p, "SLOT").unwrap(), None);
        assert_eq!(f.rdb.aux_get(p, "SLOT").unwrap(), None);
        assert_eq!(f.rdb.pending_entries().unwrap(), vec![p.clone()]);
        assert_eq!(
            f.rdb.read_pending_file(p, "SLOT").unwrap().as_deref(),
            Some(&b"9\n"[..])
        );
        assert_eq!(f.rdb.read_pending_file(&f.keys[0], "SLOT").unwrap(), None);
        let a = &f.keys[0];
        assert_eq!(
            f.rdb.read_file(a, "EMPTY").unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            f.rdb.read_file(a, "NUL").unwrap().as_deref(),
            Some(&b"a\0b"[..])
        );
        assert_eq!(
            f.rdb.aux_get(a, "DESCRIPTION").unwrap().as_deref(),
            Some("caf\u{fffd} x")
        );
        let b = &f.keys[1];
        assert_eq!(
            f.rdb.aux_get(b, "DESCRIPTION").unwrap().as_deref(),
            Some("caf\u{fffd} x")
        );
        assert_eq!(f.rdb.aux_get(b, "SLOT").unwrap().as_deref(), Some("0"));
        assert_eq!(f.rdb.read_file(b, "BIN").unwrap().as_deref(), Some(BAD));
        assert_eq!(f.rdb.aux_get(&f.keys[7], "SLOT").unwrap(), None);
        let got = f
            .rdb
            .owners(&[b"/usr/bin/x".as_slice(), b"/usr".as_slice()])
            .unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(f.rdb.list_files(&f.keys[3]).unwrap(), Some(vec![]));
    }

    #[test]
    fn empty_database_matches_an_empty_filesdb() {
        let t = Tmp::new();
        let root = t.0.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let fdb = FilesDb::new(&root);
        let rdb = RedbDb::open(t.db()).unwrap();
        crate::test_support::assert_same_reads(&fdb, &rdb, &[EntryKey::new("a", "b-1")]);
        assert_eq!(rdb.generation().unwrap(), 0);
        assert!(rdb.snapshot().unwrap().entries.is_empty());
    }

    #[test]
    fn a_readonly_handle_reads_the_same() {
        let f = fixture();
        let path = f.rdb.path().to_path_buf();
        let Fixture { fdb, rdb, keys, _t } = f;
        drop(rdb);
        let ro = RedbDb::open_readonly(&path).unwrap();
        crate::test_support::assert_same_reads(&fdb, &ro, &keys);
    }

    #[test]
    fn snapshot_entry_image_and_reverse_dependents_follow_the_rows() {
        let f = fixture();
        f.rdb.seed_meta("generation", &5u64.to_le_bytes());
        let snap = f.rdb.snapshot().unwrap();
        assert_eq!(snap.generation, 5);
        assert_eq!(snap.entries.len(), 6, "live entries only");
        for (k, fields) in &snap.entries {
            for field in FIELDS {
                assert_eq!(
                    fields.get(field).map(str::to_string),
                    f.fdb.aux_get(k, field).unwrap(),
                    "snapshot {k} {field}"
                );
            }
        }
        assert!(snap.get(&f.keys[6]).is_none());

        let img = f.rdb.entry_image(&f.keys[2]).unwrap().unwrap();
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
            f.rdb
                .entry_image(&f.keys[0])
                .unwrap()
                .unwrap()
                .metadata_stamp,
            MetadataStamp::Valid
        );
        assert!(f.rdb.entry_image(&f.keys[6]).unwrap().is_none());
        assert!(f.rdb.entry_image(&f.keys[7]).unwrap().is_none());

        let recs = f
            .rdb
            .reverse_dependents("x/y", &[DepClass::Rdepend, DepClass::Depend])
            .unwrap();
        // Only the entries whose deps carry a token (here the non-atoms "x" and
        // "y", i.e. the unsure marker) come back (S8.1).
        assert_eq!(recs.len(), 3);
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

    // --- cache -------------------------------------------------------

    #[test]
    fn the_cache_follows_generation() {
        let f = fixture();
        let a = f.keys[0].clone();
        assert_eq!(f.rdb.aux_get(&a, "SLOT").unwrap().as_deref(), Some("0/1.2"));
        let before = f.rdb.snapshot().unwrap();
        // Same generation: the very same snapshot is served again.
        assert!(Arc::ptr_eq(&before, &f.rdb.snapshot().unwrap()));
        assert!(Arc::ptr_eq(
            &f.rdb.cached(false).unwrap().1,
            &f.rdb.cached(true).unwrap().1
        ));
        // A commit that changes the data and bumps the generation (the
        // contract of the S5.3 writer) is seen without any explicit
        // invalidation: write raw, bypassing `seed_entry`'s invalidate.
        let txn = rw(&f.rdb).begin_write().unwrap();
        {
            let mut t = txn.open_table(ENTRY).unwrap();
            let rec = EntryRec::decode(
                t.get((INSTALLED, "dev-libs", "a-1"))
                    .unwrap()
                    .unwrap()
                    .value(),
            )
            .unwrap();
            let mut gone = None;
            if rec.id > 0 {
                gone = t
                    .remove((INSTALLED, "dev-libs", "a-1"))
                    .unwrap()
                    .map(|_| ());
            }
            assert!(gone.is_some());
            txn.open_table(META)
                .unwrap()
                .insert("generation", 100u64.to_le_bytes().as_slice())
                .unwrap();
        }
        txn.commit().unwrap();
        assert_eq!(f.rdb.generation().unwrap(), 100);
        assert!(!f.rdb.has_entry(&a).unwrap());
        assert_eq!(f.rdb.aux_get(&a, "SLOT").unwrap(), None);
        assert!(!f.rdb.entries().unwrap().contains(&a));
        let after = f.rdb.snapshot().unwrap();
        assert_eq!(after.generation, 100);
        assert_eq!(after.entries.len(), 5);
        assert!(!Arc::ptr_eq(&before, &after));
    }

    // --- chunks ------------------------------------------------------

    fn big(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn read_file_at_crosses_chunk_boundaries() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let k = EntryKey::new("dev-libs", "a-1");
        let data = big(CHUNK * 3 + 1234);
        let exact = big(CHUNK * 2);
        db.seed_entry(
            &k,
            INSTALLED,
            MetadataStamp::Absent,
            &vec![
                ("BIG".into(), data.clone(), 420, 0),
                ("EXACT".into(), exact.clone(), 420, 0),
                ("SMALL".into(), b"0123456789".to_vec(), 420, 0),
                ("EMPTY".into(), vec![], 420, 0),
            ],
        );
        assert_eq!(db.read_file(&k, "BIG").unwrap().unwrap(), data);
        assert_eq!(db.read_file(&k, "EXACT").unwrap().unwrap(), exact);
        assert_eq!(
            db.file_meta(&k, "BIG").unwrap().unwrap().len,
            data.len() as u64
        );
        let c = CHUNK as u64;
        let cases: &[(u64, usize)] = &[
            (0, 10),
            (c - 5, 10),
            (c, 1),
            (c - 1, 2),
            (c - 1, CHUNK + 2),
            (2 * c + 7, 3 * CHUNK),
            (3 * c, 1234),
            (3 * c + 1200, 1000),
            (data.len() as u64 - 1, 5),
            (data.len() as u64, 5),
            (data.len() as u64 + 1000, 5),
            (5, 0),
            (0, usize::MAX),
            (u64::MAX, 4),
        ];
        for &(off, len) in cases {
            let start = usize::try_from(off).unwrap_or(usize::MAX).min(data.len());
            let end = start.saturating_add(len).min(data.len());
            assert_eq!(
                db.read_file_at(&k, "BIG", off, len).unwrap().as_deref(),
                Some(&data[start..end]),
                "BIG off {off} len {len}"
            );
        }
        assert_eq!(
            db.read_file_at(&k, "EXACT", c - 3, 6).unwrap().as_deref(),
            Some(&exact[CHUNK - 3..CHUNK + 3])
        );
        assert_eq!(
            db.read_file_at(&k, "EXACT", 2 * c, 4).unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            db.read_file_at(&k, "SMALL", 8, 10).unwrap().as_deref(),
            Some(&b"89"[..])
        );
        assert_eq!(
            db.read_file_at(&k, "EMPTY", 0, 8).unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(db.read_file_at(&k, "NOPE", 0, 4).unwrap(), None);
        assert_eq!(
            db.read_file_at(&EntryKey::new("dev-libs", "zz-1"), "BIG", 0, 4)
                .unwrap(),
            None
        );
    }

    #[test]
    fn read_file_at_loads_only_the_chunks_it_needs() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let k = EntryKey::new("dev-libs", "a-1");
        let data = big(CHUNK * 3 + 10);
        db.seed_entry(
            &k,
            INSTALLED,
            MetadataStamp::Absent,
            &vec![("BIG".into(), data.clone(), 420, 0)],
        );
        // Break chunks 1 and 2: reads inside chunks 0 and 3 still work, any
        // read that touches 1 or 2 (and the whole file) is Corrupt.
        let id = {
            let txn = db.handle.begin_read().unwrap();
            let rec = Rd::new(&txn).unwrap().rec(INSTALLED, &k).unwrap().unwrap();
            rec.id
        };
        let txn = rw(&db).begin_write().unwrap();
        {
            let mut ch = txn.open_table(ENTRY_FILE_CHUNK).unwrap();
            ch.remove((id, "BIG", 1)).unwrap();
            ch.remove((id, "BIG", 2)).unwrap();
        }
        txn.commit().unwrap();
        let c = CHUNK as u64;
        assert_eq!(
            db.read_file_at(&k, "BIG", 10, 20).unwrap().as_deref(),
            Some(&data[10..30])
        );
        assert_eq!(
            db.read_file_at(&k, "BIG", 3 * c + 2, 100)
                .unwrap()
                .as_deref(),
            Some(&data[3 * CHUNK + 2..])
        );
        assert!(matches!(
            db.read_file_at(&k, "BIG", c - 1, 2),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            db.read_file_at(&k, "BIG", 2 * c, 1),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(db.read_file(&k, "BIG"), Err(Error::Corrupt(_))));
    }

    // --- encodings ---------------------------------------------------

    #[test]
    fn records_round_trip_and_reject_garbage() {
        let rec = EntryRec {
            id: 42,
            counter: Some(-3),
            stamp: MetadataStamp::Stale,
            dir_mode: 0o40755,
            dir_mtime_ns: -5,
            slot: "0".into(),
            subslot: "1.2".into(),
            repo: "gentoo".into(),
        };
        assert_eq!(EntryRec::decode(&rec.encode()).unwrap(), rec);
        let none = EntryRec {
            counter: None,
            ..rec.clone()
        };
        assert_eq!(EntryRec::decode(&none.encode()).unwrap(), none);
        let mut short = rec.encode();
        short.pop();
        assert!(EntryRec::decode(&short).is_err());
        let mut long = rec.encode();
        long.push(0);
        assert!(EntryRec::decode(&long).is_err());
        let mut bad_stamp = rec.encode();
        bad_stamp[17] = 9;
        assert!(EntryRec::decode(&bad_stamp).is_err());
        let f = FileRec {
            len: 1 << 40,
            mode: 0o100644,
            mtime_ns: i64::MIN,
        };
        assert_eq!(FileRec::decode(&f.encode()).unwrap(), f);
        assert_eq!(f.encode().len(), 20);
        assert!(FileRec::decode(&[0; 19]).is_err());
        assert_eq!(
            (
                chunk_count(0),
                chunk_count(1),
                chunk_count(CHUNK as u64 + 1)
            ),
            (0, 1, 2)
        );
        assert_eq!(chunk_len(CHUNK as u64 + 5, 1), 5);
        assert_eq!(chunk_len(CHUNK as u64, 1), 0);
    }

    // --- the write side (S5.3), mirroring sqlite's S2.5 tests ---------

    use redb::{ReadableMultimapTable, ReadableTableMetadata};

    type OwnerRow = (
        Vec<u8>,
        u32,
        String,
        Option<String>,
        Option<i64>,
        Option<Vec<u8>>,
    );

    fn merge(db: &RedbDb, k: &EntryKey, files: &[(&str, &[u8])], seal: bool) {
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

    fn rec_of_key(db: &RedbDb, k: &EntryKey, state: u8) -> Option<EntryRec> {
        db.read(|rd| rd.rec(state, k)).unwrap()
    }

    fn id_of(db: &RedbDb, k: &EntryKey, state: u8) -> Option<u64> {
        rec_of_key(db, k, state).map(|r| r.id)
    }

    /// Row counts: `[entry, entry_by_id, entry_by_counter, file_meta,
    /// file_chunk, owner, dep_atom, needed, world, world_sets,
    /// preserved_lib, config_memory]`.
    fn counts(db: &RedbDb) -> [u64; 12] {
        let txn = db.handle.begin_read().unwrap();
        let t = |d| txn.open_table(d).unwrap().len().unwrap();
        [
            t(ENTRY),
            txn.open_table(ENTRY_BY_ID).unwrap().len().unwrap(),
            txn.open_table(ENTRY_BY_COUNTER).unwrap().len().unwrap(),
            txn.open_table(ENTRY_FILE_META).unwrap().len().unwrap(),
            txn.open_table(ENTRY_FILE_CHUNK).unwrap().len().unwrap(),
            txn.open_multimap_table(OWNER).unwrap().len().unwrap(),
            txn.open_multimap_table(DEP_ATOM).unwrap().len().unwrap(),
            txn.open_multimap_table(NEEDED).unwrap().len().unwrap(),
            txn.open_table(WORLD).unwrap().len().unwrap(),
            txn.open_table(WORLD_SETS).unwrap().len().unwrap(),
            txn.open_table(PRESERVED_LIB).unwrap().len().unwrap(),
            txn.open_table(CONFIG_MEMORY).unwrap().len().unwrap(),
        ]
    }

    fn owner_rows_of(db: &RedbDb, id: u64) -> Vec<OwnerRow> {
        let txn = db.handle.begin_read().unwrap();
        let o = txn.open_multimap_table(OWNER).unwrap();
        let mut out = Vec::new();
        for item in o.iter().unwrap() {
            let (k, vals) = item.unwrap();
            for v in vals {
                let v = v.unwrap();
                let mut d = Dec(v.value());
                if d.u64().unwrap() != id {
                    continue;
                }
                let seq = d.u32().unwrap();
                let kind = d.str().unwrap();
                let md5 = (d.u8().unwrap() == 1).then(|| d.str().unwrap());
                let mtime = (d.u8().unwrap() == 1).then(|| d.i64().unwrap());
                let target = (d.u8().unwrap() == 1).then(|| {
                    let n = d.u32().unwrap() as usize;
                    d.take(n).unwrap().to_vec()
                });
                assert!(d.done());
                out.push((k.value().to_vec(), seq, kind, md5, mtime, target));
            }
        }
        out.sort_by_key(|r| r.1);
        out
    }

    /// `(arch, obj, soname, rpath, needed)` in `seq` order.
    fn needed_rows_of(db: &RedbDb, id: u64) -> Vec<(String, Vec<u8>, String, String, String)> {
        let txn = db.handle.begin_read().unwrap();
        let n = txn.open_multimap_table(NEEDED).unwrap();
        let mut out = Vec::new();
        for v in n.get(id).unwrap() {
            let v = v.unwrap();
            let mut d = Dec(v.value());
            let seq = d.u32().unwrap();
            let arch = d.str().unwrap();
            let len = d.u32().unwrap() as usize;
            let obj = d.take(len).unwrap().to_vec();
            let row = (
                arch,
                obj,
                d.str().unwrap(),
                d.str().unwrap(),
                d.str().unwrap(),
            );
            assert!(d.done());
            out.push((seq, row));
        }
        out.sort_by_key(|r| r.0);
        out.into_iter().map(|r| r.1).collect()
    }

    /// `(class, cp, token)` sorted.
    fn dep_rows_of(db: &RedbDb, id: u64) -> Vec<(String, String, String)> {
        let txn = db.handle.begin_read().unwrap();
        let t = txn.open_multimap_table(DEP_ATOM).unwrap();
        let mut out = Vec::new();
        for item in t.iter().unwrap() {
            let (k, vals) = item.unwrap();
            for v in vals {
                let v = v.unwrap();
                let mut d = Dec(v.value());
                if d.u64().unwrap() != id {
                    continue;
                }
                let class = write::DEP_CLASSES[d.u8().unwrap() as usize].field();
                let tok = String::from_utf8(d.0.to_vec()).unwrap();
                out.push((class.to_string(), k.value().to_string(), tok));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn a_dropped_or_failed_transaction_changes_nothing() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        merge(&db, &a, &a_files(), true);
        let gen0 = db.generation().unwrap();
        let before = counts(&db);
        let hwm = db.counter().unwrap();
        let big_chunks = before[4];
        assert!(big_chunks > 0 && before[5] > 0 && before[6] > 0 && before[7] > 0);

        // Dropped without commit: every kind of write is undone.
        let b = EntryKey::new("dev-libs", "b-1");
        {
            let mut t = db.begin_write().unwrap();
            t.begin_entry(&b).unwrap();
            t.put_entry_file(&b, "CONTENTS", b"obj /x 0 0\n").unwrap();
            t.finish_entry(&b).unwrap();
            t.delete_entry(&a).unwrap();
            t.next_counter().unwrap();
            t.set_world(&World {
                atoms: vec!["a/b".into()],
            })
            .unwrap();
            t.set_world_sets(&WorldSets {
                sets: vec!["s".into()],
            })
            .unwrap();
            let mut cm = ConfigMemory::default();
            cm.entries.insert("/etc/x".into(), "abc".into());
            t.set_config_memory(&cm).unwrap();
            t.set_import_mark(3, "/x").unwrap();
        }
        assert_eq!(db.generation().unwrap(), gen0);
        assert_eq!(counts(&db), before);
        assert_eq!(db.counter().unwrap(), hwm);
        assert_eq!(db.import_mark().unwrap(), None);
        assert!(db.has_entry(&a).unwrap());
        assert!(!db.has_entry(&b).unwrap());

        // A transaction that fails half way and is then dropped.
        {
            let mut t = db.begin_write().unwrap();
            t.begin_entry(&b).unwrap();
            t.put_entry_file(&b, "SLOT", b"0\n").unwrap();
            let never = EntryKey::new("x", "never-1");
            assert!(matches!(t.finish_entry(&never), Err(Error::Invalid(_))));
            assert!(matches!(
                t.put_entry_file(&never, "f", b""),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(t.delete_entry(&b), Err(Error::Invalid(_))));
            assert!(matches!(
                t.replace_file(&b, "CONTENTS", b""),
                Err(Error::Invalid(_))
            ));
            assert!(matches!(t.discard_pending(&never), Err(Error::Invalid(_))));
        }
        assert_eq!(db.generation().unwrap(), gen0);
        assert_eq!(counts(&db), before);
        assert_eq!(db.read_pending_file(&b, "SLOT").unwrap(), None);

        // An open transaction does not block reads, which see the old state.
        let mut t = db.begin_write().unwrap();
        t.delete_entry(&a).unwrap();
        assert!(db.has_entry(&a).unwrap());
        assert_eq!(db.generation().unwrap(), gen0);
        t.commit().unwrap();
        assert!(!db.has_entry(&a).unwrap());
        assert_eq!(db.generation().unwrap(), gen0 + 1);
        let c = counts(&db);
        assert_eq!(
            [c[0], c[1], c[3], c[4], c[5], c[6], c[7]],
            [0; 7],
            "delete leaves no row behind"
        );
    }

    #[test]
    fn one_write_transaction_at_a_time_per_database() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let txn = db.begin_write().unwrap();
        // The same thread cannot take a second one: it would wait for itself.
        assert!(matches!(db.begin_write(), Err(Error::Invalid(m)) if m.contains("already holds")));
        // Another thread waits (redb blocks) until the first is dropped.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut second = db.begin_write().unwrap();
                second.set_counter(Counter(5)).unwrap();
                second.commit().unwrap();
                tx.send(()).unwrap();
            });
            assert!(
                rx.recv_timeout(std::time::Duration::from_millis(300))
                    .is_err(),
                "the second begin_write must wait while the first is open"
            );
            drop(txn);
            rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        });
        assert_eq!(db.counter().unwrap(), Some(Counter(5)));
        // Once the transaction is gone the same thread may begin again.
        db.begin_write().unwrap().commit().unwrap();
        let ro_dir = Tmp::new();
        drop(RedbDb::open(ro_dir.db()).unwrap());
        let ro = RedbDb::open_readonly(ro_dir.db()).unwrap();
        assert!(matches!(ro.begin_write(), Err(Error::Invalid(_))));
    }

    #[test]
    fn generation_bumps_by_one_per_committed_write_transaction() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let g = db.generation().unwrap();
        db.begin_write().unwrap().commit().unwrap();
        assert_eq!(db.generation().unwrap(), g, "an empty transaction");
        let mut tx = db.begin_write().unwrap();
        tx.set_preserved_libs(&PreservedLibs::default()).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.generation().unwrap(),
            g,
            "an unchanged preserved-libs write"
        );
        let mut tx = db.begin_write().unwrap();
        tx.set_world(&World::default()).unwrap();
        tx.set_world_sets(&WorldSets::default()).unwrap();
        tx.commit().unwrap();
        assert_eq!(db.generation().unwrap(), g + 1, "D4-only counts, once");
        let k = EntryKey::new("a", "b-1");
        merge(&db, &k, &[("SLOT", b"0\n")], true);
        assert_eq!(db.generation().unwrap(), g + 2, "a whole merge in one txn");
        // The read cache follows the bumps.
        assert_eq!(db.entries().unwrap(), vec![k.clone()]);
        let mut tx = db.begin_write().unwrap();
        tx.delete_entry(&k).unwrap();
        tx.commit().unwrap();
        assert!(db.entries().unwrap().is_empty());
        assert_eq!(db.aux_get(&k, "SLOT").unwrap(), None);
    }

    #[test]
    fn finish_entry_fills_columns_and_derived_multimaps() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        merge(&db, &a, &a_files(), true);
        let id = id_of(&db, &a, INSTALLED).unwrap();
        let rec = rec_of_key(&db, &a, INSTALLED).unwrap();
        assert_eq!(
            (rec.slot.as_str(), rec.subslot.as_str(), rec.repo.as_str()),
            ("2", "3.4", "gentoo")
        );
        assert_eq!(rec.counter, Some(41));
        assert_eq!(rec.stamp, MetadataStamp::Valid);
        assert_eq!(
            db.counter().unwrap(),
            Some(Counter(41)),
            "hwm follows COUNTER"
        );
        let by_counter = db
            .handle
            .begin_read()
            .unwrap()
            .open_table(ENTRY_BY_COUNTER)
            .unwrap()
            .get(41)
            .unwrap()
            .map(|g| g.value());
        assert_eq!(by_counter, Some(id));

        let b = |s: &str| s.as_bytes().to_vec();
        assert_eq!(
            owner_rows_of(&db, id),
            vec![
                (b("/usr"), 0, "dir".into(), None, None, None),
                (
                    b("/usr/bin/x"),
                    1,
                    "obj".into(),
                    Some("d41d8cd98f00b204e9800998ecf8427e".into()),
                    Some(1700),
                    None
                ),
                (
                    b("/usr/bin/y"),
                    2,
                    "sym".into(),
                    None,
                    Some(1701),
                    Some(b("x"))
                ),
                (b("/dev/n"), 3, "dev".into(), None, None, None),
            ],
            "`foo` is not a recorded kind"
        );
        assert_eq!(
            needed_rows_of(&db, id),
            vec![
                (
                    "X86_64".into(),
                    b("/usr/lib/liba.so"),
                    "liba.so.1".into(),
                    "/opt/a:/usr/lib".into(),
                    "libc.so.6,libb.so".into()
                ),
                (
                    "X86_64".into(),
                    b("/usr/bin/x"),
                    String::new(),
                    String::new(),
                    "liba.so.1".into()
                ),
            ],
            "the short line is skipped, the no-rpath sentinel is empty"
        );
        let s = |c: &str, cp: &str, atom: &str| (c.to_string(), cp.to_string(), atom.to_string());
        assert_eq!(
            dep_rows_of(&db, id),
            vec![
                s("DEPEND", "dev-libs/ssl", "dev-libs/ssl"),
                s("RDEPEND", "a/b", "a/b"),
                s("RDEPEND", "c/d", "c/d"),
                s("RDEPEND", "dev-libs/old", "!<dev-libs/old-2"),
                s("RDEPEND", "dev-libs/ssl", ">=dev-libs/ssl-1.1:0=[static]"),
            ]
        );
        // The stored metadata file is the sealed one.
        let img = db.entry_image(&a).unwrap().unwrap();
        assert_eq!(img.metadata_stamp, MetadataStamp::Valid);
        let text = String::from_utf8(db.read_file(&a, "metadata").unwrap().unwrap()).unwrap();
        assert!(
            text.starts_with("#format=1\nBDEPEND=\nCOUNTER=41\n"),
            "{text}"
        );
        assert!(text.ends_with(&format!("#dir_mtime={}\n", img.dir_mtime_ns)));
        assert_eq!(db.aux_get(&a, "SLOT").unwrap().as_deref(), Some("2/3.4"));

        // A lower COUNTER does not lower the hwm; a higher one raises it.
        let c = EntryKey::new("dev-libs", "c-1");
        merge(&db, &c, &[("COUNTER", b"7"), ("SLOT", b"bad slot")], false);
        assert_eq!(db.counter().unwrap(), Some(Counter(41)));
        assert_eq!(rec_of_key(&db, &c, INSTALLED).unwrap().slot, "0");
        let d = EntryKey::new("dev-libs", "d-1");
        merge(&db, &d, &[("COUNTER", b"99\n")], false);
        assert_eq!(db.counter().unwrap(), Some(Counter(99)));
        let rec = rec_of_key(&db, &d, INSTALLED).unwrap();
        assert_eq!((rec.counter, rec.slot.as_str()), (Some(99), ""));
        // A duplicate COUNTER: the first claimant keeps entry_by_counter.
        let e = EntryKey::new("dev-libs", "e-1");
        merge(&db, &e, &[("COUNTER", b"99\n")], false);
        let claimant = db
            .handle
            .begin_read()
            .unwrap()
            .open_table(ENTRY_BY_COUNTER)
            .unwrap()
            .get(99)
            .unwrap()
            .map(|g| g.value());
        assert_eq!(claimant, id_of(&db, &d, INSTALLED));
    }

    #[test]
    fn a_pending_entry_has_no_derived_rows_and_a_late_put_stales_the_stamp() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let k = EntryKey::new("dev-libs", "a-1");
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&k).unwrap();
        tx.put_entry_file(&k, "CONTENTS", CONTENTS_A).unwrap();
        tx.put_entry_file(&k, "SLOT", b"0\n").unwrap();
        tx.seal_entry(&k).unwrap();
        tx.commit().unwrap();
        let c = counts(&db);
        assert_eq!((c[2], c[5], c[6], c[7]), (0, 0, 0, 0));
        let stamp = |db: &RedbDb| rec_of_key(db, &k, PENDING).unwrap().stamp;
        assert_eq!(stamp(&db), MetadataStamp::Valid);
        let mut tx = db.begin_write().unwrap();
        tx.put_entry_file(&k, "SLOT", b"1\n").unwrap();
        tx.commit().unwrap();
        assert_eq!(
            stamp(&db),
            MetadataStamp::Valid,
            "overwriting a name keeps the stamp"
        );
        let mut tx = db.begin_write().unwrap();
        tx.put_entry_file(&k, "extra", b"x").unwrap();
        tx.commit().unwrap();
        assert_eq!(stamp(&db), MetadataStamp::Stale, "a new name stales it");
        // A pending file is readable, a live entry does not exist yet.
        assert_eq!(db.read_pending_file(&k, "SLOT").unwrap().unwrap(), b"1\n");
        assert!(!db.has_entry(&k).unwrap());
    }

    #[test]
    fn replacing_in_a_slot_and_deleting_remove_the_derived_rows() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let old = EntryKey::new("dev-libs", "a-1");
        let new = EntryKey::new("dev-libs", "a-2");
        merge(&db, &old, &a_files(), true);
        let old_id = id_of(&db, &old, INSTALLED).unwrap();
        let before = counts(&db);
        let (owners, needed, deps) = (before[5], before[7], before[6]);
        assert!(owners > 0 && needed > 0 && deps > 0);

        // The merge's two-step shape: the pending row beside the live one,
        // then finish_entry_replacing in one transaction.
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&new).unwrap();
        tx.put_entry_file(&new, "SLOT", b"2/5\n").unwrap();
        tx.put_entry_file(&new, "CONTENTS", b"obj /usr/bin/n aaa 5\n")
            .unwrap();
        tx.put_entry_file(&new, "RDEPEND", b"dev-libs/ssl\n")
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(id_of(&db, &old, INSTALLED), Some(old_id));
        assert_eq!(counts(&db)[5], owners, "pending adds no owner rows");
        let mut tx = db.begin_write().unwrap();
        tx.finish_entry_replacing(&new, std::slice::from_ref(&old))
            .unwrap();
        tx.commit().unwrap();
        assert!(id_of(&db, &old, INSTALLED).is_none());
        let new_id = id_of(&db, &new, INSTALLED).unwrap();
        assert_eq!(owner_rows_of(&db, new_id).len(), 1);
        let c = counts(&db);
        assert_eq!(
            (c[0], c[5], c[7], c[6]),
            (1, 1, 0, 1),
            "no pending row is left"
        );
        assert_eq!(c[2], 0, "the old COUNTER claim went with the entry");
        assert_eq!(rec_of_key(&db, &new, INSTALLED).unwrap().subslot, "5");
        assert_eq!(
            db.counter().unwrap(),
            Some(Counter(41)),
            "hwm never goes down"
        );

        // The same pf: the live row is replaced as a whole by finish.
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&new).unwrap();
        tx.put_entry_file(&new, "CONTENTS", b"dir /opt\ndir /opt/a\n")
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(counts(&db)[0], 2, "pending beside live");
        let mut tx = db.begin_write().unwrap();
        tx.finish_entry(&new).unwrap();
        tx.commit().unwrap();
        let c = counts(&db);
        assert_eq!((c[0], c[1], c[5], c[6]), (1, 1, 2, 0));
        let rec = rec_of_key(&db, &new, INSTALLED).unwrap();
        assert_eq!((rec.counter, rec.slot.as_str()), (None, ""));
        assert_eq!(db.read_file(&new, "SLOT").unwrap(), None);

        // Delete removes everything.
        let mut tx = db.begin_write().unwrap();
        tx.delete_entry(&new).unwrap();
        tx.commit().unwrap();
        let c = counts(&db);
        assert_eq!([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]], [0; 8]);
    }

    #[test]
    fn replace_file_refreshes_the_rows_that_derive_from_the_file() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        // A file of three chunks, so replacing it must drop the chunks.
        let mut files = a_files();
        let big = vec![b'x'; CHUNK * 2 + 10];
        files.push(("environment.bz2", &big));
        merge(&db, &a, &files, true);
        let id = id_of(&db, &a, INSTALLED).unwrap();
        let before = db.entry_image(&a).unwrap().unwrap();
        let chunks0 = counts(&db)[4];
        let g0 = db.generation().unwrap();

        let mut tx = db.begin_write().unwrap();
        tx.replace_file(&a, "CONTENTS", b"obj /usr/bin/x ffff 9\n")
            .unwrap();
        tx.replace_file(&a, "NEEDED.ELF.2", b"X86_64;/usr/bin/x;;;libz.so.1\n")
            .unwrap();
        tx.replace_file(&a, "environment.bz2", b"\xff\x00").unwrap();
        tx.commit().unwrap();

        assert_eq!(db.generation().unwrap(), g0 + 1);
        let rows = owner_rows_of(&db, id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3.as_deref(), Some("ffff"));
        assert_eq!(counts(&db)[5], 1, "the old owner rows are gone");
        let n = needed_rows_of(&db, id);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].4, "libz.so.1");
        assert_eq!(dep_rows_of(&db, id).len(), 5, "untouched");
        assert_eq!(
            counts(&db)[4],
            chunks0 - 3 + 1,
            "chunks of the old file dropped"
        );
        assert_eq!(
            db.read_file(&a, "environment.bz2").unwrap().as_deref(),
            Some(&b"\xff\x00"[..])
        );
        // A new file name is created; stamp, directory mtime, modes and the
        // other files stay.
        let mut tx = db.begin_write().unwrap();
        tx.replace_file(&a, "brand-new", b"n").unwrap();
        tx.commit().unwrap();
        let after = db.entry_image(&a).unwrap().unwrap();
        assert_eq!(after.metadata_stamp, before.metadata_stamp);
        assert_eq!(after.dir_mtime_ns, before.dir_mtime_ns);
        assert_eq!(
            db.file_meta(&a, "CONTENTS").unwrap().unwrap().mode,
            0o100_644
        );
        let meta = |i: &EntryImage| {
            i.files
                .iter()
                .find(|f| f.meta.name == "metadata")
                .unwrap()
                .data
                .clone()
        };
        assert_eq!(meta(&after), meta(&before));
        // The dependency fields, SLOT and COUNTER refresh theirs too.
        let mut tx = db.begin_write().unwrap();
        tx.replace_file(&a, "RDEPEND", b"x/y\n").unwrap();
        tx.replace_file(&a, "SLOT", b"7\n").unwrap();
        tx.replace_file(&a, "COUNTER", b"50\n").unwrap();
        tx.commit().unwrap();
        assert_eq!(dep_rows_of(&db, id).len(), 2);
        let rec = rec_of_key(&db, &a, INSTALLED).unwrap();
        assert_eq!((rec.slot.as_str(), rec.counter), ("7", Some(50)));
        assert_eq!(counts(&db)[2], 1, "one claim, moved from 41 to 50");
        assert_eq!(
            db.counter().unwrap(),
            Some(Counter(41)),
            "replace_file leaves hwm"
        );
        // No live entry, no replace.
        let mut tx = db.begin_write().unwrap();
        assert!(matches!(
            tx.replace_file(&EntryKey::new("x", "y-1"), "f", b""),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn the_counter_is_atomic_across_transactions_and_reopen() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        // A tick that is dropped does not move the counter.
        let mut tx = db.begin_write().unwrap();
        assert_eq!(tx.next_counter().unwrap(), Counter(0));
        assert_eq!(tx.next_counter().unwrap(), Counter(1));
        drop(tx);
        assert_eq!(db.counter().unwrap(), None);
        let mut tx = db.begin_write().unwrap();
        assert_eq!(tx.next_counter().unwrap(), Counter(0));
        tx.commit().unwrap();
        let mut tx = db.begin_write().unwrap();
        assert_eq!(tx.next_counter().unwrap(), Counter(1));
        tx.commit().unwrap();
        // It survives a close and a reopen, and the tick continues.
        drop(db);
        let db = RedbDb::open(t.db()).unwrap();
        assert_eq!(db.counter().unwrap(), Some(Counter(1)));
        let mut tx = db.begin_write().unwrap();
        assert_eq!(tx.next_counter().unwrap(), Counter(2));
        tx.commit().unwrap();
        // set_counter is a plain store; the tick continues from it.
        let mut tx = db.begin_write().unwrap();
        tx.set_counter(Counter(100)).unwrap();
        assert_eq!(tx.next_counter().unwrap(), Counter(101));
        tx.commit().unwrap();
        drop(db);
        let ro = RedbDb::open_readonly(t.db()).unwrap();
        assert_eq!(ro.counter().unwrap(), Some(Counter(101)));
    }

    #[test]
    fn insert_entry_keeps_stamps_as_given_and_fills_derived_rows() {
        let t = Tmp::new();
        let src = RedbDb::open(t.db()).unwrap();
        let a = EntryKey::new("dev-libs", "a-1");
        merge(&src, &a, &a_files(), true);
        let valid = src.entry_image(&a).unwrap().unwrap();

        let image = |stamp, with_metadata: bool| {
            let mut i = valid.clone();
            i.metadata_stamp = stamp;
            i.dir_mtime_ns = 77;
            i.dir_mode = 0o750;
            for f in &mut i.files {
                f.meta.mtime_ns = 12_345;
                f.meta.mode = 0o100_600;
            }
            if !with_metadata {
                i.files.retain(|f| f.meta.name != "metadata");
            }
            i
        };
        for (tag, stamp, with_metadata) in [
            ("stale", MetadataStamp::Stale, true),
            ("absent", MetadataStamp::Absent, false),
            ("valid", MetadataStamp::Valid, true),
        ] {
            let d = Tmp::new();
            let dst = RedbDb::open(d.db()).unwrap();
            let img = image(stamp, with_metadata);
            let mut tx = dst.begin_write().unwrap();
            tx.insert_entry(&img).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                dst.entry_image(&a).unwrap().unwrap(),
                img,
                "{tag}: as given"
            );
            assert_eq!(dst.generation().unwrap(), 1);
            assert_eq!(
                dst.counter().unwrap(),
                None,
                "{tag}: counter store untouched"
            );
            let id = id_of(&dst, &a, INSTALLED).unwrap();
            assert_eq!(owner_rows_of(&dst, id).len(), 4, "{tag}");
            assert_eq!(needed_rows_of(&dst, id).len(), 2, "{tag}");
            assert_eq!(dep_rows_of(&dst, id).len(), 5, "{tag}");
            assert_eq!(rec_of_key(&dst, &a, INSTALLED).unwrap().counter, Some(41));
            // Stale stays stale through a second hop; no file is added.
            let again = Tmp::new();
            let dst2 = RedbDb::open(again.db()).unwrap();
            let mut tx = dst2.begin_write().unwrap();
            tx.insert_entry(&dst.entry_image(&a).unwrap().unwrap())
                .unwrap();
            tx.commit().unwrap();
            let hop = dst2.entry_image(&a).unwrap().unwrap();
            assert_eq!(hop.metadata_stamp, stamp, "{tag}");
            assert_eq!(
                hop.files.iter().any(|f| f.meta.name == "metadata"),
                with_metadata,
                "{tag}"
            );
        }
        // A stale stamp is not served: aux_get reads the field file.
        let d = Tmp::new();
        let dst = RedbDb::open(d.db()).unwrap();
        let mut img = image(MetadataStamp::Stale, true);
        for f in &mut img.files {
            if f.meta.name == "SLOT" {
                f.data = b"9\n".to_vec();
            }
        }
        let mut tx = dst.begin_write().unwrap();
        tx.insert_entry(&img).unwrap();
        tx.commit().unwrap();
        assert_eq!(dst.aux_get(&a, "SLOT").unwrap().as_deref(), Some("9"));
        // Replacing a live entry through insert_entry leaves no old rows.
        let mut tx = dst.begin_write().unwrap();
        tx.insert_entry(&image(MetadataStamp::Valid, true)).unwrap();
        tx.commit().unwrap();
        let c = counts(&dst);
        assert_eq!((c[0], c[2], c[5], c[6], c[7]), (1, 1, 4, 5, 2));
        // Out-of-range values are refused before anything is written.
        let mut bad = img.clone();
        bad.dir_mtime_ns = i128::MAX;
        let mut tx = dst.begin_write().unwrap();
        assert!(matches!(tx.insert_entry(&bad), Err(Error::Invalid(_))));
    }

    #[test]
    fn discard_pending_removes_only_the_pending_entry() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let k = EntryKey::new("dev-libs", "a-1");
        merge(&db, &k, &a_files(), true);
        let live = counts(&db);
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&k).unwrap();
        tx.put_entry_file(&k, "SLOT", b"9\n").unwrap();
        tx.put_entry_file(&k, "big", &vec![1; CHUNK + 1]).unwrap();
        tx.commit().unwrap();
        assert_eq!(db.pending_entries().unwrap(), vec![k.clone()]);
        let mut tx = db.begin_write().unwrap();
        tx.discard_pending(&k).unwrap();
        assert!(matches!(tx.discard_pending(&k), Err(Error::Invalid(_))));
        tx.commit().unwrap();
        assert!(db.pending_entries().unwrap().is_empty());
        assert_eq!(
            counts(&db),
            live,
            "the live entry and its rows are untouched"
        );
        assert_eq!(db.aux_get(&k, "SLOT").unwrap().as_deref(), Some("2/3.4"));
        // An orphan with no live entry beside it.
        let o = EntryKey::new("dev-libs", "orphan-1");
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&o).unwrap();
        tx.commit().unwrap();
        let mut tx = db.begin_write().unwrap();
        tx.discard_pending(&o).unwrap();
        tx.commit().unwrap();
        assert_eq!(counts(&db), live);
    }

    #[test]
    fn import_mark_round_trips_and_survives_reopen() {
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        assert_eq!(db.import_mark().unwrap(), None);
        let mut tx = db.begin_write().unwrap();
        tx.set_import_mark(42, "/var/db/pkg").unwrap();
        tx.commit().unwrap();
        assert_eq!(db.import_mark().unwrap(), Some((42, "/var/db/pkg".into())));
        let mut tx = db.begin_write().unwrap();
        tx.set_import_mark(43, "/other").unwrap();
        tx.commit().unwrap();
        drop(db);
        let db = RedbDb::open(t.db()).unwrap();
        assert_eq!(db.import_mark().unwrap(), Some((43, "/other".into())));
    }

    #[test]
    fn copy_entry_file_reads_the_source_and_its_mode() {
        use std::os::unix::fs::PermissionsExt as _;
        let t = Tmp::new();
        let db = RedbDb::open(t.db()).unwrap();
        let src = t.0.join("src.bin");
        std::fs::write(&src, b"hello").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o600)).unwrap();
        let k = EntryKey::new("a", "b-1");
        let mut tx = db.begin_write().unwrap();
        tx.begin_entry(&k).unwrap();
        tx.copy_entry_file(&k, "f", &src).unwrap();
        let err = tx
            .copy_entry_file(&k, "g", &t.0.join("missing"))
            .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err:?}");
        tx.finish_entry(&k).unwrap();
        tx.commit().unwrap();
        assert_eq!(db.read_file(&k, "f").unwrap().unwrap(), b"hello");
        assert_eq!(db.file_meta(&k, "f").unwrap().unwrap().mode & 0o777, 0o600);
    }
}
