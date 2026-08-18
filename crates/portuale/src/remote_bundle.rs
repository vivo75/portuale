//! Remote-merge wire bundles: one binpkg prepared for stdin streaming.
//!
//! Layout inside the uncompressed tar:
//!
//! ```text
//! <pf>/image/...           the merge image (${D} on the client)
//! <pf>/build-info/...      CONTENTS, ebuild, DEFINED_PHASES, *DEPEND, ...
//! <pf>/environment         decompressed hook env (absent when the binpkg
//!                          carries no environment.bz2)
//! <pf>/remote-manifest     FORMAT=1 + CPV/SLOT/REPO/HAS_ENVIRONMENT
//! <pf>/bin/...             the ebuild runtime (`bash bin/ebuild.sh`)
//! ```
//!
//! The byte count travels beside the stream, not inside it, so the client
//! can reject a truncated transfer before touching anything.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One image entry for the client's merge: type, content hash (files
/// only), source mtime, and `/`-rooted relative path.
pub struct FileMeta {
    /// `obj`, `dir` or `sym`.
    pub kind: &'static str,
    /// Hex MD5 of file bytes (`obj`), `-` otherwise.
    pub md5: String,
    /// Source mtime seconds, recorded verbatim into CONTENTS.
    pub mtime: i64,
    /// Path relative to the image root, `/`-separated, no leading `/`.
    pub rel: String,
    /// Symlink target (`sym` only).
    pub target: Option<String>,
}

/// Which of the client phases (`pretend`/`setup`/`preinst`) a
/// binpkg's `DEFINED_PHASES` actually defines -- and only when the ebuild
/// file plus the hook environment are present to run them from.
pub fn select_phases(defined_phases: &str, has_ebuild: bool, has_environment: bool) -> Vec<String> {
    if !(has_ebuild && has_environment) {
        return Vec::new();
    }
    ["pretend", "setup", "preinst"]
        .into_iter()
        .filter(|phase| defined_phases.split_whitespace().any(|word| word == *phase))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }
}
