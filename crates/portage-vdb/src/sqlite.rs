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
use crate::files::owner_wanted;
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryFields, EntryKey, Error,
    FileMeta, METADATA_FILE_FIELDS, MetadataStamp, PreservedLibs, PreservedLibsEntry, Result,
    Snapshot, World, WorldSets, in_metadata_file,
};

fn db_err(path: &Path, e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::NotADatabase => {
            Error::Corrupt(format!("{}: not a SQLite database", path.display()))
        }
        e => Error::Backend(format!("{}: {e}", path.display())),
    }
}
