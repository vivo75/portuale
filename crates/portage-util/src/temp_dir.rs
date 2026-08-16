//! A scratch directory that removes itself, plus a process-level stale
//! sweep: [`TempDir`] closes the unwind/success paths with `Drop`, and the
//! *next* process reclaims what a dead one left behind (nothing can run on
//! the way out of an abort/kill).
//!
//! Naming: `$TMPDIR/portuale-td-{tag}-{pid}-{nanos}`. The prefix is the
//! sweep's match key; the pid/nanos tail avoids collisions and tells a
//! live run's directories from a dead one's.

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A scratch directory under `$TMPDIR` that is removed when the guard
/// drops -- including when the enclosing test panics and unwinds.
///
/// Derefs to [`Path`].
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Relinquish cleanup and hand the path back.
    pub fn keep(self) -> PathBuf {
        let path = self.path.clone();
        std::mem::forget(self);
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl std::fmt::Display for TempDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path.display().fmt(f)
    }
}

impl std::fmt::Debug for TempDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path.display().fmt(f)
    }
}
