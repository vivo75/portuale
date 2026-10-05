// The read-only filesystem view of an installed-package database.
//
// This is the pure half of `portuale vdb mount`. Nothing here knows about
// FUSE: the tree, the inode numbers, `lookup`/`getattr`/`readdir`/`open`/
// `read`, the one-generation-per-handle rule, the EROFS decisions, the
// mtimes and the generated `metadata` file are plain methods on [`View`], so
// the unit tests drive them without a mount (this host has no `/dev/fuse`).
// `vdb_fuse.rs` is the thin translation layer to the `fuser` callbacks.
//
// The tree is `/` (the VDB root) -> `CAT/` -> `PF/` -> files.
//
// Inodes. An interning table maps every path seen (`Node`) to a number
// (root = 1, then 2, 3, ... in order of first sight) and back. It lives as
// long as the mount, never reuses a number and survives generation changes,
// so a path keeps its inode across commits, even when the entry is deleted
// and published again.
//
// Attributes: entry directory mode and mtime are the stored ones; a
// category shows the latest of its entries, the root the latest category;
// other directories are 0755; files show the stored mode, size and mtime.
// FUSE carries nanoseconds, so a shown mtime is exact.
//
// The `metadata` file follows the stamp state stored with the entry
// (`MetadataStamp`):
//   - Valid  -> the stored bytes with every `#dir_mtime=` line rewritten to
//     the entry directory mtime this view shows. The size is that of
//     the rewritten bytes.
//   - Stale  -> served exactly as stored; it does not match and stays stale.
//   - Absent -> there is no `metadata` file.
//
// Generations. `InstalledDb::generation` is the cache key. Every operation
// that starts from a path (`lookup`, `getattr`, `opendir`, `open`) works on
// the current generation, so a lookup after a commit sees the new one.
// What a handle returns afterwards is pinned:
//   - `opendir` materialises the whole listing and the handle serves that
//     list, whatever commits follow;
//   - `open` of a file of up to `pin_max` bytes (default 64 KiB: every small
//     field file, the `metadata` file) reads the bytes once and the handle
//     serves those bytes;
//   - a bigger file is read with `read_file_at` (never a whole-blob load).
//     The database has no snapshot to pin, so the handle remembers the
//     file's size and mtime; once the generation has moved on, each read
//     first checks that they still match and answers `ESTALE` if the file
//     changed or went away, rather than mixing two generations.
//     The check runs on every read (an in-place rewrite on `files` does
//     not move the generation). Residue, by the nature of a stat: a
//     rewrite with the same size *and* the same mtime is not noticed.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use portage_vdb::{EntryKey, EntryStat, FileMeta, InstalledDb, MetadataStamp};

/// The root directory's inode (FUSE's fixed `FUSE_ROOT_ID`).
pub const ROOT_INO: u64 = 1;

/// Files up to this many bytes are read once at `open` and pinned.
pub const DEFAULT_PIN_MAX: usize = 64 * 1024;

const METADATA: &str = "metadata";
const DIR_MTIME_PREFIX: &[u8] = b"#dir_mtime=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
}

/// What `getattr` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub ino: u64,
    pub kind: Kind,
    /// Permission bits only (`0o7777` mask).
    pub perm: u16,
    pub size: u64,
    pub nlink: u32,
    /// Nanoseconds since the epoch.
    pub mtime_ns: i128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEnt {
    pub ino: u64,
    pub kind: Kind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewError {
    /// No such path (also: the path vanished in a later generation).
    NoEnt,
    NotDir,
    IsDir,
    /// A handle that was never issued or was already released.
    BadHandle,
    /// Every write operation.
    Rofs,
    /// A pinned file changed or went away under an open handle.
    Stale,
    /// A write the read-write layer (`vdb_rw`) does not map.
    Perm,
    /// `rmdir` of a directory that still has children.
    NotEmpty,
    /// The name exists already (`mkdir`, `O_EXCL`, `link`).
    Exists,
    /// The backend failed.
    Io(String),
}

impl ViewError {
    /// The errno the adapter replies with.
    pub fn errno(&self) -> i32 {
        match self {
            ViewError::NoEnt => libc::ENOENT,
            ViewError::NotDir => libc::ENOTDIR,
            ViewError::IsDir => libc::EISDIR,
            ViewError::BadHandle => libc::EBADF,
            ViewError::Rofs => libc::EROFS,
            ViewError::Stale => libc::ESTALE,
            ViewError::Perm => libc::EPERM,
            ViewError::NotEmpty => libc::ENOTEMPTY,
            ViewError::Exists => libc::EEXIST,
            ViewError::Io(_) => libc::EIO,
        }
    }
}

impl From<portage_vdb::Error> for ViewError {
    fn from(e: portage_vdb::Error) -> Self {
        ViewError::Io(e.to_string())
    }
}

type Res<T> = Result<T, ViewError>;

/// The operations that would modify the tree. Each one is `EROFS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOp {
    Create,
    Mknod,
    Mkdir,
    Unlink,
    Rmdir,
    Rename,
    Symlink,
    Link,
    Setattr,
    Write,
    Setxattr,
    Removexattr,
    Fallocate,
    CopyFileRange,
}

/// The one decision for every write-class operation: refuse with `EROFS`.
pub fn deny(_op: WriteOp) -> ViewError {
    ViewError::Rofs
}

/// `open(2)` flags that ask for write access (`O_WRONLY`, `O_RDWR`,
/// `O_TRUNC`, `O_APPEND`, `O_CREAT`).
pub fn open_wants_write(flags: i32) -> bool {
    flags & libc::O_ACCMODE != libc::O_RDONLY
        || flags & (libc::O_TRUNC | libc::O_APPEND | libc::O_CREAT) != 0
}

/// Nanoseconds since the epoch as a `SystemTime` (negative: before it).
pub fn system_time(ns: i128) -> SystemTime {
    let secs = ns.div_euclid(1_000_000_000);
    let nanos = ns.rem_euclid(1_000_000_000) as u32;
    if secs >= 0 {
        UNIX_EPOCH + Duration::new(secs as u64, nanos)
    } else {
        // `secs` is negative and `nanos` counts forward from it.
        UNIX_EPOCH - Duration::new(secs.unsigned_abs() as u64, 0) + Duration::new(0, nanos)
    }
}

/// `stored` with every `#dir_mtime=` line's value replaced by `shown_ns`
/// (the line keeps its own line ending, or lack of one).
pub fn rewrite_dir_mtime(stored: &[u8], shown_ns: i128) -> Vec<u8> {
    let mut out = Vec::with_capacity(stored.len() + 8);
    for line in stored.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(DIR_MTIME_PREFIX) {
            out.extend_from_slice(DIR_MTIME_PREFIX);
            out.extend_from_slice(shown_ns.to_string().as_bytes());
            if line.ends_with(b"\n") {
                out.push(b'\n');
            }
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------- inodes

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Node {
    Root,
    Cat(String),
    Entry(EntryKey),
    File(EntryKey, String),
    /// An object of the read-write layer (`vdb_rw`): a volatile
    /// file, a staged directory or file. This view never serves it.
    Obj(u64),
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::File(..) | Node::Obj(_) => Kind::File,
            _ => Kind::Dir,
        }
    }
}

#[derive(Default)]
struct Inodes {
    by_node: HashMap<Node, u64>,
    /// `nodes[ino - 1]`.
    nodes: Vec<Node>,
}
