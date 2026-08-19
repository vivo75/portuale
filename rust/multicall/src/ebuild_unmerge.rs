// Real package removal (task #55's own natural complement): `ebuild
// <file> unmerge` mirrors real `dblink.unmerge()` plus the top-level
// `unmerge()` function's own success-gated `dblink.delete()` call
// (`lib/portage/dbapi/vartree.py`): runs `pkg_prerm`, deletes every
// file/dir/symlink the vdb entry's own `CONTENTS` lists from `${ROOT}`
// (in real `_unmerge_pkgfiles()`'s own reverse-sorted order -- deepest
// paths first, so a directory always empties out before its own removal
// is attempted -- `mykeys.sort(); mykeys.reverse()`), runs `pkg_postrm`,
// and -- only if every step above succeeded -- removes the vdb entry
// itself (real `dblink.delete()`: `shutil.rmtree(self.dbdir)` plus an
// `rmdir` of the parent `<category>` directory if it's now empty).
// `pkg_prerm`/`pkg_postrm` run via `ebuild_phases::run_single_phase`, the
// same way `ebuild_merge::run_merge`'s own `pkg_preinst`/`pkg_postinst`
// do -- real `unmerge()` invokes them directly
// (`EbuildPhase(phase="prerm"/"postrm")`), not through `doebuild()`'s own
// `actionmap_deps` chain.
//
// Without this, `merge` alone can never be exercised through a real
// install/reinstall/removal cycle -- every merge would just accumulate
// vdb entries and files forever.
//
// Locally-modified files are protected on removal too, real `_unmerge_
// pkgfiles()`'s own actual mechanism for it: an `obj`/`sym` entry whose
// live, on-disk mtime no longer matches what `CONTENTS` recorded at
// merge time is left in place instead of deleted (real `!mtime` skip --
// broader than `CONFIG_PROTECT` alone, since it applies to *every*
// unmerge regardless of path, and it's what actually protects a
// CONFIG_PROTECT'd file on removal too: real `dblink._protect()`
// diverts a changed write to a `._cfgNNNN_` sibling while `CONTENTS`
// still records the *original* file's own now-stale mtime, so the real
// `/etc/foo.conf` a user edited never matches and is never touched).
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching `ebuild_merge`'s own
// "narrow v1, document the cut" pattern):
//   - No preserve-libs / "others in this slot" reverse-dependency
//     checking at all -- real `unmerge()` consults every other version
//     installed in the same slot before deciding whether a shared
//     library is still needed by something else.
//   - No stale-symlink/orphan-directory bookkeeping
//     (`FEATURES=unmerge-orphans`), no `bsd_chflags` handling, no
//     `INFOPATH` special-casing, no `CONFIG_PROTECT`-aware "already
//     replaced" (`replaced`) skip.
//   - Failure handling is coarser: real `_unmerge_pkgfiles()` counts
//     per-file failures and keeps going regardless (overall success is
//     governed by the `prerm`/`postrm` phase exit codes, not by
//     individual file-removal failures); this pilot's own removal loop
//     tolerates "already gone" (`NotFound`, matching real
//     `_ignored_unlink_errnos`/`_ignored_rmdir_errnos`) and "directory
//     not empty" (matching real `!empty` tolerance) but treats any
//     other I/O error as a hard failure.
//   - `${T}`/`${D}`/etc. are recomputed fresh via `ebuild_phases::
//     compute_environment` for every `unmerge` call, the same as
//     `merge` -- real `unmerge()` reuses whatever `PORTAGE_BUILDDIR` a
//     prior merge left behind when present; this pilot doesn't attempt
//     that distinction.

use crate::ebuild_merge;
use crate::ebuild_phases;
use std::path::Path;

/// Whether `command` is the one real unmerge command this module
/// implements -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_real_phase_command`/`ebuild_merge::is_real_merge_command` before
/// routing to real execution.
pub fn is_real_unmerge_command(command: &str) -> bool {
    command == "unmerge"
}

struct ContentsEntry {
    node_type: String,
    abs_path: String,
    /// Present for `obj`/`sym` lines (real `ebuild_merge::
    /// format_contents_line`'s own trailing field) -- `None` for `dir`
    /// (which has no trailing field at all: its own last
    /// whitespace-separated token is `abs_path` itself, non-numeric, so
    /// parsing it as an mtime naturally fails into `None`).
    mtime: Option<i64>,
}

