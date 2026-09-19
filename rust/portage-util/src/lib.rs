//! Filesystem helpers shared by the portage-* crates.
//!
//! `read_dir_entries` is the single directory-listing seam the whole
//! workspace reads through (`docs/history/second_python_copy_removal.md` §9's
//! second half, backlog #51): sorted by file name by default, and
//! deliberately shuffled when `PORTUALE_SHUFFLE_DIRS` is set, so the
//! determinism test can prove no consumer depends on readdir order.
//! The shuffle is test/CI-only, seeded, and reproducible per
//! (seed, directory); production never sets the variable.

use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};

/// The seed from `PORTUALE_SHUFFLE_DIRS` (test/CI-only), if set.
pub fn shuffle_seed() -> Option<u64> {
    std::env::var("PORTUALE_SHUFFLE_DIRS")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Every entry of `dir`, sorted by file name; shuffled (Fisher-Yates,
/// seeded by `PORTUALE_SHUFFLE_DIRS` and the directory path) when that
/// test variable is set.
pub fn read_dir_entries(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    if let Some(seed) = shuffle_seed() {
        let mut state = seed ^ fnv1a(dir.to_string_lossy().as_bytes());
        for i in (1..entries.len()).rev() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            entries.swap(i, (state >> 33) as usize % (i + 1));
        }
    }
    Ok(entries)
}

/// `read_dir_entries` reduced to the entry paths.
pub fn read_dir_paths(dir: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(read_dir_entries(dir)?
        .into_iter()
        .map(|entry| entry.path())
        .collect())
}

/// Portage's `VCS_DIRS` (`lib/portage/const.py:278`): directory names
/// `_recursive_file_list` never descends into.
pub const VCS_DIRS: &[&str] = &["CVS", "RCS", "SCCS", ".bzr", ".git", ".hg", ".svn"];

/// Real `_recursive_basename_filter` (`lib/portage/util/__init__.py`):
/// dot-prefixed names and `~`-suffixed backups are never config.
pub fn recursive_basename_allowed(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with('~')
}

/// Real `_recursive_file_list` (`lib/portage/util/__init__.py`) as a file
/// list: `path` may be a regular file or a directory. Directories are
/// walked to arbitrary depth, depth-first, with one ascending sort per
/// directory over files and subdirectories together and each subdirectory
/// expanded in place (stack DFS pushing children in reverse). A directory
/// whose own basename is a `VCS_DIRS` entry or fails
/// `recursive_basename_allowed` is skipped entirely, **including the root
/// path itself**; a regular file is yielded when its basename passes the
/// filter (VCS names are only a directory rule in real). `os.stat` is
/// followed, so symlinked files and directories are both traversed and a
/// dangling/missing path yields nothing rather than erroring. Each
/// directory level is listed through `read_dir_entries`, so the
/// `PORTUALE_SHUFFLE_DIRS` test hook keeps working through this path.
pub fn recursive_config_files(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let meta = match std::fs::metadata(&current) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            let dominated = match current.file_name().and_then(|s| s.to_str()) {
                // `Path::file_name` is `None` for `/`: real splits that to
                // `""`, which passes both filters, so the root is walked.
                None => false,
                Some(name) => VCS_DIRS.contains(&name) || !recursive_basename_allowed(name),
            };
            if dominated {
                continue;
            }
            let entries = match read_dir_entries(&current) {
                Ok(e) => e,
                Err(_) => continue,
            };
            // Reverse: the stack pops from the end, so this restores the
            // ascending per-directory order with directories expanded in
            // place alongside files.
            for entry in entries.into_iter().rev() {
                stack.push(entry.path());
            }
        } else if meta.is_file() {
            let allowed = match current.file_name().and_then(|s| s.to_str()) {
                None => true,
                Some(name) => recursive_basename_allowed(name),
            };
            if allowed {
                out.push(current);
            }
        }
        // Sockets, fifos, devices: real checks `S_ISDIR` then `S_ISREG`
        // and ignores everything else.
    }
    Ok(out)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        read_dir_entries(dir)
            .unwrap()
            .into_iter()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    // One test for the whole seam: cargo runs tests in parallel and the
    // shuffle is process-global, so a second env-mutating test could
    // race the default-order assertion.
    #[test]
    fn read_dir_is_sorted_by_default_and_seed_shuffled_in_test_mode() {
        let dir = std::env::temp_dir().join(format!("portage-util-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["g", "a", "h", "c", "b", "f", "d", "e"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let sorted = ["a", "b", "c", "d", "e", "f", "g", "h"];
        assert_eq!(names(&dir), sorted);

        // SAFETY: this is the only test touching the variable, and the
        // harness holds it for the whole test body.
        unsafe { std::env::set_var("PORTUALE_SHUFFLE_DIRS", "7") };
        let first = names(&dir);
        let second = names(&dir);
        unsafe { std::env::set_var("PORTUALE_SHUFFLE_DIRS", "8") };
        let other_seed = names(&dir);
        unsafe { std::env::remove_var("PORTUALE_SHUFFLE_DIRS") };

        assert_eq!(first, second, "same seed must reproduce the same order");
        assert_ne!(first, sorted, "an 8-entry shuffle must not stay sorted");
        assert_ne!(
            first, other_seed,
            "different seeds must shuffle differently"
        );
        assert_eq!(names(&dir), sorted, "unset must go back to sorted order");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn rels(root: &Path) -> Vec<String> {
        recursive_config_files(root)
            .unwrap()
            .into_iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect()
    }

    // Backlog #89 S0: mirrors real `_recursive_file_list` on the staged
    // oracle tree (nested fragments, backups, dotfiles, VCS dirs at every
    // level, interleaved file/dir sort, symlinked file and directory).
    #[test]
    fn recursive_config_files_matches_reals_traversal_and_filters() {
        let root =
            std::env::temp_dir().join(format!("portage-util-recursive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("aa")).unwrap();
        std::fs::create_dir_all(root.join("zz")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("CVS")).unwrap();
        std::fs::create_dir_all(root.join(".hiddendir")).unwrap();
        for (rel, body) in [
            ("00-first", "x"),
            ("aa/inner", "x"),
            ("nn-mid", "x"),
            ("zz/inner", "x"),
            ("zz-last", "x"),
            ("zz-flat~", "backup"),
            (".hidden-frag", "dot"),
            (".git/frag", "vcs"),
            ("CVS/frag", "cvs"),
            (".hiddendir/frag", "hiddendir"),
        ] {
            std::fs::write(root.join(rel), body).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("00-first"), root.join("link-frag")).unwrap();
            symlink("aa", root.join("linkdir")).unwrap();
        }

        #[cfg(unix)]
        let expected = vec![
            "00-first",
            "aa/inner",
            "link-frag",
            "linkdir/inner",
            "nn-mid",
            "zz/inner",
            "zz-last",
        ];
        #[cfg(not(unix))]
        let expected = vec!["00-first", "aa/inner", "nn-mid", "zz/inner", "zz-last"];
        assert_eq!(rels(&root), expected);

        // A single file path yields itself; a filtered name yields nothing.
        assert_eq!(
            recursive_config_files(&root.join("00-first")).unwrap(),
            vec![root.join("00-first")]
        );
        assert!(
            recursive_config_files(&root.join(".hidden-frag"))
                .unwrap()
                .is_empty()
        );
        // The root path itself is filtered too: a `~`-suffixed directory
        // yields nothing, like real.
        let tilted = root.join("tilted~");
        std::fs::create_dir_all(&tilted).unwrap();
        std::fs::write(tilted.join("frag"), "x").unwrap();
        assert!(recursive_config_files(&tilted).unwrap().is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
