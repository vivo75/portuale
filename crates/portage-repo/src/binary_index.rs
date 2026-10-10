//! The binary package index (`Packages`) and binary candidates.

use crate::*;

/// Parses `<pkgdir>/Packages` (real `lib/portage/dbapi/bintree.py`'s own
/// index file) into one `HashMap` per package entry -- NOT the same
/// format `read_md5_cache` reads: real portage's own index format is
/// `KEY: value` (colon-space, confirmed by reading `getbinpkg.py`'s own
/// `PackageIndex` writer), blank-line-separated blocks, with the
/// *first* block being a global header (`PROFILE`/`ACCEPT_KEYWORDS`/etc
/// -- confirmed live against this machine's own real `/var/cache/
/// binpkgs/Packages`) rather than a package entry at all, so it's
/// always skipped here. Trusts the index outright -- no mtime/size
/// revalidation against the real on-disk binpkg file at this layer
/// (real portage's own default `FEATURES="pkgdir-index-trusted"`
/// behavior -- real `make.globals` ships it in the default `FEATURES`,
/// so `_populate_local` runs with `reindex=False` and verifies
/// size/digest at merge via `BinpkgVerifier`, backlog #174). The
/// reindex path's own mtime-staleness revalidation
/// (`bintree.py:1108-1136`'s own "avoid reading the xpak if possible"
/// fast path) lives one layer up, at the *local* `$PKGDIR`
/// CLI boundary (`portuale::binpkg::populate_local_pkgdir`, which calls
/// this function as its own fast-path cache and re-derives fresh
/// metadata via `read_xpak_metadata`/`read_gpkg_metadata` for anything
/// stale) -- this function alone is pub, format-parsing-only, with no
/// opinion on trust; `build_local_binpkg_index`'s own remote-binhost callers
/// still use it directly, unrevalidated, matching real (a synced remote
/// index has no local file to revalidate against). A missing `Packages`
/// file is an empty list, not an error -- same "PKGDIR simply has
/// nothing yet" tolerance a missing/empty repo directory already gets
/// in `list_candidates`.
pub fn read_packages_index(pkgdir: &Path) -> Vec<HashMap<String, String>> {
    let path = pkgdir.join("Packages");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_packages_index(&text).1
}

/// Split a `Packages` index text into its global header block (the
/// first `KEY: value` block -- real `getbinpkg.PackageIndex.header`,
/// carrying `TIMESTAMP`/`TTL`/`DOWNLOAD_TIMESTAMP` for the remote-index
/// freshness checks in `bintree._populate_remote_repo`) and one
/// `HashMap` per package entry. [`read_packages_index`] is this same
/// split with the header dropped.
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
    // The first block is the index's own global header, never a real
    // package entry -- see this function's own doc comment.
    if blocks.is_empty() {
        (HashMap::new(), Vec::new())
    } else {
        let header = blocks.remove(0);
        (header, blocks)
    }
}

/// A parsed binary-package index -- either the `KEY: value` blocks of a
/// `<pkgdir>/Packages` file (real `bintree`'s own on-disk index), or the
/// entries a `$PKGDIR` directory scan synthesized from each binpkg
/// file's own embedded metadata when there is no `Packages` at all
/// (real `bintree._populate_local`). Every downstream binary-candidate
/// function takes `&BinaryIndex` rather than re-reading a path, so the
/// "where did these entries come from" decision -- read the index file,
/// or scan the files -- is made exactly once, at the CLI boundary.
#[derive(Debug, Clone, Default)]
pub struct BinaryIndex {
    pub(crate) entries: Vec<HashMap<String, String>>,
    /// `(category, package)` -> the indices into `entries` for that cp,
    /// built once. A remote binhost index has ~18 000 entries and is
    /// consulted once per resolved atom, so a linear `CPV`-prefix scan
    /// per lookup was quadratic on a big graph.
    pub(crate) by_cp: HashMap<(String, String), Vec<usize>>,
}

