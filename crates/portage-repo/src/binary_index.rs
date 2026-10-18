//! The binary package index (`Packages`) and binary candidates.

use crate::*;

/// Parses `<pkgdir>/Packages` into one `HashMap` per package entry, skipping
/// the first (global header) block. Trusts the index outright (Portage's
/// default `FEATURES="pkgdir-index-trusted"`); revalidation lives one layer
/// up. A missing file is an empty list.
pub fn read_packages_index(pkgdir: &Path) -> Vec<HashMap<String, String>> {
    let path = pkgdir.join("Packages");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_packages_index(&text).1
}

/// Splits index text into its global header block and one `HashMap` per
/// package entry. [`read_packages_index`] drops the header.
pub fn parse_packages_index(text: &str) -> (HashMap<String, String>, Vec<HashMap<String, String>>) {
    let mut blocks: Vec<HashMap<String, String>> = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(": ") {
            current.insert(key.to_string(), value.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    // The first block is the global header, never a package entry.
    if blocks.is_empty() {
        (HashMap::new(), Vec::new())
    } else {
        let header = blocks.remove(0);
        (header, blocks)
    }
}

/// A parsed binary-package index: `Packages` blocks, or scan-synthesized
/// entries when there is no `Packages`. The read-vs-scan decision is made
/// once, at the CLI boundary.
#[derive(Debug, Clone, Default)]
pub struct BinaryIndex {
    pub(crate) entries: Vec<HashMap<String, String>>,
    /// `(category, package)` -> indices into `entries`, built once: a remote
    /// index has ~18 000 entries, so a linear scan per lookup was quadratic.
    pub(crate) by_cp: HashMap<(String, String), Vec<usize>>,
}

impl PartialEq for BinaryIndex {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for BinaryIndex {}

impl BinaryIndex {
    /// Parses `<pkgdir>/Packages`; a missing file is an empty index.
    pub fn from_pkgdir(pkgdir: &Path) -> Self {
        Self::from_entries(read_packages_index(pkgdir))
    }

    /// Builds from caller-assembled entries (a `$PKGDIR` scan of each
    /// `.tbz2`/`.gpkg.tar`'s embedded metadata as `Packages`-style records).
    pub fn from_entries(entries: Vec<HashMap<String, String>>) -> Self {
        let mut by_cp: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            if let Some((cat, pkg, _)) = e.get("CPV").and_then(|cpv| split_cpv(cpv)) {
                by_cp.entry((cat, pkg)).or_default().push(i);
            }
        }
        Self { entries, by_cp }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn entries(&self) -> &[HashMap<String, String>] {
        &self.entries
    }

    /// The index records for `category/package` -- O(matches), not a scan.
    pub(crate) fn entries_for_cp(
        &self,
        category: &str,
        package: &str,
    ) -> impl Iterator<Item = &HashMap<String, String>> {
        self.by_cp
            .get(&(category.to_string(), package.to_string()))
            .into_iter()
            .flatten()
            .map(move |&i| &self.entries[i])
    }
}

/// `BinaryIndex::from_pkgdir`, memoised by directory: a ~20 MB index re-parsed
/// per atom made big `--getbinpkg` graphs take minutes. A live refresh installs
/// a per-directory override here *before* resolution (`Some` = use it, `None`
/// = the binrepo contributes nothing); the override wins over memo and disk.
pub(crate) fn cached_binary_index(pkgdir: &Path) -> std::sync::Arc<BinaryIndex> {
    if let Some(overridden) = remote_binary_index_override(pkgdir) {
        return overridden.unwrap_or_default();
    }
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, std::sync::Arc<BinaryIndex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(hit) = cache.read().unwrap().get(pkgdir) {
        return hit.clone();
    }
    let idx = std::sync::Arc::new(BinaryIndex::from_pkgdir(pkgdir));
    cache
        .write()
        .unwrap()
        .insert(pkgdir.to_path_buf(), idx.clone());
    idx
}

/// The live-refresh override table for [`cached_binary_index`]. Cleared where a
/// run starts; unit-test temp dirs never collide.
pub(crate) static REMOTE_INDEX_OVERRIDE: OnceLock<
    RwLock<HashMap<PathBuf, Option<std::sync::Arc<BinaryIndex>>>>,
> = OnceLock::new();

/// Installs (or replaces) the in-memory index for `pkgdir` (Portage `bintree`'s
/// fetched-then-maybe-cached `pkgindex`): `Some` resolves from it, `None`
/// resolves nothing (a failed non-`--pretend` refresh drops even a stale cache).
pub fn set_remote_binary_index_override(pkgdir: &Path, index: Option<std::sync::Arc<BinaryIndex>>) {
    REMOTE_INDEX_OVERRIDE
        .get_or_init(|| RwLock::new(HashMap::new()))
        .write()
        .unwrap()
        .insert(pkgdir.to_path_buf(), index);
}

/// Drops every live-refresh override. Called where a run starts (review M4),
/// so a second in-process run resolves from its own refresh or the disk.
pub fn clear_remote_binary_index_overrides() {
    if let Some(table) = REMOTE_INDEX_OVERRIDE.get() {
        table.write().unwrap().clear();
    }
}

pub(crate) fn remote_binary_index_override(
    pkgdir: &Path,
) -> Option<Option<std::sync::Arc<BinaryIndex>>> {
    REMOTE_INDEX_OVERRIDE
        .get()
        .and_then(|table| table.read().unwrap().get(pkgdir).cloned())
}

/// The local `$PKGDIR` binary index for this run: the CLI layer's directory
/// scan when `<pkgdir>/Packages` was absent, else the parsed `Packages` file.
///
/// Built, not memoised (the old per-inputs memo keyed on the
/// scanned `Vec`'s raw pointer, never re-read once `None` was cached, and
/// ignored the quickpkg-direct vdb). Built once per resolution and threaded
/// to every consumer as an `Arc`.
pub fn build_local_binpkg_index(config: &portage_profile::Config) -> std::sync::Arc<BinaryIndex> {
    let mut entries = match &config.scanned_binpkgs {
        Some(entries) => entries.clone(),
        None => read_packages_index(Path::new(&config.pkgdir)),
    };
    // `--quickpkg-direct` (Portage actions + `bintree.
    // _populate_additional`): installed packages of the quickpkg source root
    // join the binary pool as `Packages`-style records. A CPV the local
    // `$PKGDIR` already carries wins. Set once by the CLI layer.
    if let Some(source_root) = quickpkg_direct_root() {
        let have: HashSet<String> = entries
            .iter()
            .filter_map(|e| e.get("CPV").cloned())
            .collect();
        for e in quickpkg_direct_index_entries(&source_root) {
            if !e.get("CPV").is_some_and(|cpv| have.contains(cpv)) {
                entries.push(e);
            }
        }
    }
    std::sync::Arc::new(BinaryIndex::from_entries(entries))
}

/// `--quickpkg-direct-root` (Portage actions): the root whose
/// installed packages `--quickpkg-direct` offers as binary candidates.
/// `None` = not active. A process-global so it needn't thread through
/// resolution signatures.
pub(crate) static QUICKPKG_DIRECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set by `pretend.rs` from `--quickpkg-direct[-root]` before any resolution.
pub fn set_quickpkg_direct_root(source_root: Option<PathBuf>) {
    *QUICKPKG_DIRECT_ROOT.write().unwrap() = source_root;
}

pub(crate) fn quickpkg_direct_root() -> Option<PathBuf> {
    QUICKPKG_DIRECT_ROOT.read().unwrap().clone()
}

/// Every package under `<source_root>/var/db/pkg` as a `Packages`-style record
/// (Portage `bintree._populate_additional` reads the vdb the same way). Keys
/// mirror what `binary_candidates_from_index` / `read_binary_metadata` read.
pub(crate) fn quickpkg_direct_index_entries(source_root: &Path) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    for pkg in all_installed_packages(source_root) {
        let (cat, name, ver) = (
            pkg.category.as_str(),
            pkg.package.as_str(),
            pkg.version.as_str(),
        );
        let read = |key: &str| read_vdb_string(source_root, cat, name, ver, key);
        let raw_slot = read("SLOT");
        let slot = if raw_slot.is_empty() {
            "0".to_string()
        } else {
            raw_slot
        };
        let mut entry: HashMap<String, String> = HashMap::new();
        entry.insert("CPV".to_string(), format!("{cat}/{name}-{ver}"));
        entry.insert("SLOT".to_string(), slot);
        for key in [
            "KEYWORDS",
            "USE",
            "IUSE",
            "LICENSE",
            "PROPERTIES",
            "RESTRICT",
            "DEPEND",
            "RDEPEND",
            "BDEPEND",
            "PDEPEND",
            "IDEPEND",
        ] {
            let v = read(key);
            if !v.is_empty() {
                entry.insert(key.to_string(), v);
            }
        }
        let repo = read("repository");
        if !repo.is_empty() {
            entry.insert("REPO".to_string(), repo);
        }
        out.push(entry);
    }
    out
}

/// Binary-package builds of `category/package` -- Portage's `bindbapi`, added
/// to the ebuild pool only when `--usepkg`/`--usepkgonly` makes them eligible.
/// Matched off `CPV` like an ebuild filename; repo fields stay pseudo values.
/// Portage: depgraph.py `_dynamic_depgraph_config`
pub fn list_binary_candidates(
    index: &BinaryIndex,
    category: &str,
    package: &str,
) -> Vec<Candidate> {
    binary_candidates_from_index(index, category, package, false)
}

/// Shared body of `list_binary_candidates` and `list_remote_binary_candidates`.
/// `repo_name` is the entry's `REPO`, else `"__unknown__"`.
pub(crate) fn binary_candidates_from_index(
    index: &BinaryIndex,
    category: &str,
    package: &str,
    remote: bool,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for entry in index.entries_for_cp(category, package) {
        let Some(cpv) = entry.get("CPV") else {
            continue;
        };
        let Some(pf) = cpv.strip_prefix(&format!("{category}/")) else {
            continue;
        };
        let Some(version) = strip_version_prefix(pf, package) else {
            continue;
        };
        let keywords = entry
            .get("KEYWORDS")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let (slot, sub_slot) = split_slot(entry.get("SLOT").map(String::as_str).unwrap_or("0"));
        let binary_use: HashSet<String> = entry
            .get("USE")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        candidates.push(Candidate {
            version: version.to_string(),
            keywords,
            slot,
            sub_slot,
            repo_location: PathBuf::new(),
            // Below any repo priority, so the `vercmp` -> `repo_priority`
            // tie-break prefers an identical-version ebuild over a binary.
            repo_priority: i32::MIN,
            repo_name: entry
                .get("REPO")
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| "__unknown__".to_string()),
            license: entry.get("LICENSE").cloned().unwrap_or_default(),
            iuse: entry.get("IUSE").cloned().unwrap_or_default(),
            properties: entry.get("PROPERTIES").cloned().unwrap_or_default(),
            restrict: entry.get("RESTRICT").cloned().unwrap_or_default(),
            source: CandidateSource::Binary,
            binary_use: Some(binary_use),
            remote,
            build_id: entry.get("BUILD_ID").filter(|s| !s.is_empty()).cloned(),
            build_time: entry.get("BUILD_TIME").and_then(|s| s.trim().parse().ok()),
            // The index `EAPI` field; without one the binpkg predates the
            // format: EAPI 0 (same default as the ebuild cache path above).
            eapi: entry
                .get("EAPI")
                .cloned()
                .unwrap_or_else(|| "0".to_string()),
            binary_deps: ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
                .iter()
                .filter_map(|k| entry.get(*k).map(|v| (k.to_string(), v.clone())))
                .collect(),
        });
    }
    candidates
}

/// The `cat/pkg-ver:slot/sub::repo` match string for one `Packages`
/// record, for `--getbinpkg-exclude/include` filtering. `None` when the
/// record's `CPV` does not parse (such records never become candidates
/// either, in [`binary_candidates_from_index`]). Missing `SLOT`/`REPO`
/// fall back exactly like candidate construction (`0/0`,
/// `"__unknown__"`).
fn remote_record_match_str(record: &HashMap<String, String>) -> Option<String> {
    let cpv = record.get("CPV")?;
    let (category, package, version) = split_cpv(cpv)?;
    let (slot, sub_slot) = split_slot(record.get("SLOT").map(String::as_str).unwrap_or("0"));
    let repo = record
        .get("REPO")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "__unknown__".to_string());
    Some(format!(
        "{category}/{package}-{version}:{slot}/{sub_slot}::{repo}"
    ))
}

