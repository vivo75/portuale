// The running binary's path, resolved once (#331).
//
// Every phase and generated script reaches portuale again through
// `PORTUALE_BIN` (the `portageq-wrapper` shim, the `__helper` shims). Linux
// reports a running executable whose file was replaced or unlinked as
// `<path> (deleted)` (`/proc/self/exe`), so asking `current_exe()` after a
// `cargo build`, or after `emerge` merged a new portuale over the running
// one, exports a path that does not exist and every later phase dies.
//
// Real Portage re-execs nothing of its own: its helpers are files under
// `PORTAGE_BIN_PATH`, which a portage merge replaces in place, so later
// phases simply run the new files. Resolving the path once at startup,
// and stripping a ` (deleted)` suffix from a late first resolution, gives
// the same behaviour: later execs open whatever file is at the path now.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SELF_EXE: OnceLock<PathBuf> = OnceLock::new();

/// The path of the running portuale binary, resolved on first use
/// (`main` calls this before any phase can run). Falls back to the bare
/// name `portuale` when the kernel cannot report it.
pub(crate) fn self_exe() -> &'static Path {
    SELF_EXE.get_or_init(|| {
        std::env::current_exe()
            .map(strip_deleted)
            .unwrap_or_else(|_| PathBuf::from("portuale"))
    })
}

/// Drops the ` (deleted)` marker the kernel appends to the link target of
/// `/proc/<pid>/exe` once the file behind it is gone.
fn strip_deleted(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(stripped) => PathBuf::from(stripped),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_deleted_drops_only_the_kernel_marker() {
        assert_eq!(
            strip_deleted(PathBuf::from("/usr/local/bin/portuale (deleted)")),
            PathBuf::from("/usr/local/bin/portuale")
        );
        assert_eq!(
            strip_deleted(PathBuf::from("/usr/bin/portuale")),
            PathBuf::from("/usr/bin/portuale")
        );
        // A name that merely contains the words keeps them.
        assert_eq!(
            strip_deleted(PathBuf::from("/opt/x (deleted)/portuale")),
            PathBuf::from("/opt/x (deleted)/portuale")
        );
    }
}