impl PartialEq for BinaryIndex {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for BinaryIndex {}

impl BinaryIndex {
    /// Parse `<pkgdir>/Packages` (real `bintree`'s own index format --
    /// `KEY: value`, blank-line-separated blocks, first block is the
    /// global header and is skipped). A missing file is an empty index.
    pub fn from_pkgdir(pkgdir: &Path) -> Self {
        Self::from_entries(read_packages_index(pkgdir))
    }

    /// Build from entries a caller assembled itself -- a `$PKGDIR`
    /// directory scan (`portuale`'s `binpkg::scan_pkgdir`, which reads
    /// each `.tbz2`/`.gpkg.tar`'s own embedded metadata). Each entry is a
    /// synthesized `Packages`-style record (`CPV`, `SLOT`, `KEYWORDS`,
    /// the dep strings, `SIZE`, `REPO`, …).
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

/// `BinaryIndex::from_pkgdir`, memoised by directory for the life of the
/// process. A remote binhost's `Packages` index can be ~20 MB and is
/// re-consulted once per resolved atom, so parsing it afresh every time
/// made a large `emerge -p --getbinpkg` graph take minutes. `emerge` is
/// a one-shot process and a synced index doesn't change mid-run, so a
/// path-keyed global cache is safe.
///
/// A live binhost-index refresh (`portuale::emerge_getbinpkg`, real
/// `bintree._populate_remote`) installs a per-directory override here
/// *before* resolution, so the resolver uses the fetched index
/// in memory even when the edb cache write failed (real ignores an
/// unwritable cache dir, `bintree.py:1819-1823`, backlog #192) -- and,
/// on a non-`--pretend` fetch failure, suppresses even a stale cached
/// copy (real sets `pkgindex = None`, `bintree.py:1809`). `Some` is the
/// index to use; `None` means "this binrepo contributes nothing this
/// run". An override wins over both the memo below and the disk.
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

/// The live-refresh override table for [`cached_binary_index`], keyed by
/// the same `packages_dir` the resolver reads. Set once per binrepo by
/// the refresh, before any resolution. `emerge` is a one-shot process,
/// but an in-process multi-run caller (or tests sharing an EROOT)
/// must not inherit a previous run's indexes, so `pretend::run`
/// calls [`clear_remote_binary_index_overrides`] where a run starts;
/// unit tests use pid+nanos-unique temp dirs, so their keys never
/// collide either way.
pub(crate) static REMOTE_INDEX_OVERRIDE: OnceLock<
    RwLock<HashMap<PathBuf, Option<std::sync::Arc<BinaryIndex>>>>,
> = OnceLock::new();

/// Install (or replace) the in-memory index for `pkgdir` (real
/// `bintree`'s fetched-then-maybe-cached `pkgindex` for the run):
/// `Some(index)` resolves from it, `None` resolves nothing (a failed
/// non-`--pretend` refresh drops even a stale cache, like real).
pub fn set_remote_binary_index_override(pkgdir: &Path, index: Option<std::sync::Arc<BinaryIndex>>) {
    REMOTE_INDEX_OVERRIDE
        .get_or_init(|| RwLock::new(HashMap::new()))
        .write()
        .unwrap()
        .insert(pkgdir.to_path_buf(), index);
}

/// Drop every live-refresh override installed by
/// [`set_remote_binary_index_override`]. Called where a run starts
/// (review M4), so a second in-process run resolves from its own
/// refresh (or the disk) instead of inheriting the first run's
/// in-memory indexes.
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

/// The local `$PKGDIR` binary index for this run: the CLI layer's own
/// `$PKGDIR` directory scan (`config.scanned_binpkgs`) when it did one
/// -- i.e. `<pkgdir>/Packages` was absent -- otherwise the parsed
/// `<pkgdir>/Packages` file.
///
/// Backlog #123: built, not memoised. The old per-inputs memo keyed on
/// the scanned `Vec`'s raw pointer (an ABA hazard the moment a second
/// `scanned_binpkgs` assignment exists in one process), never re-read
/// `<pkgdir>/Packages` once `None` was cached, and ignored the
/// quickpkg-direct vdb entirely -- so the index is built once per
/// resolution (`ResolveCtx::new`) and the resulting `Arc` is threaded
/// to every consumer instead of being re-derived from `&Config` at each
/// call site.
pub fn build_local_binpkg_index(config: &portage_profile::Config) -> std::sync::Arc<BinaryIndex> {
    let mut entries = match &config.scanned_binpkgs {
        Some(entries) => entries.clone(),
        None => read_packages_index(Path::new(&config.pkgdir)),
    };
    // `--quickpkg-direct` (real `actions.py:150-164` + `bintree.
    // _populate_additional`): every package installed in the quickpkg
    // source root is injected into the binary-package pool as a candidate
    // for the target-`ROOT` build, using that root's own vdb-recorded
    // metadata. Real portage does `dbapi.cpv_inject` on each; portuale
    // synthesizes the same `Packages`-style records and appends them
    // here, so `list_binary_candidates` / `read_binary_metadata_any` /
    // the dependency walk all pick them up transparently. A CPV the local
    // `$PKGDIR` already carries wins (real `bintree.isremote` treats a
    // local dup as already-present). Set once by the CLI layer -- the
    // env-free process-global pattern `--package-moves` / `--useoldpkg-atoms`
    // already use.
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

/// `--quickpkg-direct-root` (real `actions.py:134-149`): the root whose
/// installed packages `--quickpkg-direct` offers as binary candidates.
/// `None` = not active (the default, or `--usepkg` absent, or the source
/// root coincides with the target `ROOT` -- all resolved by the CLI
/// layer). A process-global so it needn't thread through `resolve_pretend`'s
/// / `build_local_binpkg_index`'s signatures.
pub(crate) static QUICKPKG_DIRECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set by `pretend.rs` from `--quickpkg-direct[-root]` before any
/// resolution. Default (never called) is `None` -- a strict no-op.
pub fn set_quickpkg_direct_root(source_root: Option<PathBuf>) {
    *QUICKPKG_DIRECT_ROOT.write().unwrap() = source_root;
}

pub(crate) fn quickpkg_direct_root() -> Option<PathBuf> {
    QUICKPKG_DIRECT_ROOT.read().unwrap().clone()
}

/// Every package recorded under `<source_root>/var/db/pkg`, as a
/// `Packages`-style metadata record (real `bintree._populate_additional`'s
/// `_pkg_str(cpv, metadata=dict(zip(aux_keys, repo.aux_get(cpv, aux_keys))))`
/// -- the source is the vdb, so the deps/USE/keywords are that root's own
/// installed-time snapshot). Keys mirror what `binary_candidates_from_index`
/// / `read_binary_metadata` read: `CPV`, `SLOT` (with sub-slot),
/// `KEYWORDS`, `USE`, `IUSE`, `LICENSE`, `PROPERTIES`, `RESTRICT`, the
/// five `*DEPEND`, `REPO`.
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

/// Lists every binary-package build of `category/package` in `index`
/// -- real portage's own `bindbapi`, the "binary"
/// half of `_dynamic_depgraph_config`'s own candidate `dbs` list
/// (`depgraph.py`'s own `if myopts.get("--usepkg") is True: dbs.append((
/// bindb, "binary", ...))`, confirmed by reading it) -- added to the
/// same candidate pool `list_candidates` already builds for ebuilds,
/// only when `--usepkg`/`--usepkgonly` makes them eligible at all (see
/// `resolve_pretend`'s own doc comment). A binary candidate's own
/// `CPV` field (`category/package-version[-rN]`) is matched the exact
/// same way an ebuild filename already is: filtered to this specific
/// `category/package`, then `strip_version_prefix` peels the version
/// off -- no general `pkgsplit` needed, since the target package name
/// is already known here, same reasoning `list_candidates` itself
/// already relies on. `repo_location`/`repo_priority`/`repo_name` are
/// left as this candidate's own `PATH`/pseudo values (`""`/`0`/
/// `"__binary__"`) -- a binary candidate is never re-read from a repo's
/// own `metadata/md5-cache` the way an ebuild one is (`read_md5_cache`
/// is never called for it anywhere), so those fields go unused for this
/// source, not silently wrong.
pub fn list_binary_candidates(
    index: &BinaryIndex,
    category: &str,
    package: &str,
) -> Vec<Candidate> {
    binary_candidates_from_index(index, category, package, false)
}

/// Shared body of `list_binary_candidates` (local `$PKGDIR/Packages`,
/// `remote = false`) and `list_remote_binary_candidates`'s per-binrepo
/// scan (`remote = true`). `repo_name` comes from the index entry's own
/// `REPO` field (real `Packages` records the source repo per package, so
/// a `--getbinpkg` binary of `sys-fs/fuse` shows `::gentoo` at `-pv`),
/// falling back to `portage.versions._unknown_repo` (`"__unknown__"`) --
/// the same sentinel an installed package with no vdb `repository` file
/// already uses.
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
            // Deliberately lower than any real repo's own priority
            // (main repo defaults to real portage's own -1000) -- so
            // the existing, unmodified `vercmp` -> `repo_priority`
            // tie-break every candidate-selection site already uses
            // naturally prefers an identical-version ebuild over a
            // binary candidate, matching real depgraph.py's own `dbs`
            // list order (`"ebuild"` always checked before `"binary"`)
            // with no special-casing needed anywhere else.
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
            binary_deps: ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
                .iter()
                .filter_map(|k| entry.get(*k).map(|v| (k.to_string(), v.clone())))
                .collect(),
        });
    }
    candidates
}

