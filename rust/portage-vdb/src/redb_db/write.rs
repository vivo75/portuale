//! `RedbDb`'s write side (S5.3): [`RedbTxn`] and the derived multimaps.
//!
//! See the crate module doc, "Write transactions", for the model; this
//! file follows `sqlite/write.rs` method by method.
//!
//! One [`RedbTxn`] is one `redb::WriteTransaction`, opened by
//! `RedbDb::begin_write` and committed by [`WriteTxn::commit`]; dropping it
//! without a commit aborts it (redb's own drop). Every method opens the
//! tables it needs, uses them and lets them go before the next call, so no
//! table is ever open twice.
//!
//! # Derived multimap rows
//!
//! They exist only for live (`installed`) entries and are rebuilt from the
//! stored files, so removing them re-derives the rows from the stored file
//! first (the invariant "rows = f(files)").
//!
//! - `owner`: key = the path bytes as written in `CONTENTS`; value =
//!   `id u64`, `seq u32`, `kind` (u32-length string), `md5` (flag `u8`,
//!   string), `mtime` (flag `u8`, `i64`), `target` (flag `u8`, u32-length
//!   bytes). Same line rule as sqlite's `owner` table; `id` first, so the
//!   rows of one entry for one path sort together.
//! - `dep_atom`: key = `cp`; value = `id u64`, `class u8` (index in
//!   `DEP_CLASSES`: DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND), then the
//!   atom token as the remaining bytes. One value per distinct `(class,
//!   cp, token)`, from the shared `dep_cp`; a token it cannot classify is
//!   stored under the key `""` (S8.1, `reverse_dependents` always reads it).
//! - `needed`: key = `id u64`; value = `seq u32`, `arch`, `obj` (bytes),
//!   `soname`, `rpath`, `needed` (u32-length strings; the no-rpath
//!   sentinel is stored as `""`). Removal is `remove_all(id)`.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::thread::ThreadId;
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{ReadableTable, ReadableTableMetadata, WriteTransaction};

use super::{
    CHUNK, CONFIG_MEMORY, DEP_ATOM, ENTRY, ENTRY_BY_COUNTER, ENTRY_BY_ID, ENTRY_FILE_CHUNK,
    ENTRY_FILE_META, Enc, EntryRec, FileRec, Handle, INSTALLED, META, NEEDED, OWNER, PENDING,
    PRESERVED_LIB, RedbDb, WORLD, WORLD_SETS, chunk_count, corrupt, meta_u64, rerr,
};
use crate::dep_cp::dep_index_key;
use crate::files::{normalise_aux_bytes, translate_aux_slot};
use crate::{
    ConfigMemory, Counter, DepClass, EntryImage, EntryKey, Error, IndexCounts,
    METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, Result,
    World, WorldSets, WriteTxn,
};

/// `S_IFREG | 0644`: what `files` shows for a file written with the
/// default umask.
const FILE_MODE: u32 = 0o100_644;

/// `S_IFDIR | 0755` is stored as the permission bits only, like sqlite's
/// column default (493 = 0o755).
const DIR_MODE: u32 = 0o755;

pub(super) const DEP_CLASSES: [DepClass; 5] = [
    DepClass::Depend,
    DepClass::Rdepend,
    DepClass::Bdepend,
    DepClass::Pdepend,
    DepClass::Idepend,
];

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

// ------------------------------------------------------------- errors

/// Either a redb failure (mapped with the database path at the end) or one
/// of this crate's errors; lets every helper use `?` on both.
enum TxErr {
    Redb(redb::Error),
    Crate(Error),
}

type Tx<T> = std::result::Result<T, TxErr>;

macro_rules! from_redb {
    ($($t:ty),*) => {$(
        impl From<$t> for TxErr {
            fn from(e: $t) -> Self {
                TxErr::Redb(e.into())
            }
        }
    )*};
}
from_redb!(
    redb::Error,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::TransactionError
);

