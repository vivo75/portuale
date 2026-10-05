//! The phase runtime and package-set definitions compiled into the binary.
//!
//! `build.rs` embeds `bin/` and `cnf/`; this module extracts them back to
//! files so a relocated binary runs phases without a build tree.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) enum Kind {
    Dir,
    File(&'static [u8]),
    Symlink(&'static str),
}

pub(crate) struct Entry {
    /// Path relative to the repository root, `/`-separated (`bin/ebuild.sh`).
    pub(crate) path: &'static str,
    pub(crate) mode: u32,
    pub(crate) kind: Kind,
}

include!(concat!(env!("OUT_DIR"), "/embedded_runtime.rs"));

/// The embedded bytes of one regular file, by repository-relative path.
pub(crate) fn file(path: &str) -> Option<&'static [u8]> {
    ENTRIES
        .iter()
        .find_map(|e| match (&e.kind, e.path == path) {
            (Kind::File(bytes), true) => Some(*bytes),
            _ => None,
        })
}

/// Writes the entries under `prefix` (`bin`) into `dest`, so that
/// `<prefix>/x` becomes `<dest>/x`. `dest` is created; an existing symlink or
/// file at a target is replaced. Modes are set explicitly after writing, so
/// the process umask does not matter.
pub(crate) fn extract(prefix: &str, dest: &Path) -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for entry in ENTRIES {
        let rel = if entry.path == prefix {
            ""
        } else if let Some(rest) = entry
            .path
            .strip_prefix(prefix)
            .and_then(|r| r.strip_prefix('/'))
        {
            rest
        } else {
            continue;
        };
        let target = if rel.is_empty() {
            dest.to_path_buf()
        } else {
            dest.join(rel)
        };
        match &entry.kind {
            Kind::Dir => {
                std::fs::create_dir_all(&target)?;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(entry.mode))?;
            }
            Kind::File(bytes) => {
                let _ = std::fs::remove_file(&target);
                std::fs::write(&target, bytes)?;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(entry.mode))?;
            }
            Kind::Symlink(to) => {
                let _ = std::fs::remove_file(&target);
                symlink(to, &target)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }
}
