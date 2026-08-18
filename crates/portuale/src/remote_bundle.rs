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

/// Walk `image/` (sorted, like the local `merge_tree`) recording type, MD5
/// and mtime per entry. Anything but a file, directory or symlink is an
/// error.
pub fn collect_filemeta(image: &Path) -> Result<Vec<FileMeta>, String> {
    use std::os::unix::fs::MetadataExt as _;
    fn md5_file(path: &Path) -> Result<String, String> {
        use md5::Digest as _;
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(format!("{:x}", md5::Md5::digest(&bytes)))
    }
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = image.join(&relative_dir);
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();
        for relative_path in children {
            let src = image.join(&relative_path);
            let meta =
                std::fs::symlink_metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            let file_type = meta.file_type();
            let rel = relative_path.to_string_lossy().replace('\\', "/");
            let mtime = meta.mtime();
            if file_type.is_symlink() {
                let target = std::fs::read_link(&src)
                    .map_err(|e| format!("{}: {e}", src.display()))?
                    .to_string_lossy()
                    .to_string();
                out.push(FileMeta {
                    kind: "sym",
                    md5: "-".to_string(),
                    mtime,
                    rel,
                    target: Some(target),
                });
            } else if file_type.is_dir() {
                stack.push(relative_path);
                out.push(FileMeta {
                    kind: "dir",
                    md5: "-".to_string(),
                    mtime,
                    rel,
                    target: None,
                });
            } else if file_type.is_file() {
                out.push(FileMeta {
                    kind: "obj",
                    md5: md5_file(&src)?,
                    mtime,
                    rel,
                    target: None,
                });
            } else {
                return Err(format!(
                    "{}: special file in image (only obj/dir/sym supported)",
                    src.display()
                ));
            }
        }
    }
    Ok(out)
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
