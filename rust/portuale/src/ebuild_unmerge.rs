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
// `others_in_slot` reverse-dependency checking is real too: real
// `_unmerge_pkgfiles()`'s own `is_owned` check (`vartree.py:2893-2916`,
// via `dblink.isowner()` == `bool(self._match_contents(filename))`) --
// before a `CONTENTS` entry is even considered for the mtime check,
// every *other* installed version of the same `category/PN` in the same
// `SLOT` (excluding self) is asked whether its own `CONTENTS` also
// claims that exact path; if so, the entry is left alone entirely
// (real `"replaced"` skip) regardless of node type. This is what makes
// an in-place upgrade not delete files the new version also owns --
// without it, `merge`-then-`merge` (a reinstall/upgrade) followed by
// `unmerge`ing the *old* vdb entry would have deleted files the new
// installation still depends on. Reuses `ebuild_merge::owns_path_pf`
// (already built for real blocker exclusion's own `CONTENTS`-ownership
// check) and `ebuild_merge::read_installed_slot`, both promoted to
// `pub(crate)` for this. Real `isowner`'s own path-ambiguity-via-
// symlinked-directories inode-cache mechanism isn't reproduced --
// this pilot's own `owns_path_pf` is a plain string scan, matching the
// same simplification `find_owners`/blocker exclusion already made.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching `ebuild_merge`'s own
// "narrow v1, document the cut" pattern):
//   - The real "symlink orphan" refinement (bug #326685: a symlink to a
//     directory, where the *new* owner recorded that directory as a
//     plain `dir` entry rather than a `sym`) isn't reproduced -- real
//     `_unmerge_pkgfiles()` does an extra `all_owned`/child-by-child
//     scan in that specific case to decide whether the symlink itself
//     is still safe to remove; this pilot's own `is_owned` check already
//     covers the common case (the path is skipped outright whenever
//     another same-slot instance owns it at all), just not this
//     narrower "some of the target directory's children moved to being
//     owned differently" sub-case.
//   - No stale-symlink/orphan-directory bookkeeping
//     (`FEATURES=unmerge-orphans`), no `bsd_chflags` handling, no
//     `INFOPATH` special-casing.
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
/// failure-tolerance simplification) -- except: an entry another
/// same-`category/PN`-and-`SLOT` installed package (`others_in_slot`,
/// bare `PF` strings) also owns, real `is_owned`/`"replaced"` skip (see
/// this module's own doc comment); or an `obj`/`sym` entry whose live
/// mtime no longer matches what `CONTENTS` recorded, left in place
/// instead (real `!mtime` skip -- see this module's own doc comment for
/// why this is also what protects a CONFIG_PROTECT'd file on removal).
fn remove_contents(
    root: &Path,
    category: &str,
    others_in_slot: &[String],
    contents_text: &str,
) -> Result<(), String> {
    let mut entries = parse_contents(contents_text);
    entries.sort_by(|a, b| b.abs_path.cmp(&a.abs_path));

    for entry in entries {
        if others_in_slot
            .iter()
            .any(|other_pf| ebuild_merge::owns_path_pf(root, category, other_pf, &entry.abs_path))
        {
            // Real "replaced" skip: another still-installed version of
            // this same cp:slot also claims this path -- most commonly
            // an in-place upgrade sharing files with the version being
            // unmerged. Checked before the mtime check, matching real
            // `_unmerge_pkgfiles()`'s own ordering.
            continue;
        }
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

    // Real `others_in_slot`: every other installed version of this same
    // category/PN in the same SLOT, excluding self -- see this module's
    // own doc comment.
    let own_slot = ebuild_merge::read_installed_slot(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf[env.split.pn.len() + 1..],
    );
    let others_in_slot: Vec<String> = match &own_slot {
        Some(slot) => portage_repo::installed_versions(root, &env.category, &env.split.pn)
            .into_iter()
            .map(|version| format!("{}-{version}", env.split.pn))
            .filter(|pf| pf != &env.split.pf)
            .filter(|pf| {
                let version = &pf[env.split.pn.len() + 1..];
                ebuild_merge::read_installed_slot(root, &env.category, &env.split.pn, version)
                    .as_deref()
                    == Some(slot.as_str())
            })
            .collect(),
        None => Vec::new(),
    };

    let prerm_status =
        ebuild_phases::run_single_phase(ebuild_path, "prerm", root, portage_tmpdir, debug, shell)?;
    if prerm_status != 0 {
        return Ok(prerm_status);
    }

    remove_contents(root, &env.category, &others_in_slot, &contents_text)?;

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
            "portuale-ebuild-unmerge-test-{}-{}",
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

        remove_contents(&root, "dev-libs", &[], &contents).expect("remove_contents succeeds");

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
        remove_contents(&root, "dev-libs", &[], contents).expect("remove_contents succeeds");

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
        remove_contents(&root, "dev-libs", &[], contents).expect("remove_contents succeeds");

        // /usr/share survives (non-empty), and so does its parent.
        assert!(root.join("usr/share/other.txt").is_file());
    }

    #[test]
    fn remove_contents_tolerates_entries_already_gone() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let contents = "obj /usr/share/x/hello.txt abc123 100\ndir /usr/share/x\n";
        remove_contents(&root, "dev-libs", &[], contents)
            .expect("missing entries are not an error");
    }

    #[test]
    fn remove_contents_skips_a_path_another_same_slot_package_still_owns() {
        // Real `is_owned`/"replaced" skip (see this module's own doc
        // comment): a path is left alone entirely -- not even reaching
        // the mtime check -- whenever some `others_in_slot` entry's own
        // real vdb `CONTENTS` also claims it.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        std::fs::write(root.join("usr/share/shared.txt"), b"shared").unwrap();
        std::fs::write(root.join("usr/share/only-mine.txt"), b"mine").unwrap();

        // Real, matching mtimes for both files -- so the "shared.txt"
        // skip below can only be explained by the `is_owned` check
        // itself, not incidentally by an `!mtime` mismatch.
        let shared_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/shared.txt")).unwrap(),
        )
        .unwrap();
        let mine_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/only-mine.txt")).unwrap(),
        )
        .unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/otherpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(
            other_vdb.join("CONTENTS"),
            format!("obj /usr/share/shared.txt abc123 {shared_mtime}\n"),
        )
        .unwrap();

        let contents = format!(
            "obj /usr/share/shared.txt abc123 {shared_mtime}\n\
             obj /usr/share/only-mine.txt abc123 {mine_mtime}\n"
        );
        remove_contents(&root, "dev-libs", &["otherpkg-2.0".to_string()], &contents)
            .expect("remove_contents succeeds");

        assert!(
            root.join("usr/share/shared.txt").is_file(),
            "a path another same-slot package still owns must survive unmerge"
        );
        assert!(
            !root.join("usr/share/only-mine.txt").exists(),
            "a path no other same-slot package owns is still deleted normally"
        );
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
    fn real_unmerge_of_an_in_place_upgraded_slot_mate_keeps_the_shared_file() {
        // Real, end-to-end `others_in_slot` proof: merge both
        // othersinslotpkg-1.0 and -2.0 (same SLOT, both writing the same
        // real shared path -- an in-place upgrade, real portage's own
        // "install new, then remove old" merge-list order), then unmerge
        // the *old* 1.0 vdb entry. The shared file must survive (2.0
        // still owns it); the 1.0-only file must not.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild_v1 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild");
        let ebuild_v2 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild");

        for ebuild in [&ebuild_v1, &ebuild_v2] {
            let merge_status = crate::ebuild_merge::run_merge(
                ebuild,
                &root,
                &portage_tmpdir,
                &crate::ebuild_merge::MergeOptions::default(),
            )
            .expect("run_merge succeeds");
            assert_eq!(merge_status, 0);
        }
        assert!(root.join("usr/share/othersinslotpkg/shared.txt").is_file());
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v1.txt")
            .is_file());
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v2.txt")
            .is_file());
        let vdb_v1 = root.join("var/db/pkg/dev-libs/othersinslotpkg-1.0");
        let vdb_v2 = root.join("var/db/pkg/dev-libs/othersinslotpkg-2.0");
        assert!(vdb_v1.is_dir());
        assert!(vdb_v2.is_dir());

        let unmerge_status = run_unmerge(
            &ebuild_v1,
            &root,
            &portage_tmpdir,
            false,
            ebuild_phases::ShellBackend::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(
            root.join("usr/share/othersinslotpkg/shared.txt").is_file(),
            "othersinslotpkg-2.0 still owns shared.txt -- unmerging 1.0 must not delete it"
        );
        assert!(
            !root
                .join("usr/share/othersinslotpkg/only-in-v1.txt")
                .exists(),
            "only-in-v1.txt has no other owner -- unmerging 1.0 must delete it normally"
        );
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v2.txt")
            .is_file());
        assert!(!vdb_v1.exists());
        assert!(vdb_v2.is_dir(), "the 2.0 vdb entry itself is untouched");

        // Now unmerge the remaining 2.0 entry too: with no other owner
        // left, the shared file finally goes.
        let unmerge_v2_status = run_unmerge(
            &ebuild_v2,
            &root,
            &portage_tmpdir,
            false,
            ebuild_phases::ShellBackend::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_v2_status, 0);
        assert!(!root.join("usr/share/othersinslotpkg/shared.txt").exists());
        assert!(!vdb_v2.exists());
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
