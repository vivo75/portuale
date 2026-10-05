// The read-write layer of `portuale vdb mount --rw`: what Portage writes
// into `var/db/pkg` while it merges and unmerges, mapped onto database
// transactions.
//
// `RwView` wraps the read-only `vdb_view::View` and answers first from
// its own objects, then from the view:
//
// - volatile names (memory only, never stored): slot lock files and
//   categories created by `mkdir` that hold no entry yet;
// - staged `<cat>/-MERGING-<pf>/` directories on scratch disk, published
//   with one `insert_entry` transaction at the rename;
// - rewrites and removals inside live entries.
//
// Every object gets its inode from the view's one table (`Node::Obj`), so
// inodes stay unique and a rename keeps its inode. Like `vdb_view`, this
// file holds every decision and is unit tested without a mount;
// `vdb_fuse.rs` only translates types.

use std::collections::{BTreeMap, HashMap};
use std::os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use portage_vdb::{Counter, EntryFile, EntryImage, EntryKey, FileMeta, InstalledDb, MetadataStamp};

use crate::vdb_view::{DirEnt, Kind, ViewError};

/// Handles of this layer are numbered from here, above the view's own.
const FH_BASE: u64 = 1 << 48;

fn now_ns() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}

/// The names Portage's lock code creates next to entries: kept in memory
/// only.
// Portage: locks.py, lockfile
pub(crate) fn is_volatile_name(name: &str) -> bool {
    name.starts_with('.')
        && (name.ends_with(".portage_lockfile")
            || name.contains(".portage_lockfile.hardlock-")
            || name.ends_with("-inode-test"))
}

/// What `setattr` asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetAttr {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub mtime_ns: Option<i128>,
}

/// Owner attributes the adapter reports: the volatile files' own, or the
/// mounting user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
}

/// The prefix Portage gives an entry while it is being merged.
// Portage: const.py, MERGING_IDENTIFIER
const MERGING: &str = "-MERGING-";

enum Obj {
    /// A volatile file (lock files); `nlink` counts its names.
    Vol {
        data: Vec<u8>,
        mode: u32,
        owner: Owner,
        mtime_ns: i128,
        nlink: u32,
    },
    /// A staged `<cat>/-MERGING-<pf>` directory, backed by `path` in the
    /// scratch area. Its mtime follows filesystem rules: a create,
    /// unlink or rename inside it bumps it, a `utime` sets it.
    StagedDir {
        cat: String,
        path: PathBuf,
        mode: u32,
        mtime_ns: i128,
    },
    /// A file inside a staged directory, or a new name inside a live
    /// entry: the scratch file holds the bytes, mode and mtime.
    StagedFile { path: PathBuf },
}

fn io_err(e: std::io::Error) -> ViewError {
    match e.kind() {
        std::io::ErrorKind::NotFound => ViewError::NoEnt,
        std::io::ErrorKind::AlreadyExists => ViewError::Exists,
        _ => ViewError::Io(e.to_string()),
    }
}

fn mtime_of(md: &std::fs::Metadata) -> i128 {
    md.mtime() as i128 * 1_000_000_000 + md.mtime_nsec() as i128
}

// The tests replay scripts on a database backend; without one there is
// nothing to write to.
#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
#[path = "vdb_rw/ops.rs"]
mod ops;
