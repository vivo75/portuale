// Real `lockfile(mypath, wantnewlockfile=1)` (`lib/portage/locks.py:175`):
// a real, separate `.{basename}.portage_lockfile` sibling of `mypath`,
// locked via a real, blocking `flock(2)` exclusive lock for the lifetime
// of the returned guard. Real portage calls this same primitive at
// several genuinely separate points -- `fetch.py:1315-1330` around a
// distfile fetch-and-verify sequence, `bintree.py:950`/`:1999`/`:2059`
// around a `<pkgdir>/Packages` read-modify-write -- so this is a single
// shared helper rather than one copy per caller. Released by simply
// closing the lock file's own fd (`Drop`), the same real effect real
// `unlockfile()`'s own explicit `flock(fd, LOCK_UN)` has -- POSIX
// guarantees all of a process's own `flock` locks on an fd are released
// when that fd is closed. Real `unlinkfile=0` (every real caller here
// uses the default): the lockfile itself persists on disk after release,
// just unlocked, ready for reuse.

use std::os::unix::io::AsRawFd;
use std::path::Path;

/// See this module's own doc comment for the full real grounding. Held
/// open for as long as the returned guard lives.
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
            // Never truncate: only the lock itself matters, real
            // `os.open(lockfile, O_CREAT)` doesn't `O_TRUNC` either, and
            // truncating would race a concurrent holder's own in-flight
            // read of the file (moot in practice since nothing is ever
            // written to it, but explicit is cheap and correct).
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("{}: {e}", lock_path.display()))?;
        // Real default (no `os.O_NONBLOCK`): block until the lock is
        // available.
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