impl From<Error> for TxErr {
    fn from(e: Error) -> Self {
        TxErr::Crate(e)
    }
}

impl TxErr {
    fn into_error(self, path: &Path) -> Error {
        match self {
            TxErr::Redb(e) => rerr(path, e),
            TxErr::Crate(e) => e,
        }
    }
}

fn invalid<T>(msg: String) -> Tx<T> {
    Err(TxErr::Crate(Error::Invalid(msg)))
}

// -------------------------------------------------------- the transaction

/// One write transaction.
pub(super) struct RedbTxn<'a> {
    db: &'a RedbDb,
    txn: Option<WriteTransaction>,
    /// Something was written: `commit` bumps `meta.generation`.
    dirty: bool,
    thread: ThreadId,
}

fn writer_lock(db: &RedbDb) -> Result<std::sync::MutexGuard<'_, Option<ThreadId>>> {
    db.writer
        .lock()
        .map_err(|_| Error::Backend(format!("{}: writer mutex poisoned", db.path.display())))
}

impl<'a> RedbTxn<'a> {
    pub(super) fn begin(db: &'a RedbDb) -> Result<Self> {
        let Handle::Rw(d) = &db.handle else {
            return Err(Error::Invalid(format!(
                "{}: opened read-only",
                db.path.display()
            )));
        };
        let me = std::thread::current().id();
        if *writer_lock(db)? == Some(me) {
            return Err(Error::Invalid(format!(
                "{}: this thread already holds the open write transaction \
                 (redb allows one at a time; a second begin_write would wait for itself)",
                db.path.display()
            )));
        }
        // Blocks while another thread's transaction is open.
        let txn = d.begin_write().map_err(|e| rerr(&db.path, e.into()))?;
        *writer_lock(db)? = Some(me);
        Ok(RedbTxn {
            db,
            txn: Some(txn),
            dirty: false,
            thread: me,
        })
    }

    /// Run `f` on the transaction; `dirty` marks it written when `f`
    /// succeeds.
    fn op<T>(&mut self, dirty: bool, f: impl FnOnce(&WriteTransaction) -> Tx<T>) -> Result<T> {
        let txn = self
            .txn
            .as_ref()
            .ok_or_else(|| Error::Backend("transaction already finished".into()))?;
        let v = f(txn).map_err(|e| e.into_error(&self.db.path))?;
        self.dirty |= dirty;
        Ok(v)
    }
}

impl Drop for RedbTxn<'_> {
    fn drop(&mut self) {
        // Abort (if not committed) before the slot is released.
        self.txn = None;
        if let Ok(mut w) = self.db.writer.lock()
            && *w == Some(self.thread)
        {
            *w = None;
        }
    }
}

