//! The installed-package database ("VDB") behind one interface.
//!
//! - [`InstalledDb`] is the read side plus [`InstalledDb::begin_write`];
//!   [`WriteTxn`] is the write side.
//! - [`register`] / [`for_root`] is the process-wide registry
//!   (`root → Arc<dyn InstalledDb>`). An unregistered root gets a cached
//!   [`FilesDb`], so `emerge` (which never registers) stays on the
//!   historic `var/db/pkg` layout.
//! - [`FilesDb`] is that layout; the database backends (`SqliteDb`,
//!   `RedbDb`, behind the `vdb-sqlite` / `vdb-redb` features) implement
//!   every method.
//!
//! This crate sits **below** `portage-repo` and depends only on
//! `portage-util`: atom parsing, `USE` reduction, version splitting and
//! the package-move remapping stay above it.
//!
//! Every deviation from the design (`LLM/feat-157-authoritative-vdb-database.md`,
//! `docs/vdb_to_db.md`), the per-backend read/write semantics, and the
//! step history live in `LLM/code-notes/portage-vdb/lib.md`.
//!
//! File contents, `CONTENTS` paths and [`EntryImage`] data are bytes; entry
//! keys, file names, `aux_get` values, world atoms, preserved-libs records
//! and config memory are `String` (decoded, lossy for `aux_get`).

pub mod contents;
mod convert;
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod dep_cp;
mod error;
mod files;
mod files_write;
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod loaded;
#[cfg(feature = "vdb-redb")]
mod redb_db;
mod registry;
mod scratch;
#[cfg(feature = "vdb-sqlite")]
mod sqlite;
#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
mod test_support;
mod types;

pub use contents::{ContentsLine, parse_line, paths};
pub use convert::{CopyReport, VerifyReport, copy_all, verify};
pub use error::{Error, Result};
pub use files::FilesDb;
pub use files_write::{format_preserved_libs, parse_preserved_libs};
#[cfg(feature = "vdb-redb")]
pub use redb_db::{RedbDb, SCHEMA_VERSION as REDB_SCHEMA_VERSION};
pub use registry::{for_root, register, reset};
pub use scratch::{absorb_file, materialize_entry, materialize_files};
#[cfg(feature = "vdb-sqlite")]
pub use sqlite::{SCHEMA_VERSION, SqliteDb};
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Returns the rusqlite version string (proves the feature links).
#[cfg(feature = "vdb-sqlite")]
pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

/// Creates a redb database at `path` and writes one key (proves the feature links).
#[cfg(feature = "vdb-redb")]
pub fn redb_smoke(path: &std::path::Path) -> Result<()> {
    let db = redb::Database::create(path).map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    let write_txn = db
        .begin_write()
        .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    {
        let table_def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("test");
        let mut table = write_txn
            .open_table(table_def)
            .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
        table
            .insert(b"key" as &[u8], b"value" as &[u8])
            .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    }
    write_txn
        .commit()
        .map_err(|e| Error::io(path, std::io::Error::other(e)))?;
    Ok(())
}

/// The 23 single-line fields of the consolidated `metadata` file, sorted.
/// `CONTENTS` and `NEEDED*` are line-oriented and excluded. The set is part
/// of the format: change it only with [`METADATA_FILE_FORMAT_VERSION`].
// Portage: lib/portage/vartree.py, _METADATA_FILE_FIELDS
pub const METADATA_FILE_FIELDS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "COUNTER",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "REQUIRES",
    "RESTRICT",
    "SLOT",
    "USE",
    "repository",
];

// [`EntryFields`] stores exactly this many values.
const _: () = assert!(METADATA_FILE_FIELDS.len() == 23);

/// Portage's `_METADATA_FILE_FORMAT_VERSION`.
pub const METADATA_FILE_FORMAT_VERSION: u32 = 1;

/// Whether `name` is one of the [`METADATA_FILE_FIELDS`].
pub fn in_metadata_file(name: &str) -> bool {
    METADATA_FILE_FIELDS.contains(&name)
}

/// The read side of one installed-package database: one instance per root.
/// Entries in the `merging` state are never returned by the listing and
/// per-entry methods; only [`InstalledDb::read_pending_file`] sees them.
/// Listing order is the backend's stable order.
pub trait InstalledDb: Send + Sync {
    /// Which backend this is.
    fn kind(&self) -> BackendKind;

    /// `files` only: the VDB directory (`<root>/var/db/pkg`, not
    /// canonicalised). `None` on the database backends.
    fn vdb_dir(&self) -> Option<PathBuf>;