/// Whether one remote `Packages` record is filtered out by the effective
/// `--getbinpkg-exclude` / `--getbinpkg-include` sets: excluded on any
/// exclude match, or (with a non-empty include set) missing from the
/// whitelist. Matching is `WildcardPackageSet.containsCPV` (implicit
/// `*/` wildcarding via [`matches_cli_option_atom`]); either list may
/// hold whitespace-separated multi-atom elements, split like Portage's
/// `" ".join(atoms).split()`. Local `PKGDIR` instances never reach this
/// predicate (Portage filters the remote index at load time only).
// Portage: lib/portage/dbapi/bintree.py, _populate_remote_repo
pub fn getbinpkg_record_excluded(
    record: &HashMap<String, String>,
    exclude: &[String],
    include: &[String],
) -> bool {
    let Some(candidate_str) = remote_record_match_str(record) else {
        return false;
    };
    if exclude
        .iter()
        .flat_map(|s| s.split_whitespace())
        .any(|ex| matches_cli_option_atom(ex, &candidate_str))
    {
        return true;
    }
    let includes: Vec<&str> = include.iter().flat_map(|s| s.split_whitespace()).collect();
    if !includes.is_empty()
        && !includes
            .iter()
            .any(|inc| matches_cli_option_atom(inc, &candidate_str))
    {
        return true;
    }
    false
}

