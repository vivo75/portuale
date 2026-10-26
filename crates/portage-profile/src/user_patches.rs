//! Local user-patch sets (`/etc/portage/patches`).
//!
//! Portage's `UserPatches` (`lib/portage/package/ebuild/_config/UserPatches.py`)
//! and the shell `eapply_user` writer (`bin/phase-helpers.sh`) compute the
//! same digest: per-file sha256 hex of the `.diff`/`.patch` files matching
//! the package (`cat/{PN,P,PR}[:slot]`), basenames sorted `LC_ALL=C`, empty
//! files skipped (the shell's `-s` test / negation), sha256 over the
//! concatenated hex. Stray files outside `cat/pkg` dirs are ignored, never
//! a crash.

use std::collections::HashMap;
use std::path::Path;

/// sha256 of the empty file: entries hashing to it are skipped when the
/// per-package digest is combined (but the directory still counts as a
/// patch set, and the file still lists under [`UserPatchSet::files`]).
// Portage: UserPatches.py, `_empty_hash`
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Suffixes `eapply_user` applies (shell `*.@(diff|patch)`, case-sensitive).
const PATCH_SUFFIXES: [&str; 2] = [".diff", ".patch"];

/// The loaded `<config_root>/etc/portage/patches` tree: directory
/// `"cat/name"` -> sorted `(basename bytes, content sha256 hex)` rows.
/// `None` (from [`UserPatchSet::load`]) when there is no patches directory.
#[derive(Debug, Clone, Default)]
pub struct UserPatchSet {
    dirs: HashMap<String, Vec<(Vec<u8>, String)>>,
}

impl UserPatchSet {
    /// Load the patch sets under `<config_root>/etc/portage/patches`.
    /// `None` when the path is missing or not a directory (a `patches`
    /// *file* means no patch sets).
    #[must_use]
    pub fn load(config_root: &Path) -> Option<Self> {
        let base = config_root.join("etc/portage/patches");
        if !base.is_dir() {
            return None;
        }
        let mut dirs = HashMap::new();
        let Ok(categories) = std::fs::read_dir(&base) else {
            return Some(Self { dirs });
        };
        for category in categories.flatten() {
            if !category.path().is_dir() {
                continue;
            }
            let Ok(sets) = std::fs::read_dir(category.path()) else {
                continue;
            };
            for set in sets.flatten() {
                let location = set.path();
                if !location.is_dir() {
                    continue;
                }
                let mut rows: Vec<(Vec<u8>, String)> = Vec::new();
                let Ok(files) = std::fs::read_dir(&location) else {
                    continue;
                };
                for file in files.flatten() {
                    let name = file.file_name();
                    let Some(name) = name.to_str() else {
                        continue;
                    };
                    if !PATCH_SUFFIXES.iter().any(|s| name.ends_with(s)) {
                        continue;
                    }
                    let Ok(content) = std::fs::read(file.path()) else {
                        continue;
                    };
                    rows.push((name.as_bytes().to_vec(), sha256_hex(&content)));
                }
                if !rows.is_empty() {
                    rows.sort_by(|a, b| a.0.cmp(&b.0));
                    // The key mirrors the `cat/pkg` relative path Portage
                    // queries with (`key.cp`, `key.cpv`, ...).
                    let key = format!(
                        "{}/{}",
                        category.file_name().to_string_lossy(),
                        set.file_name().to_string_lossy()
                    );
                    dirs.insert(key, rows);
                }
            }
        }
        Some(Self { dirs })
    }

    /// The four directory keys Portage queries for one package, least to
    /// most specific (later ones override same basenames).
    /// Portage: UserPatches.py, `_query`
    fn keys(category: &str, package: &str, version: &str, slot: &str) -> [String; 4] {
        let cp = format!("{category}/{package}");
        let cpv = format!("{cp}-{version}");
        // The shell strips the sub-slot (`${SLOT%/*}`); the Python key
        // carries the package's slot the same way.
        let slot = slot.split('/').next().unwrap_or(slot);
        [
            cp.clone(),
            cpv.clone(),
            format!("{cp}:{slot}"),
            format!("{cpv}:{slot}"),
        ]
    }

    /// Whether any patch set matches the package at all (any of the four
    /// directory keys present and non-empty, even if every file in it is
    /// empty): Portage's `pkg in settings._user_patches`
    /// (`UserPatches.__contains__`), the first half of the merge-time
    /// mismatch gate (`Binpkg.py`, `_unpack_metadata`). An all-empty set
    /// still counts, while [`UserPatchSet::digest`] is `""` for it.
    /// Portage: UserPatches.py, `__contains__`
    #[must_use]
    pub fn has(&self, category: &str, package: &str, version: &str, slot: &str) -> bool {
        Self::keys(category, package, version, slot)
            .iter()
            .any(|key| self.dirs.contains_key(key))
    }

    /// Merged `(basename, hash)` rows for the package: the four keyed
    /// directories overlaid least-to-most specific.
    fn merged(
        &self,
        category: &str,
        package: &str,
        version: &str,
        slot: &str,
    ) -> Vec<(Vec<u8>, String)> {
        let mut merged: HashMap<&[u8], &str> = HashMap::new();
        let mut present = false;
        for key in Self::keys(category, package, version, slot) {
            if let Some(rows) = self.dirs.get(&key) {
                present = true;
                for (base, hash) in rows {
                    merged.insert(base, hash);
                }
            }
        }
        if !present {
            return Vec::new();
        }
        let mut out: Vec<(Vec<u8>, String)> = merged
            .into_iter()
            .map(|(base, hash)| (base.to_vec(), hash.to_string()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The combined digest Portage compares against the binary's recorded
    /// `USER_PATCHES` (`""` when no patch set matches, or every matching
    /// file is empty). Lowercase hex, like both writers.
    /// Portage: UserPatches.py, `digest`/`__getitem__`
    #[must_use]
    pub fn digest(&self, category: &str, package: &str, version: &str, slot: &str) -> String {
        let merged = self.merged(category, package, version, slot);
        if merged.is_empty() {
            return String::new();
        }
        let combined: String = merged
            .iter()
            .filter(|(_, hash)| hash != EMPTY_SHA256)
            .map(|(_, hash)| hash.clone())
            .collect();
        if combined.is_empty() {
            return String::new();
        }
        sha256_hex(combined.as_bytes())
    }

    /// The matching patch files as `cat/dir/file` relative paths (the
    /// union across the four keys, including empty ones): what the
    /// `--verbose` ignored-binaries block lists.
    /// Portage: UserPatches.py, `patches` (`userPatchFiles`)
    #[must_use]
    pub fn files(&self, category: &str, package: &str, version: &str, slot: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for key in Self::keys(category, package, version, slot) {
            if let Some(rows) = self.dirs.get(&key) {
                for (base, _) in rows {
                    out.push(format!("{key}/{}", String::from_utf8_lossy(base)));
                }
            }
        }
        out.sort();
        out
    }
}

fn sha256_hex(content: &[u8]) -> String {
    use sha2::Digest as _;
    portage_util::hex_lower(&sha2::Sha256::digest(content))
}
