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

use portage_vdb::MetadataStamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
}