/// Parses a real `CONTENTS` file's own lines (see `ebuild_merge::
/// format_contents_line` for the exact format written).
fn parse_contents(text: &str) -> Vec<ContentsEntry> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let node_type = (*fields.first()?).to_string();
            let abs_path = (*fields.get(1)?).to_string();
            let mtime = fields.last().and_then(|s| s.parse::<i64>().ok());
            Some(ContentsEntry {
                node_type,
                abs_path,
                mtime,
            })
        })
        .collect()
}

/// Deletes every `CONTENTS`-listed entry from `root`, deepest paths
/// first (see this module's own doc comment for why, and for the v1
/// failure-tolerance simplification) -- except an `obj`/`sym` entry
/// whose live mtime no longer matches what `CONTENTS` recorded, which is
/// left in place instead (real `!mtime` skip -- see this module's own
/// doc comment for why this is also what protects a CONFIG_PROTECT'd
/// file on removal).
fn remove_contents(root: &Path, contents_text: &str) -> Result<(), String> {
    let mut entries = parse_contents(contents_text);
    entries.sort_by(|a, b| b.abs_path.cmp(&a.abs_path));

    for entry in entries {
        let relative = entry.abs_path.trim_start_matches('/');
        let dest = root.join(relative);
        match entry.node_type.as_str() {
            "obj" | "sym" => {
                if let (Some(recorded_mtime), Ok(meta)) =
                    (entry.mtime, std::fs::symlink_metadata(&dest))
                {
                    let current_mtime = ebuild_merge::mtime_secs(&meta)?;
                    if current_mtime != recorded_mtime {
                        // Locally modified since the merge that recorded
                        // this entry -- leave it in place.
                        continue;
                    }
                }
                if let Err(e) = std::fs::remove_file(&dest) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(format!("{}: {e}", dest.display()));
                    }
                }
            }
            "dir" => {
                // Ignore both "already gone" and "not empty" -- matches
                // real `_ignored_rmdir_errnos`/`!empty` tolerance (a
                // directory another still-installed package also uses
                // is expected to fail here, harmlessly).
                let _ = std::fs::remove_dir(&dest);
            }
            _ => {
                // fifo/device nodes: `ebuild_merge::merge_tree` doesn't
                // create these either -- nothing to remove.
            }
        }
    }
    Ok(())
}