impl WriteTxn for RedbTxn<'_> {
    /// Drops a stale pending entry of `key` and creates a fresh one; a live
    /// entry of the same key is left alone.
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()> {
        self.op(true, |t| {
            delete_pending(t, key)?;
            let rec = EntryRec {
                id: new_id(t)?,
                counter: None,
                stamp: MetadataStamp::Absent,
                dir_mode: DIR_MODE,
                dir_mtime_ns: now_ns(),
                slot: String::new(),
                subslot: String::new(),
                repo: String::new(),
            };
            put_rec(t, PENDING, key, &rec)
        })
    }

    /// Deletes only the pending entry; [`Error::Invalid`] when there is
    /// none.
    fn discard_pending(&mut self, key: &EntryKey) -> Result<()> {
        self.op(true, |t| {
            if delete_pending(t, key)? {
                Ok(())
            } else {
                invalid(format!("no pending entry for {key}"))
            }
        })
    }

    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        self.op(true, |t| put_pending(t, key, name, data, FILE_MODE))
    }

    /// Reads `src` whole; its permission bits come with it (the mtime does
    /// not). The error names `src`.
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()> {
        self.op(true, |t| {
            pending(t, key)?;
            let data = std::fs::read(src).map_err(|e| Error::io(src, e))?;
            let mode = std::fs::metadata(src)
                .map_err(|e| Error::io(src, e))?
                .mode();
            put_pending(t, key, name, &data, mode)
        })
    }

    /// `meta.counter_hwm + 1`, stored in the same transaction: a dropped
    /// transaction does not consume a value.
    fn next_counter(&mut self) -> Result<Counter> {
        self.op(true, |t| {
            let next = get_hwm(t)?.max(-1) + 1;
            set_meta(t, "counter_hwm", &next.to_le_bytes())?;
            Ok(Counter(next))
        })
    }

    /// The `FilesTxn::seal_entry` bytes (see `SqliteTxn::seal_entry`); the
    /// stamp becomes `valid`. No-op when the entry holds no field file.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()> {
        let mut wrote = false;
        let r = self.op(false, |t| {
            let mut rec = pending(t, key)?;
            let mut data: Vec<(String, String)> = Vec::new();
            for &field in METADATA_FILE_FIELDS {
                let Some(raw) = read_file(t, rec.id, field)? else {
                    continue;
                };
                // Lossy, like real's consolidation (`errors="replace"`, #309).
                let text = String::from_utf8_lossy(&raw);
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
            put_file(t, rec.id, "metadata", body.as_bytes(), FILE_MODE, now)?;
            rec.stamp = MetadataStamp::Valid;
            rec.dir_mtime_ns = now;
            put_rec(t, PENDING, key, &rec)?;
            wrote = true;
            Ok(())
        });
        self.dirty |= wrote;
        r
    }

    /// Deletes the live entry of the same key, flips the pending one to
    /// live (same id) and derives the columns and the three multimaps,
    /// raising `counter_hwm` to the entry's `COUNTER` when larger.
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()> {
        self.op(true, |t| {
            let rec = pending(t, key)?;
            delete_installed(t, key)?;
            {
                let mut e = t.open_table(ENTRY)?;
                e.remove((PENDING, key.category.as_str(), key.pf.as_str()))?;
                e.insert(
                    (INSTALLED, key.category.as_str(), key.pf.as_str()),
                    rec.encode().as_slice(),
                )?;
            }
            t.open_table(ENTRY_BY_ID)?
                .insert(rec.id, (INSTALLED, key.category.as_str(), key.pf.as_str()))?;
            if let Some(c) = index_all(t, key, rec.id)? {
                set_hwm_if_larger(t, c)?;
            }
            Ok(())
        })
    }

    /// The image as given (files, modes, mtimes, directory mode and mtime,
    /// stamp state); no `metadata` file is added and the counter store is
    /// not touched.
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()> {
        self.op(true, |t| {
            let int = |v: i128, what: &str| {
                i64::try_from(v)
                    .map_err(|_| Error::Invalid(format!("{}: {what} out of range", image.key)))
            };
            let dir_mtime = int(image.dir_mtime_ns, "dir_mtime_ns")?;
            let mut files = Vec::with_capacity(image.files.len());
            for f in &image.files {
                files.push((
                    f.meta.name.as_str(),
                    &f.data,
                    f.meta.mode,
                    int(f.meta.mtime_ns, "mtime_ns")?,
                ));
            }
            delete_installed(t, &image.key)?;
            let rec = EntryRec {
                id: new_id(t)?,
                counter: None,
                stamp: image.metadata_stamp,
                dir_mode: image.dir_mode,
                dir_mtime_ns: dir_mtime,
                slot: String::new(),
                subslot: String::new(),
                repo: String::new(),
            };
            put_rec(t, INSTALLED, &image.key, &rec)?;
            for (name, data, mode, mtime) in files {
                put_file(t, rec.id, name, data, mode, mtime)?;
            }
            index_all(t, &image.key, rec.id)?;
            Ok(())
        })
    }

    fn delete_entry(&mut self, key: &EntryKey) -> Result<()> {
        self.op(true, |t| {
            if delete_installed(t, key)? {
                Ok(())
            } else {
                invalid(format!("no such entry {key}"))
            }
        })
    }

    /// In place: the file keeps its mode, and the stamp state, the
    /// directory mtime and every other file are untouched; what derives
    /// from the file is refreshed.
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        self.op(true, |t| {
            let Some(rec) = rec_of(t, INSTALLED, key)? else {
                return invalid(format!("no such entry {key}"));
            };
            let mode = file_rec(t, rec.id, name)?.map_or(FILE_MODE, |r| r.mode);
            unindex_file(t, rec.id, name)?;
            put_file(t, rec.id, name, data, mode, now_ns())?;
            index_file(t, key, rec.id, name)
        })
    }

    fn set_world(&mut self, world: &World) -> Result<()> {
        self.op(true, |t| set_list(t, WORLD, &world.atoms))
    }

    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()> {
        self.op(true, |t| set_list(t, WORLD_SETS, &sets.sets))
    }

    /// Nothing at all when `entries == loaded`.
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()> {
        if libs.entries == libs.loaded {
            return Ok(());
        }
        self.op(true, |t| {
            let mut tbl = t.open_table(PRESERVED_LIB)?;
            tbl.retain(|_, _| false)?;
            for (reg_key, e) in &libs.entries {
                let mut enc = Enc::default()
                    .str(&e.cpv)
                    .str(&e.counter)
                    .u32(e.paths.len() as u32);
                for p in &e.paths {
                    enc = enc.str(p);
                }
                tbl.insert(reg_key.as_str(), enc.0.as_slice())?;
            }
            Ok(())
        })
    }

    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()> {
        self.op(true, |t| {
            let mut tbl = t.open_table(CONFIG_MEMORY)?;
            tbl.retain(|_, _| false)?;
            for (path, md5) in &memory.entries {
                tbl.insert(path.as_str(), md5.as_str())?;
            }
            Ok(())
        })
    }

    fn set_counter(&mut self, counter: Counter) -> Result<()> {
        self.op(true, |t| {
            set_meta(t, "counter_hwm", &counter.0.to_le_bytes())
        })
    }

    fn set_import_mark(&mut self, generation: u64, source: &str) -> Result<()> {
        self.op(true, |t| {
            set_meta(t, "imported_files_generation", &generation.to_le_bytes())?;
            set_meta(t, "imported_files_source", source.as_bytes())
        })
    }

    /// Drops the three multimaps (redb recreates them on open), then
    /// refills them from each `installed` entry's stored files. Bumps the
    /// generation: an index reader's answers may change.
    fn rebuild_index(&mut self) -> Result<IndexCounts> {
        self.op(true, |t| {
            t.delete_multimap_table(OWNER)?;
            t.delete_multimap_table(DEP_ATOM)?;
            t.delete_multimap_table(NEEDED)?;
            let mut live = Vec::new();
            {
                let e = t.open_table(ENTRY)?;
                for item in e.range((INSTALLED, "", "")..(INSTALLED + 1, "", ""))? {
                    let (k, v) = item?;
                    let (_, category, pf) = k.value();
                    live.push((EntryKey::new(category, pf), EntryRec::decode(v.value())?.id));
                }
            }
            for (key, id) in &live {
                for name in file_names(t, *id)? {
                    if is_indexed_name(&name) {
                        index_file(t, key, *id, &name)?;
                    }
                }
            }
            let count = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
            Ok(IndexCounts {
                entries: live.len(),
                owner: count(t.open_multimap_table(OWNER)?.len()?),
                dep_atom: count(t.open_multimap_table(DEP_ATOM)?.len()?),
                needed: count(t.open_multimap_table(NEEDED)?.len()?),
            })
        })
    }

    /// Bumps `meta.generation` when anything was written, then commits.
    /// On any failure the drop aborts.
    fn commit(mut self: Box<Self>) -> Result<()> {
        let dirty = self.dirty;
        self.op(false, |t| {
            if dirty {
                let g = meta_u64_of(t, "generation")?;
                set_meta(t, "generation", &(g + 1).to_le_bytes())?;
            }
            Ok(())
        })?;
        let txn = self
            .txn
            .take()
            .ok_or_else(|| Error::Backend("transaction already finished".into()))?;
        txn.commit().map_err(|e| rerr(&self.db.path, e.into()))
    }
}

