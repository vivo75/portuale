//! Advisory file lock in the shape of Portage's `lockfile`: a
//! `.{basename}.portage_lockfile` sibling held with a blocking exclusive
//! `flock(2)` for the guard's lifetime; closing the fd releases it. The
//! `wantnewlockfile` shape additionally unlinks the file on release, like
//! Portage's `unlockfile` with the implied `unlinkfile=1`.
// Portage: locks.py, lockfile

use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// A held [`PortageLockfile`] guard. Held open for as long as it lives.
pub(crate) struct PortageLockfile {
    _file: std::fs::File,
    /// Set only for the `wantnewlockfile` shape: the sibling path to unlink
    /// on drop. `None` keeps the file behind, like a plain `lockfile`.
    unlink_on_drop: Option<PathBuf>,
}

impl PortageLockfile {
    /// Portage's `lockfile(path, wantnewlockfile=1)`: the sibling lock file
    /// is removed on release (`locks.py` forces `unlinkfile=1` for this
    /// shape, and `unlockfile` unlinks it once it holds the only link).
    pub(crate) fn acquire_new_lockfile(mypath: &Path) -> Result<Self, String> {
        let parent = mypath.parent().unwrap_or_else(|| Path::new("."));
        let basename = mypath.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lock_path = parent.join(format!(".{basename}.portage_lockfile"));
        Self::acquire_inner(mypath, Some(lock_path))
    }

    fn acquire_inner(mypath: &Path, unlink_on_drop: Option<PathBuf>) -> Result<Self, String> {
        let parent = mypath.parent().unwrap_or_else(|| Path::new("."));
        let basename = mypath.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lock_path = parent.join(format!(".{basename}.portage_lockfile"));
        // A holder of the `wantnewlockfile` shape unlinks the file on
        // release, so a waiter can be granted the lock on an inode that no
        // longer has the name. Like Portage, check after locking that the
        // path still names the locked inode, and retry if not.
        // Portage: locks.py, _lockfile_was_removed
        let file = loop {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                // Never truncate: only the lock itself matters, and truncating
                // would race a concurrent holder's in-flight read of the file.
                .truncate(false)
                .open(&lock_path)
                .map_err(|e| format!("{}: {e}", lock_path.display()))?;
            // Block until the lock is available (no `O_NONBLOCK`).
            // SAFETY: `flock` takes only the fd's own raw handle (no
            // pointers); `file` outlives the call and its fd is valid.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(format!(
                    "{}: {}",
                    lock_path.display(),
                    std::io::Error::last_os_error()
                ));
            }
            use std::os::unix::fs::MetadataExt;
            let held = file.metadata().map(|m| (m.dev(), m.ino()));
            let named = std::fs::metadata(&lock_path).map(|m| (m.dev(), m.ino()));
            match (held, named) {
                (Ok(h), Ok(n)) if h == n => break file,
                _ => continue,
            }
        };
        Ok(Self {
            _file: file,
            unlink_on_drop,
        })
    }
}

impl Drop for PortageLockfile {
    fn drop(&mut self) {
        // Portage's `unlockfile` with `unlinkfile=1`: re-take the lock
        // non-blocking (trivially held: this fd still owns it) and unlink
        // the path while it has no other link, before closing releases the
        // `flock`. Failures are ignored: dropping must not panic.
        // Portage: locks.py, unlockfile
        if let Some(lock_path) = &self.unlink_on_drop {
            // SAFETY: as in `acquire_inner`; the fd is open until the end
            // of `drop`.
            let relocked =
                unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
            if relocked {
                use std::os::unix::fs::MetadataExt;
                let single_link = self
                    ._file
                    .metadata()
                    .map(|st| st.nlink() == 1)
                    .unwrap_or(false);
                if single_link {
                    let _ = std::fs::remove_file(lock_path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unlink-on-release shape keeps mutual exclusion: a waiter granted
    /// the lock on an inode the previous holder just unlinked retries on the
    /// new file instead of running beside a newcomer that created it.
    // Portage: locks.py, _lockfile_was_removed
    #[test]
    fn new_lockfile_holders_never_overlap() {
        let dir = portage_util::TempDir::new("portage_lock_overlap");
        let target = dir.path().join("Packages");
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::scope(|s| {
            for _ in 0..8 {
                let (target, inside) = (&target, inside.clone());
                s.spawn(move || {
                    for _ in 0..200 {
                        let _lock = PortageLockfile::acquire_new_lockfile(target).unwrap();
                        let n = inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        assert_eq!(n, 0, "two holders inside the lock at once");
                        std::thread::yield_now();
                        inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    }
                });
            }
        });
        assert!(!dir.path().join(".Packages.portage_lockfile").exists());
    }
}
