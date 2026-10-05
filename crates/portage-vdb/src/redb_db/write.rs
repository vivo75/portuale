//! `RedbDb`'s write side: [`RedbTxn`] and the derived multimaps.
//!
//! See the crate module doc, "Write transactions", for the model; this
//! file follows `sqlite/write.rs` method by method.
//!
//! One [`RedbTxn`] is one `redb::WriteTransaction`, opened by
//! `RedbDb::begin_write` and committed by [`WriteTxn::commit`]; dropping it
//! without a commit aborts it. Every method opens the tables it needs,
//! uses them and lets them go before the next call, so no table is ever
//! open twice.
//!
//! # Derived multimap rows
//!
//! They exist only for live (`installed`) entries and are rebuilt from the
//! stored files.
//!
//! - `owner`: key = the path bytes as written in `CONTENTS`; value =
//!   `id u64`, `seq u32`, `kind`, `md5`, `mtime`, `target`. Same line rule
//!   as sqlite's `owner` table; `id` first, so one entry's rows for one
//!   path sort together.
//! - `dep_atom`: key = `cp`; value = `id u64`, `class u8` (index in
//!   `DEP_CLASSES`), then the atom token. One value per distinct `(class,
//!   cp, token)`, from the shared `dep_cp`; an unclassifiable token is
//!   stored under the key `""`, which `reverse_dependents` always reads.
//! - `needed`: key = `id u64`; value = `seq u32`, `arch`, `obj`, `soname`,
//!   `rpath`, `needed`. Removal is `remove_all(id)`.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::thread::ThreadId;
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{ReadableTable, ReadableTableMetadata, WriteTransaction};

use super::{
    CHUNK, CONFIG_MEMORY, DEP_ATOM, ENTRY, ENTRY_BY_COUNTER, ENTRY_BY_ID, ENTRY_FILE_CHUNK,
    ENTRY_FILE_META, Enc, EntryRec, FileRec, Handle, INSTALLED, META, NEEDED, OWNER, PENDING,
    PRESERVED_LIB, WORLD, WORLD_SETS, chunk_count, corrupt, meta_u64, rerr,
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

/// Arguments of [`put_file`].
struct PutFileArgs<'a> {
    t: &'a WriteTransaction,
    id: u64,
    name: &'a str,
    data: &'a [u8],
    mode: u32,
    mtime_ns: i64,
}

/// Store (or replace) one file: its record and its 64 KiB chunks.
fn put_file(args: PutFileArgs<'_>) -> Tx<()> {
    let PutFileArgs {
        t,
        id,
        name,
        data,
        mode,
        mtime_ns,
    } = args;
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
    put_file(PutFileArgs {
        t,
        id: rec.id,
        name,
        data,
        mode,
        mtime_ns: now,
    })?;
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
