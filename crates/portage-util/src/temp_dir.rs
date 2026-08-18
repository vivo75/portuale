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

/// The prefix every [`TempDir`] directory carries, and the sweep's match key.
pub const TEMP_DIR_PREFIX: &str = "portuale-td-";

/// A scratch directory under `$TMPDIR` that is removed when the guard
/// drops -- including when the enclosing test panics and unwinds.
///
/// Derefs to [`Path`].
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create `{$TMPDIR}/portuale-td-{tag}-{pid}-{nanos}/`, sweeping any
    /// dead process's leftovers first (once per process).
    pub fn new(tag: &str) -> Self {
        Self::new_in(&std::env::temp_dir(), tag)
    }

    /// [`TempDir::new`] under an explicit parent.
    pub fn new_in(parent: &Path, tag: &str) -> Self {
        stale_sweep_once(parent);
        let path = parent.join(format!(
            "{TEMP_DIR_PREFIX}{tag}-{}-{}",
            std::process::id(),
            nanos()
        ));
        fs::create_dir_all(&path).expect("create test temp dir");
        Self { path }
    }

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

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Reclaim dead processes' `portuale-td-*` directories, at most once per
/// process (first [`TempDir::new`]). Recycled pids read as alive.
fn stale_sweep_once(parent: &Path) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| stale_sweep(parent));
}

fn stale_sweep(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(TEMP_DIR_PREFIX) else {
            continue;
        };
        let Some(pid) = parse_owner_pid(rest) else {
            continue;
        };
        if pid == std::process::id() || pid_is_alive(pid) {
            continue;
        }
        let _ = fs::remove_dir_all(entry.path());
    }
}

/// `{tag}-{pid}-{nanos}` -> the pid. The tag may itself contain `-`, so
/// take the last two `-`-separated components rather than the first.
fn parse_owner_pid(rest: &str) -> Option<u32> {
    let mut parts = rest.rsplitn(3, '-');
    let _nanos = parts.next()?;
    let pid = parts.next()?;
    // Require the third component to exist (the tag) so a stray
    // `portuale-td-x-12` without a nanos tail is not read as pid `x`.
    parts.next()?;
    pid.parse().ok()
}

/// Linux: `/proc/{pid}` exists iff some process owns that pid. Without
/// procfs this returns `true` and the sweep stays out of the way.
fn pid_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