// ------------------------------------------------------------- meta

fn get_meta(t: &WriteTransaction, key: &str) -> Tx<Option<Vec<u8>>> {
    let m = t.open_table(META)?;
    Ok(m.get(key)?.map(|g| g.value().to_vec()))
}

fn set_meta(t: &WriteTransaction, key: &str, v: &[u8]) -> Tx<()> {
    t.open_table(META)?.insert(key, v)?;
    Ok(())
}

fn meta_u64_of(t: &WriteTransaction, key: &str) -> Tx<u64> {
    match get_meta(t, key)? {
        Some(b) => Ok(meta_u64(&b, key)?),
        None => Err(corrupt(format!("meta.{key} missing")).into()),
    }
}

fn get_hwm(t: &WriteTransaction) -> Tx<i64> {
    Ok(meta_u64_of(t, "counter_hwm")? as i64)
}

fn set_hwm_if_larger(t: &WriteTransaction, counter: i64) -> Tx<()> {
    if get_hwm(t)? < counter {
        set_meta(t, "counter_hwm", &counter.to_le_bytes())?;
    }
    Ok(())
}

// ---------------------------------------------------------- entry rows

fn rec_of(t: &WriteTransaction, state: u8, key: &EntryKey) -> Tx<Option<EntryRec>> {
    let e = t.open_table(ENTRY)?;
    let r = e
        .get((state, key.category.as_str(), key.pf.as_str()))?
        .map(|g| EntryRec::decode(g.value()))
        .transpose()?;
    Ok(r)
}

