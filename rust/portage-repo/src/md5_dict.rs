//! The `md5-dict` aux-cache validator (real `portdbapi._pull_valid_cache`
//! -> `cache/template.py::validate_entry`), shared by `--regen`'s
//! repo-cache shortcut and the depcachedir rung of the depend-phase
//! fallback.
//!
//! Real walks three auxdb rungs for a cpv -- the repo's pregen
//! `metadata/md5-cache`, the writable depcachedir, then
//! `doebuild(mydo="depend")` -- and runs the same validator on the first
//! two. The formats differ in exactly one field: the pregen cache is
//! `flat_hash.md5_database` (`store_eclass_paths = False`), the writable
//! depcachedir is `mtime_md5_database` via `portdbapi.auxdbmodule`
//! (`config.py:524`), which writes `_eclasses_` as
//! `name\tpath\tmd5` triples (`cache/template.py:23`); real's
//! `validate_and_rewrite_cache` compares the checksum field and ignores
//! the path (`eclass_cache.py:150-170`). `store_eclass_paths` selects
//! which shape a caller's entry is in.

use md5::{Digest as _, Md5};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// Real `portage.eapi_is_supported` (`portage/__init__.py:391`): `EAPI`
/// in `0..=9` (`const.EAPI = 9`) plus the `_testing_eapis`
/// (`9-pre1`) and `_deprecated_eapis` (`3_pre1`/`3_pre2`/`4_pre1`/
/// `5_pre1`/`5_pre2`/`6_pre1`/`7_pre1`) sets. Anything else (including
/// an empty string, which the caller normalizes to `"0"` first, like
/// real `porttree.py:644-648`) means the cache entry is disregarded
/// and the `depend` phase runs.
pub fn eapi_is_supported(eapi: &str) -> bool {
    let eapi = eapi.trim();
    matches!(
        eapi,
        "0" | "1"
            | "2"
            | "3"
            | "4"
            | "5"
            | "6"
            | "7"
            | "8"
            | "9"
            | "9-pre1"
            | "3_pre1"
            | "3_pre2"
            | "4_pre1"
            | "5_pre1"
            | "5_pre2"
            | "6_pre1"
            | "7_pre1"
    )
}

/// Real `portdbapi._pull_valid_cache` (`porttree.py:603-658`) against a
/// single on-disk `md5-cache` entry: `true` when
/// `metadata/md5-cache/<category>/<pf>` validates, so the `depend` phase
/// can be skipped.
///
/// Validation mirrors real `cache/template.py::validate_entry`:
/// - the entry parses as `KEY=value` lines (a corrupt line invalidates,
///   real `flat_hash._parse_data` raising `CacheCorruption`);
/// - `_md5_` equals the md5 of the current ebuild file bytes;
/// - `EAPI` (defaulting to `"0"` when missing/empty, real
///   `porttree.py:644-648`) is supported -- unsupported-EAPI entries
///   are disregarded so the `depend` phase re-runs;
/// - every `_eclasses_` pair/triple still matches the eclass file that
///   wins across the `masters`-then-self chain (real
///   `eclass_cache.py::validate_and_rewrite_cache`; the last tree in
///   the chain holding the file wins, the same rule
///   `portuale::regen::eclasses_field` documents). A missing/empty
///   `_eclasses_` validates (nothing inherited, real's empty-dict
///   success); a missing eclass file, a differing md5, or a malformed
///   field length invalidates. `store_eclass_paths` selects the
///   `name\tpath\tmd5` triple form (the depcachedir) over the
///   `name\tmd5` pair form (the repo cache).
///
/// Any I/O failure (no cache file, no ebuild, no eclass dir) is
/// `false` -- the `depend` phase runs, exactly like real falling
/// through to `EbuildMetadataPhase` when no auxdb hits.
pub fn cache_entry_is_valid(
    ebuild_path: &Path,
    repo_location: &Path,
    masters: &[PathBuf],
    category: &str,
    pf: &str,
    store_eclass_paths: bool,
) -> bool {
    let cache_file = repo_location
        .join("metadata/md5-cache")
        .join(category)
        .join(pf);
    let text = match std::fs::read_to_string(&cache_file) {
        Ok(t) => t,
        Err(_) => return false,
    };
    entry_is_valid(
        &text,
        ebuild_path,
        repo_location,
        masters,
        store_eclass_paths,
    )
}

