//! Ebuild candidates for a package.

use crate::*;

/// Which kind of package this `Candidate` actually is -- real portage's
/// own `pkg.type_name` (`lib/_emerge/RootConfig.py`'s own
/// `pkg_tree_map`, exactly the two strings `"ebuild"`/`"binary"` used
/// here): an ebuild candidate's own USE gets computed dynamically (see
/// `effective_use_flags`), while a binary candidate's USE was already
/// baked in at build time (`Candidate::binary_use`) -- real `--pretend`
/// also prints a different bracket word for each (`"[ebuild"` vs.
/// `"[binary"`), see `GraphEntry::source`.
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
    /// The sub-slot half of this candidate's own `SLOT` metadata (real
    /// `SLOT="main/sub"`), read via `split_slot` alongside `slot` --
    /// defaults to `slot` itself when the ebuild's own `SLOT` carries no
    /// `/` at all, matching real `_pkg_str`'s own slot-parsing fallback.
    /// Folded into every candidate string this crate builds for
    /// `portage_dep::match_from_list` as a `slot/sub_slot` suffix (see
    /// `portage_dep::Candidate`'s own regex, which already parses this
    /// shape) -- closes a real, previously-silent gap: any dependency
    /// atom restricted on sub-slot (`dev-libs/foo:0/2`, PMS 8.3.3) could
    /// never actually match anything here before, since every candidate
    /// string omitted the sub-slot half entirely, no matter what a
    /// candidate's real `SLOT` metadata said.
    pub sub_slot: String,
    /// Which repo this candidate's ebuild/metadata actually lives in --
    /// needed once there's more than one (see `list_candidates`), both to
    /// re-read this exact package's own DEPEND/RDEPEND later
    /// (`resolve_pretend_graph`) and to break a same-version tie between
    /// two repos toward the higher-priority one (`resolve_pretend`).
    pub repo_location: PathBuf,
    pub repo_priority: i32,
    /// The repo's own `repos.conf` section name (`RepoConfig::name`),
    /// e.g. "gentoo" -- appended as a `::name` suffix to every candidate
    /// string this crate builds for `portage_dep::match_from_list`
    /// (except the two paths noted in the module doc comment's
    /// `::reponame` bullet), so a `::repo`-constrained atom actually
    /// filters. Real portage cross-checks this against each repo's own
    /// `profiles/repo_name` file too; portuale reuses the already-read
    /// `repos.conf` name as-is rather than reading a second file.
    pub repo_name: String,
    /// The raw `LICENSE` metadata string (PMS 7.3.2), unreduced -- read
    /// alongside `keywords`/`slot` at zero extra I/O cost (the same
    /// md5-cache metadata dict `list_candidates` already reads for
    /// them). Empty string for no `LICENSE` at all, matching real
    /// `use_reduce("")`'s own empty result -- see `is_visible`'s own
    /// license-masking check.
    pub license: String,
    /// The raw `IUSE` metadata string, same "already have it, zero
    /// extra I/O" reasoning as `license` -- only ever consulted when
    /// `license` actually contains a `?` USE-conditional, to resolve
    /// this specific candidate's own effective USE (real `use_reduce`'s
    /// own "if '?' in license_str" optimization, ported as-is: most
    /// packages' LICENSE has no conditional at all, so most candidates
    /// never need this field for anything).
    pub iuse: String,
    /// The raw `PROPERTIES` metadata string, same "already have it,
    /// zero extra I/O" reasoning as `license`/`iuse` -- see
    /// `properties_accepted`'s own doc comment.
    pub properties: String,
    /// The raw `RESTRICT` metadata string. See `properties`'s own doc
    /// comment.
    pub restrict: String,
    /// Ebuild (a repo's own `metadata/md5-cache`) or binary (a
    /// `PKGDIR/Packages` index entry) -- see `CandidateSource`'s own
    /// doc comment.
    pub source: CandidateSource,
    /// The USE flags this specific build was compiled with, already
    /// fully resolved (real `Packages` index's own `USE` field -- bare
    /// flag names, presence means enabled, no `+`/`-` markers needed
    /// since nothing is "still deciding" the way a fresh ebuild build's
    /// own IUSE defaults are) -- `None` for an ebuild candidate, whose
    /// USE gets computed dynamically instead (`effective_use_flags`).
    /// `list_binary_candidates`'s own only consumer:
    /// `resolve_pretend`'s own `--binpkg-respect-use` check compares
    /// this against what would currently be selected, over this
    /// candidate's own `iuse` flags.
    pub binary_use: Option<HashSet<String>>,
    /// A `--getbinpkg` binary candidate from a *remote* binhost's own
    /// `Packages` index (`list_remote_binary_candidates`), not yet
    /// present in the local `$PKGDIR` -- real `bintree.isremote`. Flows
    /// to `GraphEntry::remote_binary` -> the real `g` bracket column
    /// (`PkgAttrDisplay.remote_binary`, `output_helpers.py:627-633`).
    /// Always `false` for an ebuild candidate or a local `$PKGDIR`
    /// binary.
    pub remote: bool,
    /// The `BUILD_ID` field from this binary's own `Packages` index entry
    /// (real binpkg-multi-instance / binhost `_pkg_str.build_id`) --
    /// `Some("4")` for a binary, `None` for an ebuild or a `Packages`
    /// entry with no `BUILD_ID`. Real `output.py::_append_build_id`
    /// appends `-{build_id}` to the displayed version of a `type_name ==
    /// "binary"` package (before the `:slot` / `::repo` decoration).
    pub build_id: Option<String>,
    /// The `BUILD_TIME` field from this binary's own `Packages` entry
    /// (real `_pkg_str.build_time`). Real `dbapi._cmp_cpv` sorts
    /// same-version binary instances by `build_time`, and
    /// `_iter_match_pkgs` reverses that + `break`s on the first
    /// acceptable one -- so among several builds of one cpv the highest
    /// `BUILD_TIME` (that passes `--binpkg-respect-use` etc.) wins
    /// (`dedup_binary_instances`). `None` for an ebuild or a `Packages`
    /// entry with no `BUILD_TIME`.
    pub build_time: Option<i64>,
    /// This binary instance's own recorded `*DEPEND` strings (its
    /// `Packages` entry's `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/`IDEPEND`
    /// -- absent keys omitted), kept so `--binpkg-changed-deps` compares
    /// the *exact* instance's deps rather than re-looking-up the cpv (a
    /// binpkg-multi-instance cpv has one dep set per build). Empty for an
    /// ebuild candidate.
    pub binary_deps: HashMap<String, String>,
}

