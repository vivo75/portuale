//! Advisory file lock in the shape of Portage's `lockfile`: a
//! `.{basename}.portage_lockfile` sibling held with a blocking exclusive
//! `flock(2)` for the guard's lifetime; closing the fd releases it.
// Portage: locks.py, lockfile

use std::os::unix::io::AsRawFd;
use std::path::Path;

/// A held [`PortageLockfile`] guard. Held open for as long as it lives.
pub(crate) struct PortageLockfile {
    _file: std::fs::File,
}

impl PortageLockfile {
    pub(crate) fn acquire(mypath: &Path) -> Result<Self, String> {
        let parent = mypath.parent().unwrap_or_else(|| Path::new("."));
        let basename = mypath.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lock_path = parent.join(format!(".{basename}.portage_lockfile"));
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
        Ok(Self { _file: file })
    }
}
