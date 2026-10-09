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

/// The `portuale` binary beside the running test executable, for the unit
/// tests that spawn it as a child process.
///
/// `cargo test` does not build the binary target, so on a fresh tree the path
/// is missing and on a stale tree it predates the current sources: either way
/// the child would run the wrong code (or fail to spawn) far from the cause.
/// Fail fast instead, naming the fix.
///
/// "Stale" means a file in the binary's own cargo dep-info (`portuale.d`
/// beside it) is newer than the binary. Test-only sources are not in that
/// list, so editing a test never asks for a rebuild `cargo build` would skip.
#[cfg(test)]
pub(crate) fn built_portuale_bin() -> PathBuf {
    let mut bin = std::env::current_exe().expect("current test exe");
    bin.pop();
    if bin.ends_with("deps") {
        bin.pop();
    }
    bin.push("portuale");
    let Some(bin_mtime) = std::fs::metadata(&bin).ok().and_then(|m| m.modified().ok()) else {
        panic!(
            "run `cargo build --release` first: {} is missing",
            bin.display()
        );
    };
    if let Some(newer) = newer_dep(&bin, bin_mtime) {
        panic!(
            "run `cargo build --release` first: {} is older than {}",
            bin.display(),
            newer.display()
        );
    }
    bin
}

/// The first file in `<bin>.d` (cargo's dep-info: `target: dep dep ...`,
/// spaces in paths escaped as `\ `) whose mtime is after `bin_mtime`.
/// Cached per process: nothing under test rewrites sources. No dep-info
/// file means no check.
#[cfg(test)]
fn newer_dep(bin: &Path, bin_mtime: std::time::SystemTime) -> Option<PathBuf> {
    static NEWER: OnceLock<Option<PathBuf>> = OnceLock::new();
    NEWER
        .get_or_init(|| {
            let info = std::fs::read_to_string(bin.with_extension("d")).ok()?;
            let deps = info
                .lines()
                .next()?
                .split_once(": ")?
                .1
                .replace("\\ ", "\0");
            deps.split_whitespace()
                .map(|d| PathBuf::from(d.replace('\0', " ")))
                .find(|d| {
                    std::fs::metadata(d)
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| t > bin_mtime)
                })
        })
        .clone()
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
