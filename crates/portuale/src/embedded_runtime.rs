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

    /// What a tree looks like for comparison: relative path -> a short
    /// description (`dir`, `exec`/`file` + bytes, `-> target`).
    #[derive(Debug, PartialEq)]
    enum Node {
        Dir,
        File { exec: bool, bytes: Vec<u8> },
        Link(String),
    }

    /// A phase-running merge works from an extracted runtime, never the
    /// live `bin/`.
    #[test]
    fn a_phase_running_merge_works_from_an_extracted_runtime() {
        let tmp = TempDir::new("embedded-runtime-merge");
        let bin = tmp.join("rt/bin");
        extract("bin", &bin).unwrap();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let fixtures = repo().join("fixtures");
        let exe = crate::self_exe::built_portuale_bin();
        let out = std::process::Command::new(exe)
            .env(crate::ebuild_phases::BIN_DIR_VAR, &bin)
            .env("PORTAGE_CONFIGROOT", &fixtures)
            .env("ROOT", &fixtures)
            .env("PORTAGE_RUNNING_ROOT", &fixtures)
            .env("DISTDIR", fixtures.join("distfiles"))
            .args([
                "mrg",
                "--remote-hostname=localtest",
                "--remote-transport=local",
            ])
            .arg(format!("--remote-root={}", root.display()))
            .arg(format!("--remote-workdir={}", tmp.join("work").display()))
            .arg(format!(
                "--remote-binpkg={}",
                fixtures
                    .join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2")
                    .display()
            ))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");
        assert!(
            stdout.contains(">>> Remote merged dev-libs/binpkgrmpkg-1.0"),
            "{stdout}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n"
        );
    }
}