/// Collapse binpkg-multi-instance duplicates: several `Packages` entries
/// for one `cat/pkg-ver:slot/sub_slot::repo` (different `BUILD_ID`s).
/// Real `_iter_match_pkgs` yields same-version instances newest-`BUILD_TIME`
/// first (`dbapi._cmp_cpv`, reversed) and the selection loop `break`s on
/// the first instance that passes every per-instance check -- visibility,
/// the atom's own `[use]` deps, `--binpkg-respect-use`. `--binpkg-respect-use`
/// has already run on `candidates`; this applies the atom-`[use]` check
/// (`atom`/`config`) and, per `cpv:slot::repo` group, keeps the
/// highest-`(BUILD_TIME, BUILD_ID)` instance that *satisfies* it -- or, if
/// none in the group do, the newest one anyway (so the group still flows
/// to the ordinary `NoVisibleCandidate` / ebuild-fallback path exactly as
/// a lone candidate would). Ebuild candidates pass straight through; the
/// kept instance takes the position of the group's first member.
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
    // key -> index into `out` of the currently-kept instance.
    let mut kept: HashMap<(String, String, String, String), usize> = HashMap::new();
    let mut out: Vec<Candidate> = Vec::with_capacity(candidates.len());
    // Ranks an instance: `(satisfies-atom-use, BUILD_TIME, BUILD_ID)` --
    // a satisfying instance always outranks a non-satisfying one.
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
    for c in candidates {
        if c.source != CandidateSource::Binary {
            out.push(c);
            continue;
        }
        let key = (
            c.version.clone(),
            c.slot.clone(),
            c.sub_slot.clone(),
            c.repo_name.clone(),
        );
        match kept.get(&key).copied() {
            Some(idx) => {
                if rank(&c) > rank(&out[idx]) {
                    out[idx] = c;
                }
            }
            None => {
                kept.insert(key, out.len());
                out.push(c);
            }
        }
    }
    out
}

