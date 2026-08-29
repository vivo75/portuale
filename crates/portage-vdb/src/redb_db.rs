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

use crate::files::owner_wanted;
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryFields, EntryFile, EntryImage,
    EntryKey, EntryStat, Error, FileMeta, InstalledDb, METADATA_FILE_FIELDS, MetadataStamp,
    PreservedLibs, PreservedLibsEntry, Result, Snapshot, World, WorldSets, WriteTxn,
    in_metadata_file,
};
use redb::{
    Database, MultimapTableDefinition, ReadOnlyDatabase, ReadOnlyTable, ReadTransaction,
    ReadableDatabase, ReadableTable, TableDefinition,
};

/// The `entry_file_meta` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileRec {
    len: u64,
    mode: u32,
    mtime_ns: i64,
}

// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::FilesDb;

    struct Tmp(PathBuf);

    // --- every read against FilesDb ---------------------------------

    use crate::METADATA_FILE_FIELDS as FIELDS;

    // --- the write side, mirroring sqlite's tests ---------

    use redb::{ReadableMultimapTable, ReadableTableMetadata};
}