fn put_rec(t: &WriteTransaction, state: u8, key: &EntryKey, rec: &EntryRec) -> Tx<()> {
    let k = (state, key.category.as_str(), key.pf.as_str());
    t.open_table(ENTRY)?.insert(k, rec.encode().as_slice())?;
    t.open_table(ENTRY_BY_ID)?.insert(rec.id, k)?;
    Ok(())
}

fn pending(t: &WriteTransaction, key: &EntryKey) -> Tx<EntryRec> {
    match rec_of(t, PENDING, key)? {
        Some(r) => Ok(r),
        None => invalid(format!("no pending entry for {key}")),
    }
}

/// The next entry id: the last key of `entry_by_id` plus one.
fn new_id(t: &WriteTransaction) -> Tx<u64> {
    let by_id = t.open_table(ENTRY_BY_ID)?;
    Ok(by_id.last()?.map_or(1, |(k, _)| k.value() + 1))
}

/// Remove the row `(state, key)` and its files; `false` when absent.
fn delete_row(t: &WriteTransaction, state: u8, key: &EntryKey, rec: &EntryRec) -> Tx<()> {
    delete_all_files(t, rec.id)?;
    t.open_table(ENTRY_BY_ID)?.remove(rec.id)?;
    t.open_table(ENTRY)?
        .remove((state, key.category.as_str(), key.pf.as_str()))?;
    Ok(())
}

/// A pending entry has no derived rows and no counter claim.
fn delete_pending(t: &WriteTransaction, key: &EntryKey) -> Tx<bool> {
    let Some(rec) = rec_of(t, PENDING, key)? else {
        return Ok(false);
    };
    delete_row(t, PENDING, key, &rec)?;
    Ok(true)
}

/// Delete the live entry of `key` with its derived rows and counter claim;
/// `false` when there is none.
fn delete_installed(t: &WriteTransaction, key: &EntryKey) -> Tx<bool> {
    let Some(rec) = rec_of(t, INSTALLED, key)? else {
        return Ok(false);
    };
    unindex_all(t, rec.id)?;
    if let Some(c) = rec.counter {
        let mut bc = t.open_table(ENTRY_BY_COUNTER)?;
        let mine = bc.get(c)?.is_some_and(|g| g.value() == rec.id);
        if mine {
            bc.remove(c)?;
        }
    }
    delete_row(t, INSTALLED, key, &rec)?;
    Ok(true)
}

