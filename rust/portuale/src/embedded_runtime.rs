// The phase runtime and package-set definitions compiled into the binary
// (backlog #322 S2). `build.rs` walks the repository's `bin/` and `cnf/`
// and writes `ENTRIES`; this module turns that table back into files.
//
// Why: the vendored `bin/` is the bash half of Portage (`ebuild.sh` and its
// source closure, the `ebuild-helpers/`), located at run time through the
// path of the tree the binary was built in. A binary copied to a container
// or a minimal host has no such tree. With the table embedded, it extracts
// its own copy (see `ebuild_phases::bin_dir`), so the one file is enough.
//
// Fidelity: modes are normalised to 0o755 (any exec bit) / 0o644, symlinks
// are kept with their relative targets, order is sorted so a directory comes
// before its contents. No mtimes are recorded.

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

    fn live(root: &Path, rel: &str, out: &mut BTreeMap<String, Node>) {
        out.insert(rel.to_string(), Node::Dir);
        for e in std::fs::read_dir(root.join(rel)).unwrap() {
            let e = e.unwrap();
            let rel = format!("{rel}/{}", e.file_name().to_string_lossy());
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                live(root, &rel, out);
            } else if ft.is_symlink() {
                let t = std::fs::read_link(e.path()).unwrap();
                out.insert(rel, Node::Link(t.to_string_lossy().into_owned()));
            } else {
                let exec = e.metadata().unwrap().permissions().mode() & 0o111 != 0;
                out.insert(
                    rel,
                    Node::File {
                        exec,
                        bytes: std::fs::read(e.path()).unwrap(),
                    },
                );
            }
        }
    }

    fn live_tree(rel: &str) -> BTreeMap<String, Node> {
        let mut m = BTreeMap::new();
        live(&repo(), rel, &mut m);
        m
    }

    fn embedded_tree(prefix: &str) -> BTreeMap<String, Node> {
        ENTRIES
            .iter()
            .filter(|e| e.path == prefix || e.path.starts_with(&format!("{prefix}/")))
            .map(|e| {
                let node = match &e.kind {
                    Kind::Dir => Node::Dir,
                    Kind::File(b) => Node::File {
                        exec: e.mode & 0o111 != 0,
                        bytes: b.to_vec(),
                    },
                    Kind::Symlink(t) => Node::Link((*t).to_string()),
                };
                (e.path.to_string(), node)
            })
            .collect()
    }

    /// Backlog #322 S2.1: the compiled-in table is exactly the live `bin/`
    /// and `cnf/` trees: same paths, bytes, exec bits, symlink targets, no
    /// extra and no missing entries. (A stale table would mean a missed
    /// `rerun-if-changed`.)
    #[test]
    fn the_embedded_table_is_exactly_the_live_bin_and_cnf_trees() {
        for prefix in ["bin", "cnf"] {
            assert_eq!(embedded_tree(prefix), live_tree(prefix), "{prefix}");
        }
        assert!(file("bin/ebuild.sh").is_some_and(|b| !b.is_empty()));
        assert!(file("cnf/sets/portage.conf").is_some_and(|b| !b.is_empty()));
        assert!(
            file("bin/ebuild-helpers").is_none(),
            "a directory is not a file"
        );
    }

    /// S2.2: extraction reproduces `bin/` (bytes, exec bits, relative
    /// symlinks), sets the normalised modes explicitly, and is idempotent
    /// (a second extraction over the first changes nothing).
    #[test]
    fn extracting_the_embedded_bin_reproduces_the_live_tree_and_is_idempotent() {
        let tmp = TempDir::new("embedded-runtime-extract");
        let dest = tmp.join("rt/bin");
        extract("bin", &dest).unwrap();
        extract("bin", &dest).unwrap();

        let mut got = BTreeMap::new();
        // Re-key the extracted tree under `bin` to compare with the live one.
        let mut raw = BTreeMap::new();
        live(&tmp.join("rt"), "bin", &mut raw);
        got.extend(raw);
        assert_eq!(got, live_tree("bin"));

        let mode = |p: &str| {
            std::fs::metadata(dest.join(p))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!(mode("ebuild.sh"), 0o755);
        assert_eq!(mode("isolated-functions.sh"), 0o644);
        assert_eq!(mode("ebuild-helpers"), 0o755);
        // A symlink stays a symlink to the same relative target.
        assert_eq!(
            std::fs::read_link(dest.join("ebuild-helpers/ewarn")).unwrap(),
            Path::new("elog")
        );
    }

    /// S2.5 in-process half: a phase-running remote merge (the hook-ordering
    /// fixture: `pkg_setup`, `pkg_preinst`, `pkg_postinst` on the client) driven
    /// by the binary with `PORTUALE_BIN_DIR` at an *extracted* copy of the
    /// embedded runtime, never the live `bin/`. Proves the extracted tree
    /// works as a phase runtime (modes, symlinks, helper lookups). The
    /// relocated-binary half (no build tree at all) is the container run in
    /// `docs/evidence/322-s2.md`.
    #[test]
    fn a_phase_running_merge_works_from_an_extracted_runtime() {
        let tmp = TempDir::new("embedded-runtime-merge");
        let bin = tmp.join("rt/bin");
        extract("bin", &bin).unwrap();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let fixtures = repo().join("fixtures");
        let mut exe = std::env::current_exe().unwrap();
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
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

    /// S2.7: the set definitions fall back to the embedded copy, and the
    /// copy is the checkout's file when both exist (so `--list-sets` is
    /// byte-identical in-tree and relocated).
    #[test]
    fn the_embedded_package_sets_conf_equals_the_checkout_file() {
        let embedded = file("cnf/sets/portage.conf").expect("embedded");
        if let Ok(checkout) = std::fs::read(repo().join("3rdparty/portage/cnf/sets/portage.conf")) {
            assert_eq!(embedded, checkout.as_slice(), "vendored cnf/ copy is stale");
        }
        let text = crate::ebuild_phases::package_sets_conf().expect("a source");
        assert_eq!(text.as_bytes(), embedded);
    }
}