/// Collapses binpkg-multi-instance duplicates (several entries, different
/// `BUILD_ID`s, per `cat/pkg-ver:slot/sub_slot::repo`): keeps the highest-
/// `(BUILD_TIME, BUILD_ID)` instance satisfying the atom's `[use]` deps -- or
/// the newest anyway, preserving the `NoVisibleCandidate`/ebuild-fallback path.
/// A masked instance never wins while a visible one exists (Portage
/// `_iter_match_pkgs` yields newest first and stops at the first acceptable
/// one), so an `=`+build-id `package.mask` line unmasks the older build
/// instead of emptying the pool; when every instance is masked the ranked
/// newest is still kept, so the masked report lists it.
/// Portage `depgraph.py`, _iter_match_pkgs
pub(crate) fn dedup_binary_instances(
    candidates: Vec<Candidate>,
    atom: &portage_dep::Atom,
    config: &portage_profile::Config,
) -> Vec<Candidate> {
    let use_deps = atom.use_deps.as_deref().filter(|d| !d.is_empty());
    let use_ok = |c: &Candidate| -> bool {
        let Some(use_deps) = use_deps else {
            return true;
        };
        candidate_iuse_and_use(c, &atom.category, &atom.package, config).is_some_and(
            |(iuse, use_flags)| {
                portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
            },
        )
    };
    // Ranks an instance: satisfying the atom's use-deps outranks not, then
    // `(BUILD_TIME, BUILD_ID)`.
    let rank = |x: &Candidate| {
        (
            use_ok(x),
            x.build_time.unwrap_or(i64::MIN),
            x.build_id
                .as_deref()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(i64::MIN),
        )
    };
    // `Single` ebuilds stream through; binary groups are picked after the
    // pass so visibility can prefer an older unmasked instance over a
    // masked newer one. Positions preserve first-seen order (`(is_group,
    // idx)` into `singles`/`groups`, so no variant holds a `Candidate`).
    let mut order: Vec<(bool, usize)> = Vec::with_capacity(candidates.len());
    let mut singles: Vec<Option<Candidate>> = Vec::new();
    let mut group_idx: HashMap<(String, String, String, String), usize> = HashMap::new();
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    for c in candidates {
        if c.source != CandidateSource::Binary {
            singles.push(Some(c));
            order.push((false, singles.len() - 1));
            continue;
        }
        let key = (
            c.version.clone(),
            c.slot.clone(),
            c.sub_slot.clone(),
            c.repo_name.clone(),
        );
        match group_idx.get(&key).copied() {
            Some(idx) => groups[idx].push(c),
            None => {
                group_idx.insert(key, groups.len());
                groups.push(vec![c]);
                order.push((true, groups.len() - 1));
            }
        }
    }
    let mut out: Vec<Candidate> = Vec::with_capacity(order.len());
    for (is_group, idx) in order {
        if !is_group {
            if let Some(c) = singles[idx].take() {
                out.push(c);
            }
            continue;
        }
        let group = &groups[idx];
        // Visible instances first; the masked-only fallback keeps
        // the reporting path (strictly-greater keeps the first on
        // rank ties, as before).
        let any_visible = group
            .iter()
            .any(|c| is_visible(c, &atom.category, &atom.package, config));
        let mut best: Option<&Candidate> = None;
        for c in group {
            if any_visible && !is_visible(c, &atom.category, &atom.package, config) {
                continue;
            }
            if best.is_none_or(|b| rank(c) > rank(b)) {
                best = Some(c);
            }
        }
        if let Some(winner) = best {
            out.push(winner.clone());
        }
    }
    out
}