// ---------------------------------------------------------------- files

fn file_rec(t: &WriteTransaction, id: u64, name: &str) -> Tx<Option<FileRec>> {
    let fm = t.open_table(ENTRY_FILE_META)?;
    let r = fm
        .get((id, name))?
        .map(|g| FileRec::decode(g.value()))
        .transpose()?;
    Ok(r)
}

fn read_file(t: &WriteTransaction, id: u64, name: &str) -> Tx<Option<Vec<u8>>> {
    let Some(rec) = file_rec(t, id, name)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(usize::try_from(rec.len).unwrap_or(0));
    if rec.len > 0 {
        let n = chunk_count(rec.len);
        let ch = t.open_table(ENTRY_FILE_CHUNK)?;
        for item in ch.range((id, name, 0u32)..(id, name, n))? {
            let (_, v) = item?;
            out.extend_from_slice(v.value());
        }
    }
    if out.len() as u64 != rec.len {
        return Err(corrupt(format!(
            "entry {id} file {name:?}: {} bytes stored, {} expected",
            out.len(),
            rec.len
        ))
        .into());
    }
    Ok(Some(out))
}

/// Store (or replace) one file: its record and its 64 KiB chunks.
fn put_file(
    t: &WriteTransaction,
    id: u64,
    name: &str,
    data: &[u8],
    mode: u32,
    mtime_ns: i64,
) -> Tx<()> {
    {
        let mut ch = t.open_table(ENTRY_FILE_CHUNK)?;
        ch.retain_in((id, name, 0u32)..=(id, name, u32::MAX), |_, _| false)?;
        for (i, c) in data.chunks(CHUNK).enumerate() {
            ch.insert((id, name, i as u32), c)?;
        }
    }
    let rec = FileRec {
        len: data.len() as u64,
        mode,
        mtime_ns,
    };
    t.open_table(ENTRY_FILE_META)?
        .insert((id, name), rec.encode().as_slice())?;
    Ok(())
}

fn delete_all_files(t: &WriteTransaction, id: u64) -> Tx<()> {
    t.open_table(ENTRY_FILE_META)?
        .retain_in((id, "")..(id + 1, ""), |_, _| false)?;
    t.open_table(ENTRY_FILE_CHUNK)?
        .retain_in((id, "", 0u32)..(id + 1, "", 0u32), |_, _| false)?;
    Ok(())
}

/// The names of the files of entry `id`.
fn file_names(t: &WriteTransaction, id: u64) -> Tx<Vec<String>> {
    let fm = t.open_table(ENTRY_FILE_META)?;
    let mut out = Vec::new();
    for item in fm.range((id, "")..(id + 1, ""))? {
        let (k, _) = item?;
        out.push(k.value().1.to_owned());
    }
    Ok(out)
}

/// Store one file of a pending entry. A file added to an entry whose stamp
/// is `valid` makes it `stale`, as a new name in the directory moves its
/// mtime on `files`; overwriting a name does not.
fn put_pending(t: &WriteTransaction, key: &EntryKey, name: &str, data: &[u8], mode: u32) -> Tx<()> {
    let mut rec = pending(t, key)?;
    let now = now_ns();
    let existed = file_rec(t, rec.id, name)?.is_some();
    put_file(t, rec.id, name, data, mode, now)?;
    if !existed {
        rec.dir_mtime_ns = now;
        if rec.stamp == MetadataStamp::Valid {
            rec.stamp = MetadataStamp::Stale;
        }
        put_rec(t, PENDING, key, &rec)?;
    }
    Ok(())
}

fn set_list(
    t: &WriteTransaction,
    def: redb::TableDefinition<u32, &str>,
    items: &[String],
) -> Tx<()> {
    let mut tbl = t.open_table(def)?;
    tbl.retain(|_, _| false)?;
    for (i, s) in items.iter().enumerate() {
        tbl.insert(i as u32, s.as_str())?;
    }
    Ok(())
}

