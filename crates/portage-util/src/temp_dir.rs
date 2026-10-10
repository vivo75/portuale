//! A scratch directory that removes itself, plus a process-level stale
//! sweep. Backlog #260.
//!
//! Every test scratch directory used to be `std::env::temp_dir().join(
//! format!("{tag}-{pid}-{nanos}"))` with a trailing `remove_dir_all` at
//! the end of the test -- which a panic skips, and which `panic =
//! "abort"` (the release profile this workspace's verification pass
//! runs under) skips even on a plain failed assert. A hung test killed
//! from outside skips it too. The result was ~85k top-level `/tmp`
//! entries exhausting the tmpfs inodes.
//!
//! [`TempDir`] closes the unwind/success paths with `Drop`. The stale
//! sweep closes the abort/kill paths: nothing can run on the way out of
//! those, so the *next* process reclaims what a dead one left behind --
//! the same division of labour `ebuild_phases.rs::bin_dir` and pmtest's
//! `_reclaim_portuale_bin_overlays` already use for the `portuale-bin.*`
//! overlay (backlog #88).
//!
//! Naming: `$TMPDIR/portuale-td-{tag}-{pid}-{nanos}`. The `portuale-td-`
//! prefix is what the sweep matches on, so the entries it owns are
//! recognizable from the outside (and a `find /tmp -name 'portuale-td-*'`
//! lists exactly ours). The pid/nanos tail keeps concurrent test threads
//! and repeated runs from colliding; the sweep parses the pid back out
//! to tell a live run's directories from a dead one's.

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The name prefix every [`TempDir`] directory carries. Also the sweep's
/// match key; see the module docs.
pub const TEMP_DIR_PREFIX: &str = "portuale-td-";

/// A scratch directory under `$TMPDIR` that is removed when the guard
/// drops -- including when the enclosing test panics and unwinds.
///
/// Derefs to [`Path`], so the existing `tmp.join(..)` / `&tmp` call
/// shapes keep working unchanged.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create `{$TMPDIR}/portuale-td-{tag}-{pid}-{nanos}/`, sweeping any
    /// dead process's leftovers first (once per process).
    pub fn new(tag: &str) -> Self {
        Self::new_in(&std::env::temp_dir(), tag)
    }

    /// [`TempDir::new`] under an explicit parent (a test that wants its
    /// scratch on real disk, or nested under another guard's tree).
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

    /// The directory itself.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Relinquish cleanup and hand the path back. For the rare test that
    /// leaves the tree to a subprocess outliving it (or deliberately
    /// plants a leftover for a later assertion).
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

/// Reclaim `portuale-td-*` directories left by processes that are no
/// longer running. Runs at most once per process, at the first
/// [`TempDir::new`].
///
/// A pid that has been recycled reads as alive and its predecessor's
/// directories wait for a later sweep -- conservative in the safe
/// direction (we never delete a live run's scratch out from under it).
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

/// Linux: `/proc/{pid}` exists iff some process owns that pid. The
/// workspace is a Linux/Gentoo package manager (musl static target); on
/// a kernel without procfs this returns `true` and the sweep stays out
/// of the way rather than deleting a live run's directories.
fn pid_is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_removes_itself_on_drop() {
        let tmp = TempDir::new("drop-probe");
        let path = tmp.path().to_path_buf();
        assert!(path.is_dir(), "{path:?} must exist while the guard is held");
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("portuale-td-drop-probe-"),
            "name carries the prefix and tag: {path:?}"
        );
        drop(tmp);
        assert!(!path.exists(), "{path:?} must be gone after drop");
    }

    #[test]
    fn temp_dir_removes_itself_on_unwind() {
        // Backlog #260's actual failure mode: the trailing
        // `remove_dir_all` at the end of a test is skipped when the test
        // panics. `catch_unwind` stands in for the panic.
        let probe = std::sync::Arc::new(std::sync::Mutex::new(None::<PathBuf>));
        let slot = probe.clone();
        let result = std::panic::catch_unwind(move || {
            let tmp = TempDir::new("unwind-probe");
            *slot.lock().unwrap() = Some(tmp.path().to_path_buf());
            panic!("test failure under the guard");
        });
        assert!(result.is_err(), "the inner panic must propagate");
        let path = probe.lock().unwrap().take().expect("guard created");
        assert!(
            !path.exists(),
            "{path:?} must be reclaimed even when the test panics"
        );
    }

    #[test]
    fn temp_dir_keep_relinquishes_cleanup() {
        let tmp = TempDir::new("keep-probe");
        let path = tmp.keep();
        assert!(path.is_dir(), "{path:?} survives keep()");
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn temp_dir_derefs_to_path() {
        let tmp = TempDir::new("deref-probe");
        // The shapes every converted call site uses.
        let child = tmp.join("child");
        std::fs::create_dir_all(&child).unwrap();
        assert!(child.is_dir());
        assert!(AsRef::<Path>::as_ref(&tmp).is_dir());
        assert!(format!("{tmp}").contains("portuale-td-deref-probe-"));
    }

    #[test]
    fn stale_sweep_reclaims_a_dead_pids_directories_and_keeps_our_own() {
        // Plant two `portuale-td-*` directories: one owned by this
        // process (must survive -- it is a live run's scratch) and one
        // owned by a pid that cannot exist (`u32::MAX`, which /proc
        // will not have). The sweep must reclaim only the dead one.
        let parent = TempDir::new("sweep-parent");
        let live = parent.path().join(format!(
            "{TEMP_DIR_PREFIX}sweep-live-{}-1",
            std::process::id()
        ));
        let dead = parent
            .path()
            .join(format!("{TEMP_DIR_PREFIX}sweep-dead-{}-1", u32::MAX));
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&dead).unwrap();
        // A non-`portuale-td-` neighbour is not ours and stays put.
        let foreign = parent.path().join("not-ours-1");
        fs::create_dir_all(&foreign).unwrap();

        stale_sweep(parent.path());

        assert!(live.is_dir(), "our own live directories must survive");
        assert!(!dead.exists(), "a dead pid's directories must be reclaimed");
        assert!(
            foreign.is_dir(),
            "unrecognised names are not ours to remove"
        );
    }

    #[test]
    fn parse_owner_pid_takes_the_last_two_components() {
        assert_eq!(parse_owner_pid("tag-123-456"), Some(123));
        // A tag with dashes and digits of its own must not shift the parse.
        assert_eq!(parse_owner_pid("161-run-missdep-999-888"), Some(999));
        assert_eq!(parse_owner_pid("a-b-c-1-2"), Some(1));
        // Missing the nanos tail (or not numeric) is not one of ours.
        assert_eq!(parse_owner_pid("tag-123"), None);
        assert_eq!(parse_owner_pid("tag-pid-nanos"), None);
        assert_eq!(parse_owner_pid(""), None);
    }
}
