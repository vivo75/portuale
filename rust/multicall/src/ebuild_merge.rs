// Real merge/filesystem mutation (task #55, `PORTING/PROMPT-next.md`'s own
// "Real merge/install/filesystem mutation" section) -- the first slice:
// after running the real `install` phase chain (task #54's own
// `ebuild_phases` module), really copy `${D}`'s own regular files,
// directories, and symlinks into `${ROOT}`, and write a real vdb entry
// (`CONTENTS`, in the exact `obj`/`dir`/`sym` line format real
// `dblink._format_contents_line` uses, plus `CATEGORY`/`SLOT`/
// `repository`) -- mirroring real `dblink.merge()`/`treewalk()`/
// `mergeme()` (`lib/portage/dbapi/vartree.py`, ~6500 lines total) at a
// deliberately narrow v1 scope, the same "narrow v1, document the cut"
// pattern `ebuild_phases`'s own module doc comment already established.
//
// KNOWN, DOCUMENTED GAPS (v1 scope):
//   - No `pkg_preinst`/`pkg_postinst` hook execution around the merge --
//     real `treewalk()` runs `pkg_preinst` before copying anything and
//     `pkg_postinst` after; this slice does neither. `ebuild_phases`
//     could run them as ordinary phases already, but wiring that in is a
//     separate, natural follow-on, not bundled into this slice.
//   - No `CONFIG_PROTECT`/collision-protect/preserve-libs handling at
//     all -- real `mergeme()`'s own config-file-protection branch
//     (renaming a changed `/etc` file to `._cfg0000_...` instead of
//     overwriting it) and `FEATURES=collision-protect`/`preserve-libs`
//     are both real, separately-scoped features this slice doesn't
//     attempt.
//   - No `COUNTER`/`env_update()`/`ldconfig` triggering, and no atomic
//     `dbtmpdir`-then-rename vdb write -- real `merge()` builds the new
//     vdb entry in a temporary `.dblink-tmp-<pf>` directory and
//     atomically moves it into place only once everything succeeded,
//     specifically so a crash mid-merge can't corrupt a pre-existing vdb
//     entry; this slice writes directly into the final vdb directory
//     instead, a real (if lower-fidelity) risk this pilot accepts for
//     now.
//   - Real `os.chown`/permission-preserving `os.chmod` per merged file
//     are not reproduced explicitly -- `std::fs::copy` already preserves
//     a regular file's permission bits on Unix, which covers the common
//     case; ownership is left as whatever the copying process's own
//     default is (this pilot's own single-user dev/test context has no
//     privilege-dropping concept anywhere else either).
//   - Directory-entry merge order is sorted by filename for determinism
//     (this pilot's own test-reproducibility need) rather than real
//     `os.listdir()`'s own arbitrary/OS-dependent order -- `CONTENTS`
//     line order has no real semantic meaning portage itself relies on.
//   - `SLOT` is read directly from the ebuild's own text via a simple
//     `SLOT=...` assignment regex (see `parse_slot`), the same
//     "real-file, direct-text-parsing" shortcut `ebuild_phases::
//     parse_eapi` already takes for `EAPI` -- a `SLOT` computed by real
//     bash logic rather than declared as a literal is out of scope.
//   - `repository` is resolved by walking up from the ebuild's own
//     package directory looking for a `profiles/repo_name` file (real
//     portage's own mechanism for naming a repo), defaulting to the same
//     `"__unknown__"` sentinel `portage_repo::new_repo_changed` already
//     uses when no such file is found at all.

use crate::ebuild_phases;
use md5::{Digest, Md5};
use std::path::{Path, PathBuf};

/// Whether `command` is the one real merge command this module implements
/// -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_real_phase_command` before routing to real execution.
pub fn is_real_merge_command(command: &str) -> bool {
    command == "merge"
}

