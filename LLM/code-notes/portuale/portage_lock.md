# crates/portuale/src/portage_lock.rs
Advisory file lock in the shape of Portage's `lockfile` primitive.

## module
- Ports `lockfile(mypath, wantnewlockfile=1)` (lib/portage/locks.py:175). Portage calls this same primitive at several genuinely separate points -- `fetch.py:1315-1330` around a distfile fetch-and-verify sequence, `bintree.py:950`/`:1999`/`:2059` around a `<pkgdir>/Packages` read-modify-write -- so this is a single shared helper rather than one copy per caller.
- Release is by closing the lock file's own fd (`Drop`), the same effect Portage's `unlockfile()` explicit `flock(fd, LOCK_UN)` has: POSIX guarantees all of a process's own `flock` locks on an fd are released when that fd is closed.
- Portage `unlinkfile=0` (every caller here uses the default): the lockfile itself persists on disk after release, just unlocked, ready for reuse.

## impl PortageLockfile::acquire
- Never truncates: Portage's `os.open(lockfile, O_CREAT)` doesn't `O_TRUNC` either (moot in practice since nothing is ever written to the lockfile, but explicit is cheap and correct).
- Blocks until the lock is available: Portage's default passes no `os.O_NONBLOCK`.
- SAFETY of the `flock` call: it takes only the fd's own raw handle (no pointers); `file` outlives the call and its fd is guaranteed valid by `OpenOptions::open`.
