//! Filesystem helpers shared by the portage-* crates.
//!
//! `read_dir_entries` is the single directory-listing seam: sorted by file
//! name, or seeded-shuffled when `PORTUALE_SHUFFLE_DIRS` is set (test/CI
//! only, so the determinism test proves no readdir-order dependence).

use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};

pub mod profile;
mod temp_dir;
mod varexpand;
pub use temp_dir::{TEMP_DIR_PREFIX, TempDir};
pub use varexpand::varexpand;

/// The seed from `PORTUALE_SHUFFLE_DIRS` (test/CI-only), if set. Read
/// once at first use and frozen for the life of the process.
pub fn shuffle_seed() -> Option<u64> {
    use std::sync::OnceLock;
    static SEED: OnceLock<Option<u64>> = OnceLock::new();
    *SEED.get_or_init(|| {
        std::env::var("PORTUALE_SHUFFLE_DIRS")
            .ok()?
            .trim()
            .parse()
            .ok()
    })
}

/// Lowercase hex of `bytes`, the single spelling every crate uses for a
/// digest value (the hash crates' output no longer formats with `{:x}`).
pub fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every entry of `dir`, sorted by file name (shuffled when the test
/// variable is set).
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

pub fn read_dir_paths(dir: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(read_dir_entries(dir)?
        .into_iter()
        .map(|entry| entry.path())
        .collect())
}

/// The `"-MERGING-"` prefix of a not-yet-finalized vdb entry directory.
// Portage: lib/portage/const.py, MERGING_IDENTIFIER
pub const MERGING_IDENTIFIER: &str = "-MERGING-";

/// Whether a vdb directory name is an in-progress merge entry Portage
/// readers skip.
pub fn is_merging_vdb_entry(name: &str) -> bool {
    name.starts_with(MERGING_IDENTIFIER)
}

/// Directory names Portage's `_recursive_file_list` never descends into.
// Portage: lib/portage/const.py, VCS_DIRS
pub const VCS_DIRS: &[&str] = &["CVS", "RCS", "SCCS", ".bzr", ".git", ".hg", ".svn"];

/// Dot-prefixed names and `~`-suffixed backups are never config.
// Portage: lib/portage/util/__init__.py, _recursive_basename_filter
pub fn recursive_basename_allowed(name: &str) -> bool {
    !name.starts_with('.') && !name.ends_with('~')
}

/// Portage's `_recursive_file_list` as a file list: depth-first with one
/// ascending sort per directory, `VCS_DIRS`/dotfile/`~`-filtered
/// (**including the root itself**). Symlinks are traversed; a
/// dangling/missing path yields nothing. Listed via `read_dir_entries`.
// Portage: lib/portage/util/__init__.py, _recursive_file_list
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
                // `/` has no file name; it passes both filters, so it is walked.
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
            // Reversed: the stack pops from the end.
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
        // Sockets, fifos, devices are ignored.
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

    // One test for the whole seam (the shuffle is process-global); the
    // seeded half lives in `tests/shuffle_seed_freeze.rs`.
    #[test]
    fn shuffle_seed_is_frozen_at_first_use_and_read_dir_is_sorted_by_default() {
        let dir = TempDir::new("portage-util").keep();
        for name in ["g", "a", "h", "c", "b", "f", "d", "e"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let first = shuffle_seed();
        // SAFETY: a value set after the first read is ignored.
        unsafe { std::env::set_var("PORTUALE_SHUFFLE_DIRS", "7") };
        let second = shuffle_seed();
        unsafe { std::env::remove_var("PORTUALE_SHUFFLE_DIRS") };

        assert_eq!(first, second, "the first use pins the value");
        assert_eq!(first, None, "the harness never sets the variable");
        let sorted = ["a", "b", "c", "d", "e", "f", "g", "h"];
        assert_eq!(names(&dir), sorted, "unset must be sorted");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn rels(root: &Path) -> Vec<String> {
        recursive_config_files(root)
            .unwrap()
            .into_iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect()
    }

    // Expected value from Portage's `_recursive_file_list` (staged oracle tree).
    #[test]
    fn recursive_config_files_matches_portages_traversal_and_filters() {
        let root = TempDir::new("portage-util-recursive").keep();
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

        assert_eq!(
            recursive_config_files(&root.join("00-first")).unwrap(),
            vec![root.join("00-first")]
        );
        assert!(
            recursive_config_files(&root.join(".hidden-frag"))
                .unwrap()
                .is_empty()
        );
        // A `~`-suffixed root directory also yields nothing.
        let tilted = root.join("tilted~");
        std::fs::create_dir_all(&tilted).unwrap();
        std::fs::write(tilted.join("frag"), "x").unwrap();
        assert!(recursive_config_files(&tilted).unwrap().is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