// --------------------------------------------------------- derived data

/// The files whose content the derived rows depend on, besides `SLOT`,
/// `repository` and `COUNTER` (the columns).
fn is_indexed_name(name: &str) -> bool {
    matches!(name, "CONTENTS" | "NEEDED.ELF.2") || DEP_CLASSES.iter().any(|d| d.field() == name)
}

/// Remove the derived rows of every indexed file of entry `id`.
fn unindex_all(t: &WriteTransaction, id: u64) -> Tx<()> {
    for name in file_names(t, id)? {
        unindex_file(t, id, &name)?;
    }
    Ok(())
}

/// Fill the columns and every derived row of the live entry `key`;
/// returns its `COUNTER`.
fn index_all(t: &WriteTransaction, key: &EntryKey, id: u64) -> Tx<Option<i64>> {
    let counter = derive_columns(t, key, id)?;
    for name in file_names(t, id)? {
        if is_indexed_name(&name) {
            index_file(t, key, id, &name)?;
        }
    }
    Ok(counter)
}

/// Remove the rows derived from the stored file `name` of entry `id`
/// (call before the file changes).
fn unindex_file(t: &WriteTransaction, id: u64, name: &str) -> Tx<()> {
    if !is_indexed_name(name) {
        return Ok(());
    }
    let data = read_file(t, id, name)?;
    match name {
        "CONTENTS" => {
            let mut o = t.open_multimap_table(OWNER)?;
            for (path, row) in owner_rows(id, data.as_deref()) {
                o.remove(path.as_slice(), row.as_slice())?;
            }
        }
        "NEEDED.ELF.2" => {
            t.open_multimap_table(NEEDED)?.remove_all(id)?;
        }
        _ => {
            if let Some(class) = DEP_CLASSES.iter().position(|d| d.field() == name) {
                let mut d = t.open_multimap_table(DEP_ATOM)?;
                for (cp, row) in dep_rows(id, class, data.as_deref()) {
                    d.remove(cp.as_str(), row.as_slice())?;
                }
            }
        }
    }
    Ok(())
}

/// Refill what depends on the (new) stored file `name` of the live entry.
fn index_file(t: &WriteTransaction, key: &EntryKey, id: u64, name: &str) -> Tx<()> {
    match name {
        "SLOT" | "repository" | "COUNTER" => {
            derive_columns(t, key, id)?;
        }
        "CONTENTS" => {
            let data = read_file(t, id, name)?;
            let mut o = t.open_multimap_table(OWNER)?;
            for (path, row) in owner_rows(id, data.as_deref()) {
                o.insert(path.as_slice(), row.as_slice())?;
            }
        }
        "NEEDED.ELF.2" => {
            let data = read_file(t, id, name)?;
            let mut n = t.open_multimap_table(NEEDED)?;
            for row in needed_rows(data.as_deref()) {
                n.insert(id, row.as_slice())?;
            }
        }
        _ => {
            if let Some(class) = DEP_CLASSES.iter().position(|d| d.field() == name) {
                let data = read_file(t, id, name)?;
                let mut d = t.open_multimap_table(DEP_ATOM)?;
                for (cp, row) in dep_rows(id, class, data.as_deref()) {
                    d.insert(cp.as_str(), row.as_slice())?;
                }
            }
        }
    }
    Ok(())
}

/// Test seeding: fill the derived rows of the stored file `name`.
#[cfg(test)]
pub(super) fn index_file_for_test(t: &WriteTransaction, key: &EntryKey, id: u64, name: &str) {
    assert!(index_file(t, key, id, name).is_ok());
}

