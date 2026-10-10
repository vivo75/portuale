//! Ebuild candidates for a package.

use crate::*;

/// Which kind of package this `Candidate` is -- Portage's `pkg.type_name`.
/// An ebuild's USE is computed dynamically; a binary's was baked at build
/// time (`binary_use`). `--pretend` prints `[ebuild` vs. `[binary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    Ebuild,
    Binary,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub version: String,
    pub keywords: Vec<String>,
    pub slot: String,
    /// The sub-slot half of `SLOT="main/sub"` (defaults to `slot`); folded into
    /// candidate strings so sub-slot atoms can match. Portage: PMS 8.3.3
    pub sub_slot: String,
    /// Which repo the ebuild/metadata lives in: re-reads DEPEND later and
    // breaks same-version cross-repo ties toward higher priority.
    pub repo_location: PathBuf,
    pub repo_priority: i32,
    /// The repo's `repos.conf` section name, appended as `::name` so `::repo`
    /// atoms filter (Portage also cross-checks `profiles/repo_name`).
    pub repo_name: String,
    /// Raw `LICENSE` (PMS 7.3.2), unreduced; empty when absent. Read with the
    /// same md5-cache dict as keywords/slot at zero extra I/O.
    pub license: String,
    /// Raw `IUSE`: consulted only when `license` holds a `?` conditional
    /// (Portage `use_reduce`'s `if '?' in license_str` optimization).
    pub iuse: String,
    /// Raw `PROPERTIES` / `RESTRICT`, same zero-extra-I/O reasoning.
    pub properties: String,
    pub restrict: String,
    /// Ebuild (md5-cache) or binary (`Packages` index entry).
    pub source: CandidateSource,
    /// The baked build-time USE (`Packages` `USE` field; presence = enabled).
    /// `None` for ebuilds, whose USE is computed (`effective_use_flags`).
    pub binary_use: Option<HashSet<String>>,
    /// From a remote binhost index, not yet in the local `$PKGDIR`
    /// (Portage `bintree.isremote`); flows to the `g` bracket column.
    pub remote: bool,
    /// The `BUILD_ID` / `BUILD_TIME` fields (`None` for ebuilds): display
    /// suffix and newest-instance ordering.
    pub build_id: Option<String>,
    pub build_time: Option<i64>,
    /// This binary instance's own recorded `*DEPEND` strings, so
    /// `--binpkg-changed-deps` compares the exact instance. Empty for ebuilds.
    pub binary_deps: HashMap<String, String>,
}

/// Accepts `<package>-<version>` only when the rest starts with a digit, so
/// "foo" never absorbs a sibling "foo-bar-2.0" directory.
pub(crate) fn strip_version_prefix<'a>(dir_name: &'a str, package: &str) -> Option<&'a str> {
    let rest = dir_name.strip_prefix(package)?.strip_prefix('-')?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Splits `package-version` into `(PN, PVR)` like Portage `_pkgsplit`: longest
/// trailing `-`-suffix that `ververify` accepts. Inverse of
/// [`strip_version_prefix`]; the single copy.
pub fn split_pf(pf: &str) -> Option<(String, String)> {
    let words: Vec<&str> = pf.split('-').collect();
    for i in 1..words.len() {
        let candidate = words[i..].join("-");
        // A `-r<digits>` revision belongs to the version -- but only when the
        // rest still verifies (Portage `_pkgsplit` has the same shape).
        if portage_versions::ververify(&candidate) {
            return Some((words[..i].join("-"), candidate));
        }
    }
    None
}

/// Every version of `category/package` with an ebuild in any of `repos`
/// (Portage `portdbapi.cp_list`), with md5-cache metadata. A missing or
/// unreadable cache entry is silently skipped; a repo without the directory
/// contributes nothing. Memoised per `(repo set, cp)` as a shared `Arc`
/// (ebuild trees are immutable for the life of a run).
pub fn list_candidates(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Result<std::sync::Arc<Vec<Candidate>>, Error> {
    use std::hash::{Hash, Hasher};
    type Cache = HashMap<(u64, String, String), std::sync::Arc<Vec<Candidate>>>;
    static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    let mut fp = std::collections::hash_map::DefaultHasher::new();
    for repo in repos {
        repo.location.hash(&mut fp);
        repo.priority.hash(&mut fp);
    }
    let key = (fp.finish(), category.to_string(), package.to_string());

    if let Ok(guard) = cache.read()
        && let Some(v) = guard.get(&key)
    {
        return Ok(std::sync::Arc::clone(v));
    }

    let result = std::sync::Arc::new(list_candidates_uncached(repos, category, package)?);
    if let Ok(mut guard) = cache.write() {
        guard.insert(key, std::sync::Arc::clone(&result));
    }
    Ok(result)
}

pub(crate) fn list_candidates_uncached(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Result<Vec<Candidate>, Error> {
    let mut candidates = Vec::new();
    for repo in repos {
        let pkg_dir = repo.location.join(category).join(package);
        if !pkg_dir.is_dir() {
            continue;
        }
        let entries = portage_util::read_dir_entries(&pkg_dir).map_err(|e| Error::ReadFile {
            path: pkg_dir.display().to_string(),
            source: e,
        })?;

        for entry in entries {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(stem) = file_name.strip_suffix(".ebuild") else {
                continue;
            };
            let Some(version) = strip_version_prefix(stem, package) else {
                continue;
            };
            let Ok(metadata) = repo_aux_metadata(&repo.location, category, stem) else {
                continue;
            };
            let keywords = metadata
                .get("KEYWORDS")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            let (slot, sub_slot) =
                split_slot(metadata.get("SLOT").map(String::as_str).unwrap_or(""));
            candidates.push(Candidate {
                version: version.to_string(),
                keywords,
                slot,
                sub_slot,
                repo_location: repo.location.clone(),
                repo_priority: repo.priority,
                repo_name: repo.name.clone(),
                license: metadata.get("LICENSE").cloned().unwrap_or_default(),
                iuse: metadata.get("IUSE").cloned().unwrap_or_default(),
                properties: metadata.get("PROPERTIES").cloned().unwrap_or_default(),
                restrict: metadata.get("RESTRICT").cloned().unwrap_or_default(),
                source: CandidateSource::Ebuild,
                binary_use: None,
                remote: false,
                build_id: None,
                build_time: None,
                binary_deps: HashMap::new(),
            });
        }
    }
    Ok(candidates)
}