/// `--getbinpkg`/`-g`: binary candidates from every `config.binrepos` binrepo's
/// `Packages` index -- the live refresh's in-memory copy when it ran (Portage
/// `bintree._populate_remote`; `--pretend` refreshes like a
/// merge), else the on-disk index. A remote instance the local `$PKGDIR` also
/// carries is dropped (Portage `bintree.isremote`); binrepos resolve in order
/// with the same `vercmp` -> `repo_priority` tie-break (all binaries share
/// `repo_priority = i32::MIN`, so ties fall to first-seen).
pub fn list_remote_binary_candidates(
    binrepos: &[portage_profile::BinRepo],
    root: &Path,
    local: &BinaryIndex,
    category: &str,
    package: &str,
) -> Vec<Candidate> {
    if binrepos.is_empty() {
        return Vec::new();
    }
    // Instances are `(version, BUILD_ID)`: under binpkg-multi-instance a local
    // build 6 must not hide a remote build 7. A `BUILD_ID`-less
    // entry is its own `None` key, keeping the version-level shadowing.
    let local_instances: HashSet<(String, Option<String>)> =
        binary_candidates_from_index(local, category, package, false)
            .into_iter()
            .map(|c| (c.version, c.build_id))
            .collect();

    // An instance the local `$PKGDIR` or an earlier-priority binrepo carries
    // shadows a later binrepo's identical instance -- but one binrepo can list
    // several builds of one version, so the shadow set grows a whole binrepo
    // at a time (`dedup_binary_instances` collapses them later).
    let mut out: Vec<Candidate> = Vec::new();
    let mut shadowed: HashSet<(String, Option<String>)> = local_instances;
    for binrepo in binrepos {
        let binrepo_index = cached_binary_index(&binrepo.packages_dir(root));
        let mut this_repo: HashSet<(String, Option<String>)> = HashSet::new();
        for cand in binary_candidates_from_index(&binrepo_index, category, package, true) {
            let key = (cand.version.clone(), cand.build_id.clone());
            if shadowed.contains(&key) {
                continue;
            }
            this_repo.insert(key);
            out.push(cand);
        }
        shadowed.extend(this_repo);
    }
    out
}