/// `slot`, `subslot`, `repo`, `counter` of the entry record from its
/// `SLOT`, `repository` and `COUNTER` files (empty string / `None` when
/// the file is missing or, for the counter, not a number), and the
/// `entry_by_counter` claim (the first claimant of a counter keeps it).
fn derive_columns(t: &WriteTransaction, key: &EntryKey, id: u64) -> Tx<Option<i64>> {
    let text = |name: &str| -> Tx<Option<String>> {
        Ok(read_file(t, id, name)?
            .map(|b| normalise_aux_bytes(&b))
            .filter(|s| !s.is_empty()))
    };
    let (slot, subslot) = match text("SLOT")?.map(|s| translate_aux_slot("SLOT", s)) {
        Some(s) => match s.split_once('/') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (s.clone(), s),
        },
        None => (String::new(), String::new()),
    };
    let repo = text("repository")?.unwrap_or_default();
    let counter = read_file(t, id, "COUNTER")?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.trim().parse::<i64>().ok());
    let Some(mut rec) = rec_of(t, INSTALLED, key)? else {
        return Err(corrupt(format!("live entry {key} vanished")).into());
    };
    {
        let mut bc = t.open_table(ENTRY_BY_COUNTER)?;
        if let Some(old) = rec.counter {
            let mine = bc.get(old)?.is_some_and(|g| g.value() == id);
            if mine {
                bc.remove(old)?;
            }
        }
        if let Some(c) = counter {
            let free = bc.get(c)?.is_none();
            if free {
                bc.insert(c, id)?;
            }
        }
    }
    rec.slot = slot;
    rec.subslot = subslot;
    rec.repo = repo;
    rec.counter = counter;
    put_rec(t, INSTALLED, key, &rec)?;
    Ok(counter)
}

/// `(path, value)` of the `owner` rows of `CONTENTS`, with the line rule of
/// `files::claim_paths` (and sqlite's `derive_owner`): whitespace-separated
/// words, the first a recorded kind (`obj`, `sym`, `dir`, `dev`, `fif`,
/// `bin`), the second the path. `seq` counts recorded lines. A `CONTENTS`
/// that is not UTF-8 gives no rows.
fn owner_rows(id: u64, contents: Option<&[u8]>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let Some(text) = contents.and_then(|b| std::str::from_utf8(b).ok()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seq = 0_u32;
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
        let mut e = Enc::default()
            .u64(id)
            .u32(seq)
            .str(kind)
            .opt_str(md5)
            .opt_i64(mtime);
        e = match target {
            Some(t) => {
                let mut e = e.u8(1);
                e = e.bytes(t.as_bytes());
                e
            }
            None => e.u8(0),
        };
        out.push((path.as_bytes().to_vec(), e.0));
        seq += 1;
    }
    out
}

/// Values of the `needed` rows of `NEEDED.ELF.2` (see sqlite's
/// `derive_needed`): a line with fewer than five `;` fields is skipped, the
/// `"  -  "` no-rpath sentinel is `""`.
fn needed_rows(raw: Option<&[u8]>) -> Vec<Vec<u8>> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let lossy = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut out = Vec::new();
    let mut seq = 0_u32;
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
        out.push(
            Enc::default()
                .u32(seq)
                .str(&lossy(f[0]))
                .bytes(f[1])
                .str(&lossy(f[2]))
                .str(&rpath)
                .str(&lossy(f[4]))
                .0,
        );
        seq += 1;
    }
    out
}

/// `(cp, value)` of the `dep_atom` rows of one `*DEPEND` field (`class` is
/// its index in `DEP_CLASSES`): one per distinct `(cp, token)`.
fn dep_rows(id: u64, class: usize, raw: Option<&[u8]>) -> Vec<(String, Vec<u8>)> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(raw);
    let rows: BTreeSet<(&str, &str)> = text
        .split_whitespace()
        .filter_map(|tok| dep_index_key(tok).map(|cp| (cp, tok)))
        .collect();
    rows.into_iter()
        .map(|(cp, tok)| {
            let mut v = Enc::default().u64(id).u8(class as u8).0;
            v.extend_from_slice(tok.as_bytes());
            (cp.to_owned(), v)
        })
        .collect()
}