    /// `files` only: the directory of a live entry, for code that must
    /// hand a path to bash or a copier. No I/O; the entry need not exist.
    fn entry_path(&self, key: &EntryKey) -> Option<PathBuf> {
        self.vdb_dir()
            .map(|vdb| vdb.join(&key.category).join(&key.pf))
    }

    /// Cache key for "anything in the database changed"; compare for
    /// equality only. `0` when the VDB is missing.
    fn generation(&self) -> Result<u64>;

    /// Cheaper cache key for one category, `0` when missing. Database
    /// backends may return [`InstalledDb::generation`].
    fn category_generation(&self, category: &str) -> Result<u64>;

    /// Every live entry, in listing order.
    fn entries(&self) -> Result<Vec<EntryKey>>;

    /// Every category that exists, in listing order. The caller lists each
    /// with [`InstalledDb::category_entries`].
    fn categories(&self) -> Result<Vec<String>>;

    /// The `pf` of every live entry in `category`, in listing order;
    /// empty when the category does not exist.
    fn category_entries(&self, category: &str) -> Result<Vec<String>>;

    /// Whether `key` is a live entry.
    fn has_entry(&self, key: &EntryKey) -> Result<bool>;

    /// Entries that are mid-merge, by the key they will be published
    /// under, sorted. Not live: absent from every other read. Converters
    /// report them and never copy them.
    fn pending_entries(&self) -> Result<Vec<EntryKey>> {
        Ok(Vec::new())
    }

    /// One of the [`METADATA_FILE_FIELDS`] of a live entry: whitespace
    /// collapsed, invalid UTF-8 lossy, invalid `SLOT` served as `"0"`,
    /// missing field as `""`. `Ok(None)` without the entry; a foreign key
    /// is [`Error::Invalid`].
    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>>;

    /// Every live entry with all 23 normalised fields. Expensive on `files`.
    fn snapshot(&self) -> Result<Arc<Snapshot>>;

    /// The files of a live entry, `None` when the entry does not exist.
    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>>;

    /// Size, mode and mtime of one file of a live entry, `None` when the
    /// entry or the file does not exist.
    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>>;

    /// The exact bytes of one file of a live entry, `None` when the entry
    /// or the file does not exist.
    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// Up to `len` bytes of one file from offset `off` (FUSE). Short at
    /// end of file; `None` when the entry or the file does not exist.
    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>>;

    /// One file of the **pending** entry for `key`, `None` when there is
    /// no pending entry or no such file. Callers wanting "live, then
    /// pending" call [`InstalledDb::read_file`] first.
    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// One named file of every live entry, in listing order; `None`
    /// where the file is missing.
    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>>;

    /// A whole live entry for a converter: every file with its bytes,
    /// mode and mtime, the directory's mode and mtime, and the
    /// `metadata` stamp state. `None` when the entry does not exist.
    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>>;

    /// [`InstalledDb::entry_image`] without the file bytes. `None` when
    /// the entry does not exist. The default derives it from the image;
    /// every backend overrides it with a read that skips the bytes.
    fn entry_stat(&self, key: &EntryKey) -> Result<Option<EntryStat>> {
        Ok(self.entry_image(key)?.map(EntryStat::from))
    }

    /// Candidate reverse dependents of `cp`: one [`DepRecord`] per entry,
    /// in that order. A superset; the caller reduces and matches.
    fn reverse_dependents(&self, cp: &str, classes: &[DepClass]) -> Result<Vec<DepRecord>>;

    /// File owners: every `(path, entry)` pair where a live entry's
    /// `CONTENTS` records one of `paths`. Listing then `CONTENTS` order.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>>;

    /// The `world` file's atoms. Missing store: empty.
    fn world(&self) -> Result<World>;

    /// The `world_sets` file's set names, without the `@`. Missing store:
    /// empty.
    fn world_sets(&self) -> Result<WorldSets>;

    /// The preserved-libs registry, with `loaded == entries`. Missing or
    /// unparsable store: empty.
    fn preserved_libs(&self) -> Result<PreservedLibs>;

    /// The config-protect memory. Missing store: empty.
    fn config_memory(&self) -> Result<ConfigMemory>;

    /// The counter store, `None` when missing or unparsable. Not ticked;
    /// [`WriteTxn::next_counter`] ticks.
    fn counter(&self) -> Result<Option<Counter>>;

    /// Start a write transaction. Database backends take the write lock
    /// here (sqlite `BEGIN IMMEDIATE`).
    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>>;