/// Picks one `Packages` entry of `cat/pkg-version` (binpkg-multi-instance
/// aware). `Some(id)` selects exactly that `BUILD_ID`;
/// `None` the newest build -- highest `(BUILD_TIME, BUILD_ID)`, Portage
/// `dbapi._cmp_cpv` order.
pub(crate) fn pick_binary_instance<'a>(
    entries: &'a [HashMap<String, String>],
    want_cpv: &str,
    build_id: Option<&str>,
) -> Option<&'a HashMap<String, String>> {
    let matching = entries
        .iter()
        .filter(|e| e.get("CPV").map(String::as_str) == Some(want_cpv));
    match build_id {
        Some(id) => matching
            .into_iter()
            .find(|e| e.get("BUILD_ID").map(String::as_str) == Some(id)),
        None => matching.max_by_key(|e| {
            (
                e.get("BUILD_TIME")
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .unwrap_or(i64::MIN),
                e.get("BUILD_ID")
                    .and_then(|t| t.trim().parse::<i64>().ok())
                    .unwrap_or(i64::MIN),
            )
        }),
    }
}

/// The raw `Packages` record for `category/package-version` from the first
/// binrepo (in order) listing that exact CPV: the repo (sync URI, signature
/// policy) plus the record (`PATH`, `SIZE`, digests). `None` if no binrepo
/// lists it. Newest build when several exist; use
/// [`find_remote_binpkg_instance`] for a specific `BUILD_ID`.
/// Portage: gpkg per-binrepo signature policy
pub fn find_remote_binpkg<'a>(
    binrepos: &'a [portage_profile::BinRepo],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
    find_remote_binpkg_instance(FindRemoteBinpkgInstanceArgs {
        binrepos,
        root,
        category,
        package,
        version,
        build_id: None,
    })
}