/// `--getbinpkg`/`-g`: binary candidates for `category/package` from
/// every `config.binrepos` binrepo's own `Packages` index -- the live
/// refresh's in-memory copy when it ran (real `bintree._populate_remote`,
/// backlog #192; `--pretend` refreshes exactly like a real merge), else
/// the on-disk index (`BinRepo::packages_dir`: the edb cache for
/// `http(s)`/`ssh`, the URI's own directory for `file://`).
/// `root` is the `EROOT` under which the `http(s)://`/`ssh://` cache
/// lives (`BinRepo::packages_dir`). A remote build of a cpv+version the
/// local `$PKGDIR` (`local_pkgdir`) also carries is dropped -- real
/// `bintree.isremote` returns `False` once a package is in
/// `_additional_pkgs` (downloaded), and portuale treats "present in
/// `$PKGDIR/Packages`" as exactly that. Binrepos are consulted in
/// `config.binrepos` order (already `(priority, name)`-sorted); a later
/// binrepo does not shadow an earlier one for the same version (both
/// stay in the pool, resolved by the same `vercmp` -> `repo_priority`
/// tie-break every other candidate goes through -- and every binary
/// candidate shares `repo_priority = i32::MIN`, so ties fall to
/// first-seen, matching real's binrepo-priority order).
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
    // Instances are identified by `(version, BUILD_ID)`: under
    // binpkg-multi-instance real `bintree.isremote` is per `cpv` +
    // `build_id`, so a local build 6 must not hide a remote build 7
    // (backlog #300). An entry with no `BUILD_ID` is its own `None` key,
    // which keeps the pre-multi-instance version-level shadowing.
    let local_instances: HashSet<(String, Option<String>)> =
        binary_candidates_from_index(local, category, package, false)
            .into_iter()
            .map(|c| (c.version, c.build_id))
            .collect();

    // An instance already carried by the local `$PKGDIR` (real
    // `bintree.isremote`), or by an *earlier*-priority binrepo, shadows
    // a later binrepo's identical instance -- but binpkg-multi-instance
    // means one binrepo can legitimately list several builds of one
    // version, so the shadow set is only grown a whole binrepo at a time
    // (all of that binrepo's instances stay; `dedup_binary_instances`
    // collapses them later by `(cpv, slot, repo)` keeping the newest
    // `BUILD_TIME`).
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

