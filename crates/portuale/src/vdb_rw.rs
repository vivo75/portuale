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

fn now_ns() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}
