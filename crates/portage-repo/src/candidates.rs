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

#[derive(Debug, Clone, PartialEq)]
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
    /// The package's `EAPI` (`repo_aux_metadata` for ebuilds, the `Packages`
    /// `EAPI` field for binaries): which grammar `*DEPEND` validation parses
    /// with. Portage: `Package.eapi` (`_validate_deps`' `dep_eapi`).
    pub eapi: String,
    /// This binary instance's own recorded `*DEPEND` strings, so
    /// `--binpkg-changed-deps` compares the exact instance. Empty for ebuilds.
    pub binary_deps: HashMap<String, String>,
    /// This binary instance's recorded `USER_PATCHES` digest (`""` when
    /// the index entry carries no key: the `-r1` empty default, bug
    /// 984255). Compared against the local patch-set digest by the
    /// `--binpkg-respect-user-patches` filter (backlog #509); ebuilds
    /// carry `""` (never filtered). Lowercase hex, like Portage.
    // Portage: Package.py, `user_patches`
    pub user_patches: String,
}

impl Candidate {
    /// The string an atom is matched against: `cat/pkg-ver:slot/sub::repo`.
    /// The only place that formats it from a `Candidate`; `match_str_lint`
    /// rejects hand-built copies, which once dropped the sub-slot.
    /// Portage: `match_from_list` is handed a `Package`'s cpv with slot,
    /// sub-slot and repo attributes (`dep/__init__.py`, `match_from_list`).
    #[must_use]
    pub fn match_str(&self, category: &str, package: &str) -> String {
        // Exhaustive, no `..`: a new field must be classified here.
        let Candidate {
            version,          // read: match_str
            keywords: _,      // field-ok: keyword visibility, not part of the atom-match string
            slot,             // read: match_str
            sub_slot,         // read: match_str
            repo_location: _, // field-ok: a path; the match string carries the repo name
            repo_priority: _, // field-ok: tie-break order, not matched
            repo_name,        // read: match_str
            license: _,       // field-ok: license masking is not an atom property
            iuse: _,          // field-ok: USE deps are checked apart from the string
            properties: _,    // field-ok: not an atom property
            restrict: _,      // field-ok: not an atom property
            source: _,        // field-ok: ebuild vs binary is not matched by atoms
            binary_use: _,    // field-ok: USE deps are checked apart from the string
            remote: _,        // field-ok: display flag
            build_id: _,      // field-ok: a `-<build_id>` atom is matched by the caller
            build_time: _,    // field-ok: ordering only
            eapi: _,          // field-ok: grammar selection, not matched
            binary_deps: _,   // field-ok: --binpkg-changed-deps data, not matched
            user_patches: _,  // field-ok: --binpkg-respect-user-patches data, not matched
        } = self;
        format!("{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}")
    }
}