/// The path-independent half of [`cache_entry_is_valid`]: one entry's
/// bytes in real's `flat_hash` format, accepted or not against the
/// current ebuild. Shared by `--regen` (the repo's pregen
/// `metadata/md5-cache`, pairs), the depcachedir rung
/// (`ebuild_phases::depend_phase_metadata`, triples) and
/// `repo_aux_metadata`'s read-path validation.
pub fn entry_is_valid(
    text: &str,
    ebuild_path: &Path,
    repo_location: &Path,
    masters: &[PathBuf],
    store_eclass_paths: bool,
) -> bool {
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            return false;
        };
        fields.insert(k, v);
    }
    let ebuild_bytes = match std::fs::read(ebuild_path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let ebuild_md5 = format!("{:x}", Md5::digest(&ebuild_bytes));
    if fields.get("_md5_").is_none_or(|v| *v != ebuild_md5) {
        return false;
    }
    let eapi = fields.get("EAPI").map(|s| s.trim()).unwrap_or("");
    let eapi = if eapi.is_empty() { "0" } else { eapi };
    if !eapi_is_supported(eapi) {
        return false;
    }
    let eclasses_raw = fields.get("_eclasses_").map(|s| s.trim()).unwrap_or("");
    if eclasses_raw.is_empty() {
        return true;
    }
    let parts: Vec<&str> = eclasses_raw.split('\t').collect();
    let width = if store_eclass_paths { 3 } else { 2 };
    if !parts.len().is_multiple_of(width) {
        return false;
    }
    let porttrees: Vec<&Path> = masters
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(repo_location))
        .collect();
    for pair in parts.chunks(width) {
        // Triple form: `name\tpath\tmd5`; pair form: `name\tmd5`. Real's
        // `validate_and_rewrite_cache` compares the checksum entry only.
        let (name, want_md5) = if store_eclass_paths {
            (pair[0], pair[2])
        } else {
            (pair[0], pair[1])
        };
        // `md5` values are 32 hex digits (real `_md5_deserializer`
        // rejects anything else as corruption).
        if want_md5.len() != 32 || !want_md5.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        let Some(got_md5) = porttrees
            .iter()
            .rev()
            .find_map(|tree| eclass_md5(&tree.join("eclass").join(format!("{name}.eclass"))))
        else {
            return false;
        };
        if got_md5 != want_md5 {
            return false;
        }
    }
    true
}

/// The md5 of one eclass file, memoised per path and invalidated when
/// the file's mtime/size change -- real `eclass_cache.hashed_path`'s own
/// mtime check. The memo only matters when one process reads the same
/// eclass many times (the resolver's per-candidate validation), which is
/// exactly real's hot path.
fn eclass_md5(path: &Path) -> Option<String> {
    struct Cached {
        mtime: Option<std::time::SystemTime>,
        size: u64,
        md5: Option<String>,
    }
    type Memo = HashMap<PathBuf, Cached>;
    static MEMO: OnceLock<RwLock<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| RwLock::new(HashMap::new()));

    let (mtime, size) = match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => (None, 0),
    };
    if let Ok(guard) = memo.read()
        && let Some(hit) = guard.get(path)
        && hit.mtime == mtime
        && hit.size == size
    {
        return hit.md5.clone();
    }
    let md5 = std::fs::read(path)
        .ok()
        .map(|bytes| format!("{:x}", Md5::digest(&bytes)));
    if let Ok(mut guard) = memo.write() {
        guard.insert(
            path.to_path_buf(),
            Cached {
                mtime,
                size,
                md5: md5.clone(),
            },
        );
    }
    md5
}

