// The running binary's path, resolved once.
//
// Every phase and generated script re-execs portuale through `PORTUALE_BIN`.
// Resolving once (and stripping a ` (deleted)` suffix from a late first
// resolution) keeps later execs opening whatever file is at the path now.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SELF_EXE: OnceLock<PathBuf> = OnceLock::new();

/// The path of the running portuale binary, resolved on first use.
/// Falls back to the bare name `portuale` when the kernel cannot report it.
pub(crate) fn self_exe() -> &'static Path {
    SELF_EXE.get_or_init(|| {
        std::env::current_exe()
            .map(strip_deleted)
            .unwrap_or_else(|_| PathBuf::from("portuale"))
    })
}

/// Drops the ` (deleted)` marker the kernel appends to `/proc/<pid>/exe`.
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
