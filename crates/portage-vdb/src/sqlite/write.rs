//! `SqliteDb`'s write side: [`SqliteTxn`] and the derived tables.
//!
//! See the crate module doc, "Write transactions", for the model.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};

use super::{BUSY_TIMEOUT_MS, blob, db_err, entry_row, installed_row};
use crate::dep_cp::dep_index_key;
use crate::files::{normalise_aux_bytes, translate_aux_slot};
use crate::{
    ConfigMemory, Counter, DepClass, EntryImage, EntryKey, Error, IndexCounts,
    METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, Result,
    World, WorldSets, WriteTxn,
};

trait Be<T> {
    fn be(self, path: &Path) -> Result<T>;
}

impl<T> Be<T> for rusqlite::Result<T> {
    fn be(self, path: &Path) -> Result<T> {
        self.map_err(|e| db_err(path, e))
    }
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}