/// A directory entry's name is only accepted as `<package>-<version>` if
/// what follows the "<package>-" prefix looks like a version (starts with
/// a digit) -- otherwise a package like "foo" would wrongly absorb a
/// sibling package's directory like "foo-bar-2.0" as if it were "foo"
/// version "bar-2.0". Same disambiguation principle as the PMS package-name
/// grammar ported in portage-dep's atom parser.
pub(crate) fn strip_version_prefix<'a>(dir_name: &'a str, package: &str) -> Option<&'a str> {
    let rest = dir_name.strip_prefix(package)?.strip_prefix('-')?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Split `package-version` (`PF` without category) into `(PN, PVR)` the
/// way real `_pkgsplit` does: the version is the longest trailing
/// `-`-separated suffix that `ververify` accepts, so a package name may
/// itself contain digit-led words (`foo-1bar-2.0` -> `foo-1bar`).
/// Needed by the md5-cache validator to find an entry's ebuild
/// (`<repo>/<cat>/<pn>/<pf>.ebuild`); the inverse of
/// [`strip_version_prefix`]. Since #127 this is the single copy: the
/// former `portuale::remote_bundle::split_pf` and this crate's private
/// `split_installed_dir` were deleted for it.
pub fn split_pf(pf: &str) -> Option<(String, String)> {
    let words: Vec<&str> = pf.split('-').collect();
    for i in 1..words.len() {
        let candidate = words[i..].join("-");
        // A `-r<digits>` revision belongs to the version, not the name --
        // but only when the rest still verifies (else `foo-r1-2.0` would
        // mis-split; real `_pkgsplit` has the same shape).
        if portage_versions::ververify(&candidate) {
            return Some((words[..i].join("-"), candidate));
        }
    }
    None
}

/// Lists every version of `category/package` that has an ebuild in ANY of
/// `repos`, with metadata (KEYWORDS, SLOT) from each repo's own md5-cache
/// -- mirroring real `portdbapi.cp_list`, which gathers candidates from
/// every configured repo the same way, not just the first one that has
/// the package. `repos` is iterated in the order given (see
/// `find_repos`'s ascending `(priority, name)` sort), and each resulting
/// `Candidate` remembers which repo it came from. A candidate whose cache
/// entry is missing or unreadable is silently skipped (v1 doesn't
/// distinguish "stale cache" from "doesn't exist" -- both just mean "not
/// visible"); a repo with no directory at all for this category/package
/// simply contributes nothing, same as before.
/// Every version of `category/package` with an ebuild in any of `repos`,
/// memoised per `(repo set, cp)` and returned as a shared `Arc`. The
/// resolver calls this per cp per graph-walk visit -- visibility,
/// keyword, slot-operator, changed-deps and parent-scan checks all
/// re-list the same `cp` -- and each build is a `read_dir` plus a
/// `Candidate` (a dozen owned strings) per version. Ebuild trees are
/// immutable for the life of any portuale run (nothing here ever writes a
/// `.ebuild`; `--regen` only touches `metadata/md5-cache`, in a separate
/// process), so a plain per-process cache is sound -- same reasoning as
/// [`read_md5_cache`]. Keyed by a hash of the repo locations + `cp`.
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
