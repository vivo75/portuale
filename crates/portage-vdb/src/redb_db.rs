//! The redb backend: one file, typed tables, one write transaction per
//! commit.
//!
//! The file tables are the truth; `owner`, `dep_atom` and `needed` are
//! derived from them and are filled by the writer, like on sqlite.
//! `meta.schema_version` ([`SCHEMA_VERSION`]) is independent of
//! `METADATA_FILE_FORMAT_VERSION` and of the sqlite schema version.
//!
//! Design: `docs/vdb_to_db.md` §4, §7.2.
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
//! and from sqlite's model:
//!
//! - `entry` is keyed by `(state, category, pf)`: the interface has no
//!   `pn`/`ver` split, and a pending entry is its own row beside a live
//!   row of the same key. State first, so "every live entry in order" and
//!   "every pending entry" are single range scans.
//! - `entry_by_counter` is not unique in sqlite's sense: a corpus can hold
//!   duplicate counters, so the writer keeps the first claimant; no read
//!   uses it.
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
//! own commits; the writer bumps `generation` on every effective commit
//! (the policy of sqlite), so nothing else needs to invalidate the cache. A read-only handle never sees a commit (nobody can hold the file
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

/// Arguments of [`Rd::chunks`].
struct ChunksArgs<'a> {
    id: u64,
    name: &'a str,
    len: u64,
    first: u32,
    last: u32,
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
    fn chunks(&self, args: ChunksArgs<'_>) -> R<Vec<u8>> {
        let ChunksArgs {
            id,
            name,
            len,
            first,
            last,
        } = args;
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
        self.chunks(ChunksArgs {
            id,
            name,
            len: rec.len,
            first: 0,
            last,
        })
        .map(Some)
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
        let data = self.chunks(ChunksArgs {
            id,
            name,
            len: rec.len,
            first,
            last,
        })?;
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

    /// `meta.generation`: +1 on every effective commit. One point
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
    /// normalised like `aux_get`. A superset of the true dependents; an
    /// entry none of whose tokens is, or might be, `cp` is not read.
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

    /// From the `owner` multimap: an indexed lookup of each distinct
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

    type Rows = Vec<(String, Vec<u8>, u32, i64)>;

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

    // --- chunks ------------------------------------------------------

    fn big(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
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

    // --- the write side, mirroring sqlite's tests ---------

    use redb::{ReadableMultimapTable, ReadableTableMetadata};

    type OwnerRow = (
        Vec<u8>,
        u32,
        String,
        Option<String>,
        Option<i64>,
        Option<Vec<u8>>,
    );

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
}