/// Arguments of [`find_remote_binpkg_instance`]; the result borrows
/// only `binrepos` (`'a`).
pub struct FindRemoteBinpkgInstanceArgs<'a, 'b> {
    pub binrepos: &'a [portage_profile::BinRepo],
    pub root: &'b Path,
    pub category: &'b str,
    pub package: &'b str,
    pub version: &'b str,
    pub build_id: Option<&'b str>,
}

/// [`find_remote_binpkg`] for one specific build: `Some(build_id)`
/// scans the binrepos in order for exactly that `BUILD_ID`, falling back to
/// the newest build of the version.
pub fn find_remote_binpkg_instance<'a>(
    args: FindRemoteBinpkgInstanceArgs<'a, '_>,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
    let FindRemoteBinpkgInstanceArgs {
        binrepos,
        root,
        category,
        package,
        version,
        build_id,
    } = args;
    let want_cpv = format!("{category}/{package}-{version}");
    if build_id.is_some() {
        for binrepo in binrepos {
            let index = cached_binary_index(&binrepo.packages_dir(root));
            if let Some(entry) = pick_binary_instance(index.entries(), &want_cpv, build_id) {
                return Some((binrepo, entry.clone()));
            }
        }
    }
    for binrepo in binrepos {
        let index = cached_binary_index(&binrepo.packages_dir(root));
        if let Some(entry) = pick_binary_instance(index.entries(), &want_cpv, None) {
            return Some((binrepo, entry.clone()));
        }
    }
    None
}
