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

// ---------------------------------------------------------------------
// Cleanup on panic (release builds abort, so `Drop` never runs)
// ---------------------------------------------------------------------

mod cleanup {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

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