/// The `masters`-then-self chain a validator needs for `_eclasses_`,
/// memoised per repo location. A repo the resolved config does not
/// describe (or an unreadable config) yields an empty chain:
/// `_eclasses_` then fails validation, so the depend phase re-runs --
/// the safe direction.
pub fn repo_masters_for_location(repo_location: &Path) -> Vec<PathBuf> {
    type Memo = HashMap<PathBuf, Vec<PathBuf>>;
    static MEMO: OnceLock<RwLock<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| RwLock::new(HashMap::new()));

    if let Ok(guard) = memo.read()
        && let Some(masters) = guard.get(repo_location)
    {
        return masters.clone();
    }
    let masters = crate::find_repos(&crate::config_root_from_env())
        .ok()
        .and_then(|repos| {
            repos
                .into_iter()
                .find(|r| r.location == repo_location)
                .map(|r| r.masters)
        })
        .unwrap_or_default();
    if let Ok(mut guard) = memo.write() {
        guard.insert(repo_location.to_path_buf(), masters.clone());
    }
    masters
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    struct Layout {
        dir: PathBuf,
    }

    impl Layout {
        fn new() -> Self {
            let n = SEQ.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "portage-repo-md5-dict-{}-{}",
                std::process::id(),
                n
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn path(&self) -> &Path {
            &self.dir
        }
    }

    impl Drop for Layout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn write_repo(
        dir: &Layout,
        ebuild_text: &str,
        eclass_text: Option<&str>,
        cache_text: Option<&str>,
    ) -> (PathBuf, PathBuf, Vec<PathBuf>) {
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("dev-libs/pkg")).unwrap();
        std::fs::create_dir_all(repo.join("eclass")).unwrap();
        let ebuild_path = repo.join("dev-libs/pkg/pkg-1.0.ebuild");
        std::fs::write(&ebuild_path, ebuild_text).unwrap();
        if let Some(ec) = eclass_text {
            std::fs::write(repo.join("eclass/myclass.eclass"), ec).unwrap();
        }
        if let Some(cache) = cache_text {
            let cache_dir = repo.join("metadata/md5-cache/dev-libs");
            std::fs::create_dir_all(&cache_dir).unwrap();
            let mut f = std::fs::File::create(cache_dir.join("pkg-1.0")).unwrap();
            f.write_all(cache.as_bytes()).unwrap();
        }
        (ebuild_path, repo, Vec::new())
    }

    fn md5_hex(bytes: &[u8]) -> String {
        format!("{:x}", Md5::digest(bytes))
    }

    fn valid_repo(
        ebuild: &str,
        eclass: Option<&str>,
        cache: Option<&str>,
        store_paths: bool,
    ) -> bool {
        let dir = Layout::new();
        let (ebuild_path, repo, masters) = write_repo(&dir, ebuild, eclass, cache);
        cache_entry_is_valid(
            &ebuild_path,
            &repo,
            &masters,
            "dev-libs",
            "pkg-1.0",
            store_paths,
        )
    }

    #[test]
    fn valid_entry_without_eclasses_is_valid() {
        let ebuild = "EAPI=8\nDESCRIPTION=x\n";
        assert!(valid_repo(
            ebuild,
            None,
            Some(&format!(
                "EAPI=8\nSLOT=0\n_md5_={}\n",
                md5_hex(ebuild.as_bytes())
            )),
            false,
        ));
    }

    #[test]
    fn stale_ebuild_md5_invalidates() {
        assert!(!valid_repo(
            "EAPI=8\nDESCRIPTION=new\n",
            None,
            Some("EAPI=8\nSLOT=0\n_md5_=00000000000000000000000000000000\n"),
            false,
        ));
    }

    #[test]
    fn missing_cache_file_is_invalid() {
        assert!(!valid_repo("EAPI=8\n", None, None, false));
    }

    #[test]
    fn corrupt_line_and_unsupported_eapi_invalidate() {
        let ebuild = "EAPI=8\n";
        let md5 = md5_hex(ebuild.as_bytes());
        // No "=" on one line: real `flat_hash._parse_data` corruption.
        assert!(!valid_repo(
            ebuild,
            None,
            Some(&format!("EAPI=8\nNOEQUALS\n_md5_={md5}\n")),
            false,
        ));
        // Unsupported EAPI is disregarded (real `porttree.py:648-653`).
        assert!(!valid_repo(
            ebuild,
            None,
            Some(&format!("EAPI=99\n_md5_={md5}\n")),
            false,
        ));
    }

    #[test]
    fn pair_eclass_format() {
        let ebuild = "EAPI=8\n";
        let ebuild_md5 = md5_hex(ebuild.as_bytes());
        let eclass_md5 = md5_hex(b"# eclass\n");
        let good = format!("EAPI=8\n_eclasses_=myclass\t{eclass_md5}\n_md5_={ebuild_md5}\n");
        assert!(valid_repo(ebuild, Some("# eclass\n"), Some(&good), false));
        // Changed eclass content.
        assert!(!valid_repo(ebuild, Some("# changed\n"), Some(&good), false));
        // Missing eclass file.
        assert!(!valid_repo(ebuild, None, Some(&good), false));
        // Odd-count `_eclasses_` (malformed).
        assert!(!valid_repo(
            ebuild,
            Some("# eclass\n"),
            Some(&format!("EAPI=8\n_eclasses_=myclass\n_md5_={ebuild_md5}\n")),
            false,
        ));
        // The pair form is not a triple form.
        assert!(!valid_repo(ebuild, Some("# eclass\n"), Some(&good), true));
    }

    /// S0 cell (a): the writable depcachedir is `mtime_md5_database`
    /// (`store_eclass_paths = True`), so its `_eclasses_` is
    /// `name\tpath\tmd5`; the path is ignored for validation (real
    /// `validate_and_rewrite_cache`'s `itemgetter(1)` on the
    /// `(dir, checksum)` tuple).
    #[test]
    fn triple_eclass_format() {
        let ebuild = "EAPI=8\n";
        let ebuild_md5 = md5_hex(ebuild.as_bytes());
        let eclass_md5 = md5_hex(b"# eclass\n");
        let good = format!(
            "EAPI=8\n_eclasses_=myclass\t/some/where/eclass\t{eclass_md5}\n_md5_={ebuild_md5}\n"
        );
        assert!(valid_repo(ebuild, Some("# eclass\n"), Some(&good), true));
        // A wrong path element does not matter, only the checksum.
        let wrong_path = format!(
            "EAPI=8\n_eclasses_=myclass\t/elsewhere/eclass\t{eclass_md5}\n_md5_={ebuild_md5}\n"
        );
        assert!(valid_repo(
            ebuild,
            Some("# eclass\n"),
            Some(&wrong_path),
            true
        ));
        // A stale checksum invalidates.
        let stale = format!(
            "EAPI=8\n_eclasses_=myclass\t/some/where/eclass\t{}\n_md5_={ebuild_md5}\n",
            md5_hex(b"# old\n")
        );
        assert!(!valid_repo(ebuild, Some("# eclass\n"), Some(&stale), true));
        // The triple form is not a pair form.
        assert!(!valid_repo(ebuild, Some("# eclass\n"), Some(&good), false));
    }

    /// A changed eclass must be re-hashed, not served from the memo
    /// (real `eclass_cache.hashed_path`'s mtime check).
    #[test]
    fn eclass_change_after_a_validation_is_seen() {
        let dir = Layout::new();
        let ebuild = "EAPI=8\n";
        let ebuild_md5 = md5_hex(ebuild.as_bytes());
        let first = md5_hex(b"# eclass\n");
        let cache = format!("EAPI=8\n_eclasses_=myclass\t{first}\n_md5_={ebuild_md5}\n");
        let (ebuild_path, repo, masters) =
            write_repo(&dir, ebuild, Some("# eclass\n"), Some(&cache));
        assert!(cache_entry_is_valid(
            &ebuild_path,
            &repo,
            &masters,
            "dev-libs",
            "pkg-1.0",
            false
        ));
        // Content changes (size and mtime change) -> invalid.
        std::fs::write(repo.join("eclass/myclass.eclass"), "# changed!\n").unwrap();
        assert!(!cache_entry_is_valid(
            &ebuild_path,
            &repo,
            &masters,
            "dev-libs",
            "pkg-1.0",
            false
        ));
    }

    #[test]
    fn eapi_support_matches_real() {
        for eapi in ["0", "5", "8", "9", "9-pre1", "5_pre1", " 8 "] {
            assert!(eapi_is_supported(eapi), "{eapi}");
        }
        for eapi in ["", "99", "10", "8-pre1"] {
            assert!(!eapi_is_supported(eapi), "{eapi:?}");
        }
    }
}
