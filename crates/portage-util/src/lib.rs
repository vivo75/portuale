//! Filesystem helpers shared by the portage-* crates.
//!
//! `read_dir_entries` is the single directory-listing seam: sorted by file
//! name, or seeded-shuffled when `PORTUALE_SHUFFLE_DIRS` is set (test/CI
//! only, so the determinism test proves no readdir-order dependence).

use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};

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