/// Picks one `Packages` entry of `cat/pkg-version` from `entries`
/// (binpkg-multi-instance aware, backlog #299/#301). `Some(id)` selects
/// the entry whose `BUILD_ID` is exactly `id` (the build the resolver
/// planned); `None` selects the newest build -- highest `(BUILD_TIME,
/// BUILD_ID)`, real `dbapi._cmp_cpv` order -- which for an index with a
/// single (or `BUILD_ID`-less) entry is simply that entry.
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

/// The raw `Packages` index record for `category/package-version` from
/// the first binrepo (in `binrepos` order) whose cached index carries
/// it -- the repo itself (its `sync_uri` for the download, its
/// `verify_signature` for real `gpkg.__init__`'s own per-binrepo
/// signature policy, `gpkg.py:792-819`) plus the index record
/// (`PATH`, `SIZE`, the `SHA*`/`MD5` digests). `None` if no configured
/// binrepo lists that exact CPV. When the binrepo lists several builds
/// of the version, the newest one is returned; use
/// [`find_remote_binpkg_instance`] to ask for a specific `BUILD_ID`.
pub fn find_remote_binpkg<'a>(
    binrepos: &'a [portage_profile::BinRepo],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
    find_remote_binpkg_instance(binrepos, root, category, package, version, None)
}

/// [`find_remote_binpkg`] for one specific build (backlog #301): with
/// `Some(build_id)` the binrepos are scanned in order for the record
/// carrying exactly that `BUILD_ID`; if no binrepo has it (or `None` was
/// given) the newest build of the version from the first binrepo that
/// lists the version is returned.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn find_remote_binpkg_instance<'a>(
    binrepos: &'a [portage_profile::BinRepo],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> Option<(&'a portage_profile::BinRepo, HashMap<String, String>)> {
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
