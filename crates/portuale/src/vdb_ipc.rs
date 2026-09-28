// The parent pipe (`PORTUALE_VDB_IPC`): installed-package lookups while
// `mrg` holds a redb database read-write.
//
// redb allows one writer process per database file, so the native
// `portuale portageq` that an ebuild phase runs for `has_version` /
// `best_version` cannot open it mid-merge. The parent `mrg` answers over a
// Unix domain socket and exports its path in `PORTUALE_VDB_IPC`.
// Portage: ebuild-ipc (a phase asks the parent emerge over a FIFO).
//
// One connection, one request line, one reply line; fields are separated
// by TAB, and no field may hold a TAB or a newline.
//
// ```text
// match\t<eroot>\t<atom>\t<unevaluated atom or empty>\t<implicit IUSE>\n
//     -> ok\t<version>\n          (the highest matching installed version)
//     -> none\n                   (nothing matches)
// categories\t<eroot>\t<pn>\n
//     -> ok\t<cat> <cat> ...\n    (categories with <pn> installed)
// anything else, or a request the server cannot answer
//     -> err\t<message>\n
// ```
//
// `<eroot>` must name the ROOT the server serves; the implicit IUSE is sent
// only when the atom has USE dependencies.
//
// `mrg` starts the server (redb, not `--pretend`) before `pretend::run` and
// keeps the returned [`Server`] alive for the whole run. Dropping it stops
// the accept thread and removes the socket and its directory.
//
// The server thread only reads, and each connection is served on its own
// short-lived thread with timeouts, so a stuck client never blocks the
// merge. A read sees the last committed state, never the merge's open write
// transaction nor its pending entry.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The variable that carries the socket path to the children.
pub(crate) const IPC_VAR: &str = "PORTUALE_VDB_IPC";
/// Name prefix of the private socket directory.
const DIR_PREFIX: &str = "portuale-vdb-ipc-";

/// A running server. Dropping it stops the accept thread and removes the
/// socket and its directory.
#[derive(Debug)]
pub(crate) struct Server {
    dir: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking `accept`; it sees the flag and returns.
        if UnixStream::connect(&self.socket).is_ok()
            && let Some(t) = self.thread.take()
        {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.dir);
        cleanup::unregister(&self.dir);
    }
}

/// `mkdir <parent>/portuale-vdb-ipc-<pid>-<nanos>` with mode 0700 (never
/// an existing directory).
fn make_private_dir(parent: &Path) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut last = None;
    for attempt in 0..16u128 {
        let dir = parent.join(format!(
            "{DIR_PREFIX}{}-{}",
            std::process::id(),
            nanos + attempt
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                // The umask can only remove bits; make sure of 0700.
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free socket directory name")))
}

// ---------------------------------------------------------------------
// Cleanup on panic (release builds abort, so `Drop` never runs)
// ---------------------------------------------------------------------

mod cleanup {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static LIVE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn unregister(dir: &Path) {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|d| d != dir);
        }
    }

    #[cfg(test)]
    fn install_hook() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "vdb-redb")]
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .unwrap()
    }
}
