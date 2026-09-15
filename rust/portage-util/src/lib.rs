//! Filesystem helpers shared by the portage-* crates.
//!
//! `read_dir_entries` is the single directory-listing seam the whole
//! workspace reads through (`docs/second_python_copy_removal.md` §9's
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
}