/// The head EAPI of one ebuild file when Portage would mask by it: `Some`
/// for a non-empty, unsupported head value (`99`, `7_alpha`), `None`
/// otherwise (supported, empty, or unreadable — those stay on the
/// metadata path). An empty head (`EAPI=`) is a broken ebuild Portage
/// drops outright, never an EAPI mask.
/// Portage: _parse_eapi_ebuild_head, eapi_is_supported
pub(crate) fn unsupported_head_eapi(ebuild_path: &Path) -> Option<String> {
    let text = fs::read_to_string(ebuild_path).ok()?;
    let head = portage_dep::eapi::parse_eapi_head(&text);
    (!head.is_empty() && !portage_dep::eapi::eapi_is_supported(&head)).then_some(head)
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
///
/// This is the RAW discovery read: it does not know the category
/// allow-list. Resolution paths use [`CategoryGate::repo_candidates`]
/// instead; addressed metadata reads (an installed package's own ebuild,
/// a resolved entry's display) keep this one, like Portage's `aux_get`,
/// which reads the cache file by path without consulting `cp_list`.
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
                // No usable metadata: Portage still lists an unsupported-EAPI
                // ebuild as a masked candidate (its `getmaskingstatus` EAPI
                // reason needs only the head value); anything else stays
                // invisible, as before. The head EAPI decides without
                // running the depend phase.
                // Portage: getmaskingstatus.py, porttree.py, _pull_valid_cache
                if let Some(eapi) = unsupported_head_eapi(&pkg_dir.join(format!("{stem}.ebuild"))) {
                    candidates.push(Candidate {
                        version: version.to_string(),
                        // Portage calls EAPI-masked metadata "mostly useless"
                        // (no SLOT read): only version, repo and EAPI matter.
                        // Portage: depgraph.py, show_masked_packages
                        keywords: Vec::new(),
                        slot: "0".to_string(),
                        sub_slot: "0".to_string(),
                        repo_location: repo.location.clone(),
                        repo_priority: repo.priority,
                        repo_name: repo.name.clone(),
                        license: String::new(),
                        iuse: String::new(),
                        properties: String::new(),
                        restrict: String::new(),
                        source: CandidateSource::Ebuild,
                        binary_use: None,
                        remote: false,
                        build_id: None,
                        build_time: None,
                        eapi,
                        user_patches: String::new(),
                        binary_deps: HashMap::new(),
                    });
                }
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
                user_patches: String::new(),
                // Every cache entry carries `EAPI` (egencache writes it);
                // without one the ebuild predates the format: EAPI 0.
                eapi: metadata
                    .get("EAPI")
                    .cloned()
                    .unwrap_or_else(|| "0".to_string()),
                binary_deps: HashMap::new(),
            });
        }
    }
    Ok(candidates)
}

/// Portage's `settings.categories` gate for repo ebuild discovery: the
/// allow-listed half of `portdbapi.cp_list` (`porttree.py:1156,1216-1225`).
/// Built once per config (never re-read per lookup); `Copy` so resolve
/// helpers and [`SelectSource`](crate::resolve::pretend::SelectSource)
/// take it by value.
#[derive(Clone, Copy)]
pub struct CategoryGate<'a> {
    /// The repos whose ebuilds are discovered.
    pub repos: &'a [RepoConfig],
    /// The resolved allow-list (repo files stacked, then the user file).
    pub categories: &'a BTreeSet<String>,
    /// The config root the unlisted-category `!!!` line's path renders from.
    pub config_root: &'a Path,
    /// `(category, package)` pairs the `!!!` already fired for: Portage's
    /// `cp_list` cache prints once per cp per process; one walk prints
    /// once per cp per walk.
    pub warned: &'a RefCell<HashSet<(String, String)>>,
}

impl CategoryGate<'_> {
    /// The repo ebuilds for `category/package`, or nothing when the
    /// category is unlisted (printing Portage's `!!! ... not listed ...`
    /// line once per cp when ebuilds exist on disk; a plain miss with no
    /// notice otherwise).
    // Portage: portdbapi.cp_list, porttree.py:1156,1216-1225.
    pub fn repo_candidates(
        &self,
        category: &str,
        package: &str,
    ) -> Result<std::sync::Arc<Vec<Candidate>>, Error> {
        if self.categories.contains(category) {
            return list_candidates(self.repos, category, package);
        }
        let has_ebuilds = list_candidates(self.repos, category, package)
            .map(|c| !c.is_empty())
            .unwrap_or(false);
        if has_ebuilds
            && self
                .warned
                .borrow_mut()
                .insert((category.to_string(), package.to_string()))
        {
            eprintln!(
                "\n!!! '{category}/{package}' has a category that is not listed in {}/etc/portage/categories",
                self.config_root.display()
            );
        }
        Ok(std::sync::Arc::new(Vec::new()))
    }

    /// Silent half of the `cp_exists` check displays use: whether any repo
    /// carries an ebuild for `category/package` *within the allow-list*.
    /// Never prints: the walk's own lookup already fired the `!!!` for a
    /// cp the miss report names, and a display-only re-print would double
    /// it (each display phase builds its own gate).
    pub fn has_ebuilds(&self, category: &str, package: &str) -> bool {
        self.categories.contains(category)
            && list_candidates(self.repos, category, package)
                .map(|c| !c.is_empty())
                .unwrap_or(false)
    }
}