/// Real top-level `unmerge()`: `dblink.unmerge()` (`prerm` -> delete
/// files -> `postrm`), then -- only on success -- `dblink.delete()`
/// (remove the vdb entry itself).
pub fn run_unmerge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
    shell: ebuild_phases::ShellBackend,
) -> Result<i32, String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let vdb_dir = root
        .join("var/db/pkg")
        .join(&env.category)
        .join(&env.split.pf);
    let contents_path = vdb_dir.join("CONTENTS");
    let contents_text = std::fs::read_to_string(&contents_path)
        .map_err(|e| format!("{}: not installed ({e})", vdb_dir.display()))?;

    let prerm_status =
        ebuild_phases::run_single_phase(ebuild_path, "prerm", root, portage_tmpdir, debug, shell)?;
    if prerm_status != 0 {
        return Ok(prerm_status);
    }

    remove_contents(root, &contents_text)?;

    let postrm_status =
        ebuild_phases::run_single_phase(ebuild_path, "postrm", root, portage_tmpdir, debug, shell)?;
    if postrm_status != 0 {
        return Ok(postrm_status);
    }

    std::fs::remove_dir_all(&vdb_dir).map_err(|e| format!("{}: {e}", vdb_dir.display()))?;
    if let Some(cat_dir) = vdb_dir.parent() {
        // Real `delete()`'s own best-effort `os.rmdir` of the now-maybe-
        // empty parent category directory -- ignored on failure (another
        // installed package in the same category is the common case).
        let _ = std::fs::remove_dir(cat_dir);
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "multicall-ebuild-unmerge-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_real_unmerge_command_covers_exactly_unmerge() {
        assert!(is_real_unmerge_command("unmerge"));
        assert!(!is_real_unmerge_command("merge"));
        assert!(!is_real_unmerge_command("qmerge"));
        assert!(!is_real_unmerge_command("install"));
    }

    #[test]
    fn parse_contents_reads_type_and_path_ignoring_trailing_fields() {
        let text = "dir /usr\nobj /usr/hello.txt abc123 100\nsym /usr/link -> hello.txt 100\n";
        let entries = parse_contents(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].node_type, "dir");
        assert_eq!(entries[0].abs_path, "/usr");
        assert_eq!(entries[1].node_type, "obj");
        assert_eq!(entries[1].abs_path, "/usr/hello.txt");
        assert_eq!(entries[2].node_type, "sym");
        assert_eq!(entries[2].abs_path, "/usr/link");
    }

    #[test]
    fn remove_contents_deletes_files_symlinks_and_empties_directories() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/x")).unwrap();
        std::fs::write(root.join("usr/share/x/hello.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("hello.txt", root.join("usr/share/x/link.txt")).unwrap();

        let file_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/x/hello.txt")).unwrap(),
        )
        .unwrap();
        let link_mtime = ebuild_merge::mtime_secs(
            &std::fs::symlink_metadata(root.join("usr/share/x/link.txt")).unwrap(),
        )
        .unwrap();
        let contents = format!(
            "dir /usr\n\
             dir /usr/share\n\
             dir /usr/share/x\n\
             obj /usr/share/x/hello.txt abc123 {file_mtime}\n\
             sym /usr/share/x/link.txt -> hello.txt {link_mtime}\n"
        );

        remove_contents(&root, &contents).expect("remove_contents succeeds");

        assert!(!root.join("usr/share/x/hello.txt").exists());
        assert!(root
            .join("usr/share/x/link.txt")
            .symlink_metadata()
            .is_err());
        assert!(!root.join("usr/share/x").exists());
        assert!(!root.join("usr/share").exists());
        assert!(!root.join("usr").exists());
    }

    #[test]
    fn remove_contents_leaves_a_locally_modified_file_in_place() {
        // Real "!mtime" staleness check (see this module's own doc
        // comment): a file whose live mtime no longer matches what
        // CONTENTS recorded is treated as locally modified and left
        // alone -- the same real mechanism that also protects a
        // CONFIG_PROTECT'd file on removal (its own recorded mtime
        // reflects the ._cfg-diverted write, never the real file's own).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"user's own edits").unwrap();

        // A recorded mtime that can never match the file just written
        // above (created "now").
        let contents = "obj /etc.conf abc123 1\n";
        remove_contents(&root, contents).expect("remove_contents succeeds");

        assert!(
            root.join("etc.conf").is_file(),
            "a locally-modified file must survive unmerge"
        );
    }

    #[test]
    fn remove_contents_tolerates_a_directory_still_shared_with_another_package() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        // A file this CONTENTS doesn't know about, simulating another
        // still-installed package's own file in the same shared dir.
        std::fs::write(root.join("usr/share/other.txt"), b"other").unwrap();

        let contents = "dir /usr\ndir /usr/share\n";
        remove_contents(&root, contents).expect("remove_contents succeeds");

        // /usr/share survives (non-empty), and so does its parent.
        assert!(root.join("usr/share/other.txt").is_file());
    }

    #[test]
    fn remove_contents_tolerates_entries_already_gone() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let contents = "obj /usr/share/x/hello.txt abc123 100\ndir /usr/share/x\n";
        remove_contents(&root, contents).expect("missing entries are not an error");
    }

    #[test]
    fn real_unmerge_removes_a_previously_real_merged_package() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let merge_status = crate::ebuild_merge::run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_merge::MergeOptions::default(),
        )
        .expect("run_merge succeeds");
        assert_eq!(merge_status, 0);
        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        assert!(vdb_dir.is_dir());

        let unmerge_status = run_unmerge(
            &ebuild,
            &root,
            &portage_tmpdir,
            false,
            ebuild_phases::ShellBackend::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(!root.join("usr/share/mergepkg/hello.txt").exists());
        assert!(!root.join("usr/share/mergepkg/hello-link.txt").exists());
        assert!(!root.join("usr/share/mergepkg").exists());
        assert!(!vdb_dir.exists());
        // The category dir itself is now empty too.
        assert!(!root.join("var/db/pkg/dev-libs").exists());
    }

    #[test]
    fn unmerge_of_a_never_installed_package_fails_clearly() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let result = run_unmerge(
            &ebuild,
            &root,
            &portage_tmpdir,
            false,
            ebuild_phases::ShellBackend::default(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not installed"));
    }
}