/// Real PMS: unlike `EAPI` (restricted to the ebuild's own first real
/// line), `SLOT` may appear anywhere among an ebuild's own top-level
/// variable assignments -- this scans every line for the first literal
/// `SLOT=...` match. No match at all defaults to `"0"` (the same default
/// `portage_repo::installed_candidates`/`split_slot` already use for a
/// missing `SLOT`).
fn parse_slot(ebuild_text: &str) -> String {
    let slot_re = regex::Regex::new(r#"^[ \t]*SLOT=(?:"([^"]*)"|'([^']*)'|(\S*))[ \t]*(#.*)?$"#)
        .expect("static regex is valid");
    for line in ebuild_text.lines() {
        if let Some(caps) = slot_re.captures(line) {
            let value = caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            return value.to_string();
        }
    }
    "0".to_string()
}

/// Real portage's own mechanism for naming a repo (`layout.conf`'s
/// `repo-name` key aside, the canonical source is a repo's own
/// `profiles/repo_name` file, first line): walks up from `pkg_dir`
/// (`<category>/<package>`) through every ancestor, returning the first
/// `profiles/repo_name` found. `None` when no ancestor has one at all
/// (e.g. a standalone ebuild file outside any repo checkout).
fn repository_name_for(pkg_dir: &Path) -> Option<String> {
    for ancestor in pkg_dir.ancestors() {
        let candidate = ancestor.join("profiles").join("repo_name");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let name = text.lines().next().unwrap_or("").trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn md5_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Md5::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Real `dblink._format_contents_line`: `<type> <path>[ <md5>| -> <target>][ <mtime>]\n`.
fn format_contents_line(
    node_type: &str,
    abs_path: &str,
    md5_digest: Option<&str>,
    symlink_target: Option<&str>,
    mtime_secs: Option<i64>,
) -> String {
    let mut fields = vec![node_type.to_string(), abs_path.to_string()];
    if let Some(md5) = md5_digest {
        fields.push(md5.to_string());
    } else if let Some(target) = symlink_target {
        fields.push(format!("-> {target}"));
    }
    if let Some(mtime) = mtime_secs {
        fields.push(mtime.to_string());
    }
    format!("{}\n", fields.join(" "))
}

fn mtime_secs(metadata: &std::fs::Metadata) -> Result<i64, String> {
    use std::time::UNIX_EPOCH;
    let mtime = metadata
        .modified()
        .map_err(|e| format!("reading mtime: {e}"))?;
    Ok(mtime
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("mtime before epoch: {e}"))?
        .as_secs() as i64)
}

/// Walks `d` (real `${D}`) and merges every entry into `root` (real
/// `${ROOT}`), returning the accumulated real `CONTENTS` text (see this
/// module's own doc comment for the exact line format and the v1 scope
/// cuts -- no config-protect, no chown, sorted-by-name traversal order).
fn merge_tree(d: &Path, root: &Path) -> Result<String, String> {
    let mut contents = String::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = d.join(&relative_dir);
        let mut children: Vec<PathBuf> = std::fs::read_dir(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| relative_dir.join(e.file_name()))
            .collect();
        children.sort();

        for relative_path in children {
            let src = d.join(&relative_path);
            let dest = root.join(&relative_path);
            let abs_path = format!("/{}", relative_path.display());
            let file_type = std::fs::symlink_metadata(&src)
                .map_err(|e| format!("{}: {e}", src.display()))?
                .file_type();

            if file_type.is_symlink() {
                let target =
                    std::fs::read_link(&src).map_err(|e| format!("{}: {e}", src.display()))?;
                if dest.exists() || dest.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&dest);
                }
                std::os::unix::fs::symlink(&target, &dest)
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
                let mtime = mtime_secs(
                    &std::fs::symlink_metadata(&src)
                        .map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                contents.push_str(&format_contents_line(
                    "sym",
                    &abs_path,
                    None,
                    Some(&target.to_string_lossy()),
                    Some(mtime),
                ));
            } else if file_type.is_dir() {
                std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                contents.push_str(&format_contents_line("dir", &abs_path, None, None, None));
                stack.push(relative_path);
            } else if file_type.is_file() {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::copy(&src, &dest).map_err(|e| format!("{}: {e}", src.display()))?;
                let digest = md5_hex(&src)?;
                let mtime = mtime_secs(
                    &std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                contents.push_str(&format_contents_line(
                    "obj",
                    &abs_path,
                    Some(&digest),
                    None,
                    Some(mtime),
                ));
            }
            // fifo/device nodes: real mergeme() handles these too, but no
            // fixture this pilot has needs them -- out of scope for now.
        }
    }
    Ok(contents)
}

/// Writes a minimal real vdb entry under `root` for the package described
/// by `env` -- `CATEGORY`/`SLOT`/`repository`/`CONTENTS`, matching the
/// same one-value-per-file convention this pilot's own fixtures (and
/// `portage_repo`'s own vdb readers) already use. Not atomic (see this
/// module's own doc comment).
fn write_vdb_entry(
    root: &Path,
    env: &ebuild_phases::Environment,
    slot: &str,
    repository: &str,
    contents: &str,
) -> Result<(), String> {
    let vdb_dir = root
        .join("var/db/pkg")
        .join(&env.category)
        .join(&env.split.pf);
    std::fs::create_dir_all(&vdb_dir).map_err(|e| format!("{}: {e}", vdb_dir.display()))?;
    for (name, value) in [
        ("CATEGORY", env.category.as_str()),
        ("SLOT", slot),
        ("repository", repository),
    ] {
        std::fs::write(vdb_dir.join(name), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", vdb_dir.join(name).display()))?;
    }
    std::fs::write(vdb_dir.join("CONTENTS"), contents)
        .map_err(|e| format!("{}: {e}", vdb_dir.join("CONTENTS").display()))?;
    Ok(())
}

/// Real `merge()`'s own first step is always the real `install` phase
/// chain having already completed (`actionmap_deps["merge"] ==
/// ["install"]`) -- run here directly rather than requiring the caller
/// to have run it first, exactly like `ebuild_phases::run_commands`
/// itself already chains `install`'s own prerequisites automatically.
/// Real `PORTAGE_BUILDDIR`-relative resume markers make re-running an
/// already-done `install` chain cheap, so this is safe to call even when
/// the caller's own command list already ran `install` immediately
/// before `merge` (see `ebuild.rs`'s own dispatch loop).
pub fn run_merge(ebuild_path: &Path, root: &Path, portage_tmpdir: &Path) -> Result<i32, String> {
    let status = ebuild_phases::run_commands(ebuild_path, &["install"], root, portage_tmpdir)?;
    if status != 0 {
        return Ok(status);
    }

    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let ebuild_text = std::fs::read_to_string(&env.ebuild_abs)
        .map_err(|e| format!("{}: {e}", env.ebuild_abs.display()))?;
    let slot = parse_slot(&ebuild_text);
    let repository = repository_name_for(&env.pkg_dir).unwrap_or_else(|| "__unknown__".to_string());

    let contents = merge_tree(&env.d(), root)?;
    write_vdb_entry(root, &env, &slot, &repository, &contents)?;

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_real_merge_command_covers_exactly_merge() {
        assert!(is_real_merge_command("merge"));
        assert!(!is_real_merge_command("qmerge"));
        assert!(!is_real_merge_command("unmerge"));
        assert!(!is_real_merge_command("install"));
    }

    #[test]
    fn parse_slot_reads_a_literal_assignment_anywhere_in_the_file() {
        assert_eq!(parse_slot("EAPI=8\nSLOT=\"0\"\n"), "0");
        assert_eq!(parse_slot("SLOT=\"2/5\"\n"), "2/5");
        assert_eq!(parse_slot("SLOT='1'\n"), "1");
        assert_eq!(parse_slot("SLOT=0\n"), "0");
    }

    #[test]
    fn parse_slot_defaults_to_0_when_missing() {
        assert_eq!(parse_slot("EAPI=8\nDESCRIPTION=x\n"), "0");
        assert_eq!(parse_slot(""), "0");
    }

    #[test]
    fn format_contents_line_matches_real_dblink_format() {
        assert_eq!(
            format_contents_line("dir", "/usr/share/x", None, None, None),
            "dir /usr/share/x\n"
        );
        assert_eq!(
            format_contents_line("obj", "/usr/share/x/f", Some("abc123"), None, Some(100)),
            "obj /usr/share/x/f abc123 100\n"
        );
        assert_eq!(
            format_contents_line("sym", "/usr/lib/x.so", None, Some("x.so.1"), Some(100)),
            "sym /usr/lib/x.so -> x.so.1 100\n"
        );
    }

    #[test]
    fn repository_name_for_finds_the_nearest_ancestor_repo_name() {
        let tmp = tempdir();
        let repo = tmp.join("myrepo");
        let pkg_dir = repo.join("dev-libs/foo");
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
        assert_eq!(repository_name_for(&pkg_dir), Some("myrepo".to_string()));
    }

    #[test]
    fn repository_name_for_is_none_when_no_ancestor_has_one() {
        let tmp = tempdir();
        let pkg_dir = tmp.join("dev-libs/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        assert_eq!(repository_name_for(&pkg_dir), None);
    }

    #[test]
    fn merge_tree_copies_files_dirs_and_symlinks_and_writes_matching_contents() {
        let tmp = tempdir();
        let d = tmp.join("D");
        let root = tmp.join("ROOT");
        std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
        std::fs::write(d.join("usr/share/x/hello.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("hello.txt", d.join("usr/share/x/link.txt")).unwrap();
        std::fs::create_dir_all(&root).unwrap();

        let contents = merge_tree(&d, &root).expect("merge_tree succeeds");

        assert!(root.join("usr/share/x/hello.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/x/hello.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_link(root.join("usr/share/x/link.txt")).unwrap(),
            PathBuf::from("hello.txt")
        );

        assert!(contents.contains("dir /usr\n"));
        assert!(contents.contains("dir /usr/share\n"));
        assert!(contents.contains("dir /usr/share/x\n"));
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/x/hello.txt ")));
        assert!(contents.contains("sym /usr/share/x/link.txt -> hello.txt"));
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "multicall-ebuild-merge-test-{}-{}",
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
    fn real_merge_lands_files_and_a_symlink_and_writes_a_real_vdb_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let status = run_merge(&ebuild, &root, &portage_tmpdir).expect("run_merge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/mergepkg/hello.txt"))
                .unwrap()
                .trim(),
            "hello from mergepkg"
        );
        let link = root.join("usr/share/mergepkg/hello-link.txt");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("hello.txt")
        );

        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("CATEGORY"))
                .unwrap()
                .trim(),
            "dev-libs"
        );
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("SLOT"))
                .unwrap()
                .trim(),
            "0"
        );
        assert_eq!(
            std::fs::read_to_string(vdb_dir.join("repository"))
                .unwrap()
                .trim(),
            "testrepo"
        );
        let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
        assert!(contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt ")));
        assert!(contents.contains("sym /usr/share/mergepkg/hello-link.txt -> hello.txt"));
    }
}