    /// Whether a merge deletes the same-slot instances it replaces in the
    /// publishing transaction ([`WriteTxn::finish_entry_replacing`]).
    /// `files`: `false`; database backends: `true`, so the old instance
    /// stays installed until the one commit that publishes the new one.
    fn replace_in_publish(&self) -> bool {
        self.kind() != BackendKind::Files
    }

    /// The import mark: source generation and path at conversion time.
    /// `None` when never converted, or on a `files` backend.
    fn import_mark(&self) -> Result<Option<(u64, String)>> {
        Ok(None)
    }
}

/// One write transaction: `files` applies every call at once and `commit`
/// does nothing; the database backends apply everything at `commit` and
/// dropping without committing rolls back. Callers must not rely on
/// rollback. Any subset of calls is legal.
pub trait WriteTxn {
    /// Create the empty pending entry for `key`. Readers do not see it.
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Write one file of the pending entry for `key`.
    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Copy the regular file `src` into the pending entry for `key` as
    /// `name` (the mode comes with it).
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()>;

    /// Tick the counter store and return the new value: the larger of the
    /// `counter` file and every entry's `COUNTER`, plus one, written back
    /// atomically. The caller writes the entry's `COUNTER` file itself.
    fn next_counter(&mut self) -> Result<Counter>;

    /// Write the consolidated `metadata` file of the pending entry, stamp
    /// appended last. No-op without any [`METADATA_FILE_FIELDS`]; no file
    /// may be added afterwards.
    // Portage: lib/portage/vartree.py, _write_metadata_file
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Publish the pending entry for `key` (remove a live same-`pf` entry,
    /// then rename into place).
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Delete the live entries `replaced`, then publish the pending entry
    /// for `key`, in this transaction (delete before publish). On a
    /// database backend the whole call commits at once. The merge passes an
    /// empty `replaced` on `files` (see [`InstalledDb::replace_in_publish`]).
    fn finish_entry_replacing(&mut self, key: &EntryKey, replaced: &[EntryKey]) -> Result<()> {
        for old in replaced {
            self.delete_entry(old)?;
        }
        self.finish_entry(key)
    }

    /// Delete the pending entry for `key`: the sweep of an orphan a
    /// crashed merge left. A live entry of the same key is never touched;
    /// [`Error::Invalid`] when `key` is not pending.
    fn discard_pending(&mut self, key: &EntryKey) -> Result<()>;

    /// Insert a whole live entry from an image (converters, one call).
    /// Counters are kept as they are; the `metadata` stamp goes last.
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()>;

    /// Delete a live entry (`files`: remove the directory, then the
    /// category directory if now empty, best effort).
    fn delete_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Rewrite one file of a live entry (`files`: in place, so the entry
    /// mtime and the `metadata` stamp stay valid).
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Replace the `world` store, atoms one per line; an empty list writes
    /// an empty file.
    fn set_world(&mut self, world: &World) -> Result<()>;

    /// Replace the `world_sets` store, same rules; each name as `@<name>`.
    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()>;

    /// Replace the preserved-libs registry; writes nothing when
    /// `libs.entries == libs.loaded`.
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()>;

    /// Replace the config-protect memory, unconditionally.
    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()>;

    /// Set the counter store (converters; a merge uses
    /// [`WriteTxn::next_counter`]).
    fn set_counter(&mut self, counter: Counter) -> Result<()>;

    /// Record the import mark when converting from a `files` backend.
    /// Default: no-op.
    fn set_import_mark(&mut self, _generation: u64, _source: &str) -> Result<()> {
        Ok(())
    }

    /// Throw away the derived index and recompute it from the stored files
    /// of every live entry (`portuale vdb rebuild-index`). `files` has no
    /// derived index: [`Error::Unsupported`].
    fn rebuild_index(&mut self) -> Result<IndexCounts> {
        Err(Error::Unsupported(
            "rebuild_index: this backend has no derived index (S8.3)".into(),
        ))
    }

    /// Make the transaction durable. A no-op on `files`.
    fn commit(self: Box<Self>) -> Result<()>;
}

#[cfg(all(test, feature = "vdb-sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_version() {
        let version = sqlite_version();
        assert!(!version.is_empty());
        assert!(version.chars().any(|c| c.is_ascii_digit()));
    }
}

#[cfg(all(test, feature = "vdb-redb"))]
mod redb_tests {
    use super::*;

    #[test]
    fn test_redb_smoke() {
        let temp_dir = std::env::temp_dir().join("redb-smoke-test");
        let db_path = temp_dir.join("test.db");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        let result = redb_smoke(&db_path);
        assert!(result.is_ok(), "redb_smoke failed: {:?}", result);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&temp_dir);
    }
}
