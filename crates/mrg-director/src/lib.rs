//! The `mrg` **director** component contracts: traits, their
//! portuale/portage-repo implementations, and the `Director` wiring struct
//! routing every stage through its slots.
//!
//! The `mrg` applet is a *director*: it does not solve, read the cache, or
//! merge itself -- it orchestrates interchangeable, runtime-swappable
//! components, each behind a Rust trait. Every type in the method
//! signatures is a `portage-*` crate type or primitive, so an alternate
//! implementation needs no other dependency.
//!
//! Slot grounding (Portage source -> implementation here):
//!
//! | Slot | Portage | Portuale |
//! |------|---------|----------|
//! | Solver | `_emerge/depgraph.py` + `_emerge/resolver/backtracking.py` | `portage_repo::Resolver` (via `active_resolver()`) |
//! | PkgDatabase | `portage/dbapi/{vartree,porttree,bintree}.py` | `VdbReader` (filesystem) / `MemoryDb` (in-memory) |
//! | RepoCache | `portage/cache/template.py::database` | `Md5Cache` (flat file) / `VolatileCache` (in-memory) |
//! | BinpkgFetch | `portage/package/ebuild/fetch.py` + `_emerge/*binpkg*` | `WgetFetcher` (shared `wget` transport) |
//! | MergeEngine | `_emerge/MergeListItem.py` + `PackageMerge.py` / `EbuildMerge.py` | `SourceMergeEngine` / `BinaryMergeEngine` kind routing |
//! | BinpkgIndex | `portage/dbapi/bintree.py` (local `$PKGDIR` + remote `PORTAGE_BINHOST`) | `PkgdirBinIndex` / `RemoteBinhostIndex` |
//! | NewsSet | `portage/news.py::Item.isRelevant`/`isValid` | `FilesystemNews` in the binary crate; `MetadataNews` pins the shape |
//! | SchedulerPolicy | `_emerge/Scheduler.py::Scheduler._run` | `LoadAwarePolicy` (`--jobs=N`) / `UnlimitedPolicy` (bare `-j`) |
//! | Director | `actions.py::action_build` | `struct Director` below |
//!
//! A trait with a single implementation earns its place here for one
//! reason: **the director needs a stable spine before the interchangeable
//! algorithms land** (see notes). Slots carrying production traffic prove
//! the seam (`SchedulerPolicy`, `MergeEngine`, `NewsSelector`, `Fetcher`,
//! `PackagesDb::contents_files`); the rest stay swappable behind
//! `Director`'s delegation methods until their second algorithm lands.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// A dependency-resolution strategy: [`ResolveRequest`] in, merge-ordered
/// [`GraphResult`] out (or a hard-error string).
///
/// A **re-export** of `portage_repo::Resolver`, so there is exactly one
/// solver seam in the workspace. The only implementation is
/// [`portage_repo::BacktrackingResolver`], via [`portage_repo::active_resolver`].
///
/// Grounding: Portage's solver boundary is `_emerge/actions.py`
/// constructing `_emerge/depgraph.py`'s `depgraph` from
/// `create_depgraph_params(...)` and reading the merged graph back out.
pub use portage_repo::Resolver;

// ---------------------------------------------------------------------------
// PkgDatabase
// ---------------------------------------------------------------------------

/// An **installed-packages database**: what the director needs from
/// `/var/db/pkg` to decide ownership, reverse deps, and slot occupancy.
/// Portage: `portage/dbapi/vartree.py` (`vartree` read side; see notes
/// for the v1 subset and the future-implementation rule).
pub trait PackagesDb {
    /// The CPVs installed under `root` for one `category/package`,
    /// highest version first (Portage `dbapi.cp_list` order).
    fn installed_versions(&self, category: &str, package: &str) -> Vec<String>;

    /// The paths one installed CPV owns: its vdb `CONTENTS` entries' path
    /// fields, with one leading `/` stripped.
    fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String>;

    /// The installed CPVs directly depending on `consumer_cpv` (via
    /// `portage_dep::match_from_list` over their dep strings).
    fn reverse_dependents(
        &self,
        consumer_category: &str,
        consumer_package: &str,
        consumer_version: &str,
    ) -> Vec<String>;

    /// The single current `root` this database reads, so a database
    /// value is portable as one unit across roots.
    fn root(&self) -> &Path;
}

// ---------------------------------------------------------------------------
// RepoCache
// ---------------------------------------------------------------------------

/// A **repository metadata cache backend**: a `cat/pkg-ver` →
/// `(aux key → value)` map read from a repo's cache directory.
/// Portage: `portage/cache/template.py::database` (see notes for the
/// backends and the narrowed-out machinery).
pub trait RepoCache {
    /// The full aux dictionary for one `category/package-revision`
    /// ("pf"), or `Err` when absent (`DEPEND`, `KEYWORDS`, … spelling).
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String>;

    /// The `pf` names present for one category, in stable sort order.
    fn category(&self, category: &str) -> Vec<String>;

    /// The repo this cache reads (`::reponame`), for provenance.
    fn repo(&self) -> &str;
}

// ---------------------------------------------------------------------------
// BinpkgFetch
// ---------------------------------------------------------------------------

/// A source that can materialize a package's files locally so a merge or
/// build can consume them. The implementation is [`WgetFetcher`].
/// Portage: `portage/package/ebuild/fetch.py` (see notes for the seam's
/// narrowing: no Manifest verification, no `fsmirror` copy).
///
/// The trait takes one already-assembled [`FetchRequest`]: candidate
/// assembly and the `mirror+`/`fetch+` override policy stay at the call
/// site; the transport owns only the download. The Manifest-derived
/// `DIGESTS` variable travels pre-formatted in [`FetchRequest::vars`].
pub trait Fetcher {
    /// Download `request.uri` to `request.dest` (resume when
    /// `request.resume` is set); digest verification is the caller's.
    fn fetch(&self, request: &FetchRequest<'_>) -> Result<(), String>;
}

/// One resolved candidate download for a [`Fetcher`]: the URI, the local
/// distfile path it must land in, and whether to resume a partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchRequest<'a> {
    /// The `SRC_URI` filename this candidate serves (the `dest` basename).
    pub filename: &'a str,
    /// The fully resolved URI to download.
    pub uri: &'a str,
    /// The local path the download must be written to (the call site's
    /// already-computed `distdir/<filename>`; never re-derived here).
    pub dest: &'a Path,
    /// Resume a non-empty partial at `dest` (Portage `RESUMECOMMAND`).
    pub resume: bool,
    /// The `FETCHCOMMAND`/`RESUMECOMMAND` family; `None` uses the
    /// `make.globals` defaults (`portage_fetch::default_commands`).
    pub commands: Option<&'a portage_fetch::FetchCommands>,
    /// Portage `DIGESTS` and `PORTAGE_SSH_OPTS` inputs, pre-formatted by
    /// the caller; unset variables expand to empty, same as Portage.
    pub vars: portage_fetch::FetchCommandVars<'a>,
}

// ---------------------------------------------------------------------------
// MergeEngine
// ---------------------------------------------------------------------------

/// How one merge unit is built: from source or from a binary package.
/// Portage: `_emerge/MergeListItem.py::_start` on `pkg.type_name` (same
/// bit as `portage_repo::CandidateSource::{Ebuild, Binary}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    /// Build from the ebuild, then merge (`EbuildBuild` + `EbuildMerge`).
    Source,
    /// Unpack a binpkg and merge it (`_emerge/Binpkg.py`).
    Binary,
}

/// Portage `Scheduler._pkg_count`: the per-package `(curval, maxval)`
/// snapshot each `MergeListItem` carries (`maxval` counts only merges).
/// Every merge progress line prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeProgress {
    /// 1-based ordinal of this unit among the run's merging entries.
    pub cur: usize,
    /// Total merging entries in the run.
    pub max: usize,
}

impl MergeProgress {
    /// A lone unit executed outside a merge run: it is the only package,
    /// so it is 1 of 1.
    pub fn single() -> Self {
        Self { cur: 1, max: 1 }
    }
}

/// A single unit of merge work: one resolved entry as plain data (never
/// how a source merge is executed). Derived from the solver's
/// [`portage_repo::GraphEntry`] plus the entry's `CandidateSource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeUnit {
    /// The resolved target, `category/package-version`.
    pub cpv: String,
    /// Source build or binary unpack.
    pub kind: MergeKind,
    /// `::repo` the candidate resolved from, for provenance.
    pub repo: Option<String>,
    /// The `${ROOT}` this unit merges into.
    pub root: PathBuf,
    /// The in-slot installed version an in-place same-slot replace must
    /// unmerge first. `None` when nothing is replaced.
    pub replaces_same_slot: Option<String>,
    /// A binary unit fetchable from a binhost. Source units ignore it.
    pub remote_binary: bool,
    /// A build-id-qualified binary: the exact `$PKGDIR` file among
    /// multi-instance same-version binpkgs. Source units ignore it.
    pub build_id: Option<String>,
    /// The resolved slot/sub-slot, threaded into the build-phase env.
    /// `None` behaves like the resolver's own `unwrap_or("0")` default.
    pub slot: Option<String>,
    /// See [`MergeUnit::slot`].
    pub sub_slot: Option<String>,
    /// The entry's resolved IUSE flags: the source engine exports the
    /// enabled ones as the build-phase `USE`. Empty means `USE=""`.
    pub use_flags: Vec<(String, bool)>,
    /// This unit's `(curval, maxval)` snapshot; hand-built units keep `single()`.
    pub progress: MergeProgress,
}

impl MergeUnit {
    /// A source-build unit for `cpv` into `root` (no replace, local,
    /// unqualified -- the common case).
    pub fn source(cpv: &str, root: &Path) -> Self {
        Self {
            cpv: cpv.to_string(),
            kind: MergeKind::Source,
            repo: None,
            root: root.to_path_buf(),
            replaces_same_slot: None,
            remote_binary: false,
            build_id: None,
            slot: None,
            sub_slot: None,
            use_flags: Vec::new(),
            progress: MergeProgress::single(),
        }
    }

    /// A binary-unpack unit for `cpv` into `root` (same defaults).
    pub fn binary(cpv: &str, root: &Path) -> Self {
        Self {
            kind: MergeKind::Binary,
            ..Self::source(cpv, root)
        }
    }
}

/// The execution context a merge engine runs under: Portage
/// `_emerge/Scheduler.py`'s two knobs (`--jobs`, `--keep-going`) plus the
/// paths the merge writes to. Deliberately coarse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeContext {
    /// The `${ROOT}` the engine merges into.
    pub root: PathBuf,
    /// `PORTAGE_TMPDIR`-adjacent build root (`${PORTAGE_BUILDDIR}` base).
    pub builddir: PathBuf,
    /// Max concurrent builds (`--jobs=N`; 1 = serial).
    pub jobs: usize,
    /// On a unit failure, drop its transitive dependents and merge the
    /// rest instead of aborting (Portage `Scheduler._calc_resume_list`).
    pub keep_going: bool,
}

/// The coarse three-valued outcome Portage reports in the `>>>`/`!!!`
/// merge lines. An engine returns one per [`MergeUnit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The unit merged (vdb entry written).
    Merged,
    /// The unit failed; carries the `!!!` line's explanation.
    Failed(String),
    /// The unit was skipped (a `--keep-going` dependent of a failure, or
    /// a fetch-only dry pass); carries the reason.
    Skipped(String),
}

/// An **ebuild merge method**: whatever turns merge units into an
/// installed system. One call per unit; the director owns ordering and
/// failure policy, the engine owns the copy/install.
/// Portage: `_emerge/PackageMerge.py` + `vartree.py::dblink.merge()`.
pub trait MergeEngine {
    /// Execute `unit` under `ctx`. Safe to call concurrently up to
    /// `ctx.jobs` for the *build* half; the vdb-write half is serialized.
    fn execute(&self, unit: &MergeUnit, ctx: &MergeContext) -> MergeOutcome;
}

/// The source-build `MergeEngine`: Portage `_emerge/MergeListItem.py`'s
/// `"ebuild"` arm. The phase chain lives in the `portuale` binary crate;
/// this marker pins the kind dispatch (own kind `Skipped`, other kind
/// `Failed`).
pub struct SourceMergeEngine;
impl MergeEngine for SourceMergeEngine {
    fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
        if unit.kind == MergeKind::Source {
            MergeOutcome::Skipped(
                "SourceMergeEngine marker: source merges execute in portuale::emerge_build"
                    .to_string(),
            )
        } else {
            MergeOutcome::Failed(format!(
                "SourceMergeEngine cannot merge binary unit {}",
                unit.cpv
            ))
        }
    }
}

/// The binary-unpack `MergeEngine`: Portage `_emerge/MergeListItem.py`'s
/// `"binary"` arm. Mirrors [`SourceMergeEngine`] for the other kind: a
/// [`MergeKind::Binary`] unit is declined as [`MergeOutcome::Skipped`],
/// a [`MergeKind::Source`] unit refused as [`MergeOutcome::Failed`].
pub struct BinaryMergeEngine;
impl MergeEngine for BinaryMergeEngine {
    fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
        if unit.kind == MergeKind::Binary {
            MergeOutcome::Skipped(
                "BinaryMergeEngine marker: binary merges execute in portuale::emerge_getbinpkg"
                    .to_string(),
            )
        } else {
            MergeOutcome::Failed(format!(
                "BinaryMergeEngine cannot merge source unit {}",
                unit.cpv
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// BinpkgIndex
// ---------------------------------------------------------------------------

/// A source of **binary-package candidates** for a `cat/pkg`, from
/// whatever store backs them. Both backends funnel through
/// `portage_repo::BinaryIndex`; the trait keeps them swappable.
/// Portage: `portage/dbapi/bintree.py` (local `$PKGDIR` + remote
/// `PORTAGE_BINHOST`; see notes).
pub trait BinpkgIndex {
    /// The binary candidates for `category/package`, highest version
    /// first (Portage `bintree.cp_list` order).
    fn candidates(&self, category: &str, package: &str) -> Vec<portage_repo::Candidate>;

    /// One candidate's raw `Packages` aux record -- the Portage
    /// `bintree.aux_get` surface. `None` for an unknown exact CPV.
    fn metadata(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<std::collections::HashMap<String, String>>;

    /// One candidate's aux record **plus the identity of the store that
    /// held it** -- the download-side counterpart of [`Self::metadata`]
    /// (see notes for the per-backend identity and lookup rules).
    fn metadata_with_source(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<(String, std::collections::HashMap<String, String>)>;

    /// [`Self::metadata_with_source`] for the build the resolver planned
    /// (binpkg-multi-instance; see notes). The default ignores
    /// `build_id`, so single-instance backends need no change.
    fn metadata_with_source_instance(
        &self,
        category: &str,
        package: &str,
        version: &str,
        build_id: Option<&str>,
    ) -> Option<(String, std::collections::HashMap<String, String>)> {
        let _ = build_id;
        self.metadata_with_source(category, package, version)
    }

    /// Where this index's candidates come from for the `g` bracket / `-pv`
    /// `::repo` provenance decoration.
    fn source_name(&self) -> String;
}

// ---------------------------------------------------------------------------
// NewsSet
// ---------------------------------------------------------------------------

/// A **news / security-item selector**: the swappable relevance decision
/// behind `--check-news` (and a future `@security` GLSA read).
/// Portage: `portage/news.py` (`Item.isRelevant` / `isValid`); the GLSA
/// half stays gated on `@security` entering scope (see notes).
pub trait NewsSelector {
    /// The `metadata/news/<id>` item id of every valid, not-already-read,
    /// currently-relevant item this selector reads.
    fn unread_ids(&self) -> Vec<String>;

    /// The repo whose `metadata/news` this selector reads, for the
    /// `… news items need reading for repository '<repo>'.` line.
    fn repo_name(&self) -> String;
}

// ---------------------------------------------------------------------------
// SchedulerPolicy
// ---------------------------------------------------------------------------

/// A **build-scheduler dispatch policy**: when the `-jN` merge DAG may
/// start another build. A different scheduler shape changes only this
/// method, never the DAG walk that calls it.
/// Portage: `_emerge/Scheduler.py::Scheduler._run` (bounded by `--jobs`
/// and `--load-average`; the first build is always allowed).
pub trait SchedulerPolicy {
    /// Whether to dispatch another build now (`running` in flight,
    /// `loadavg_1min` from `/proc/loadavg`); `running == 0` is unconditional.
    fn should_start(&self, running: usize, loadavg_1min: f64) -> bool;

    /// The hard concurrency ceiling (`--jobs=N`; 1 = serial merge).
    fn max_jobs(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Implementation markers (the single implementations that satisfy the
// contracts today)
// ---------------------------------------------------------------------------

/// The filesystem `PackagesDb`: queries go through `portage_repo`
/// helpers over `portage_vdb::for_root(root)`. One of two implementations
/// (the other is [`MemoryDb`]); see notes for the production traffic.
pub struct VdbReader<'a> {
    root: &'a Path,
}

impl<'a> VdbReader<'a> {
    /// A reader over `root` (`<root>/var/db/pkg`).
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }
}

impl PackagesDb for VdbReader<'_> {
    fn installed_versions(&self, category: &str, package: &str) -> Vec<String> {
        // Portage `dbapi.cp_list` sorts highest-first; the vdb scan yields
        // `read_dir` order, so sort here to honour the trait contract.
        let mut versions = portage_repo::installed_versions(self.root, category, package);
        versions.sort_by(|a, b| portage_versions::vercmp(b, a).unwrap_or(0).cmp(&0));
        versions
    }
    fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String> {
        portage_repo::installed_contents_files(self.root, category, package, version)
    }
    fn reverse_dependents(
        &self,
        consumer_category: &str,
        consumer_package: &str,
        consumer_version: &str,
    ) -> Vec<String> {
        portage_repo::installed_reverse_dependents(
            self.root,
            consumer_category,
            consumer_package,
            consumer_version,
        )
    }
    fn root(&self) -> &Path {
        self.root
    }
}

/// The in-memory `PackagesDb`: the "second implementation per slot",
/// holding a recorded snapshot instead of reading `<root>/var/db/pkg`.
/// Portage: `_emerge/FakeVartree.py::FakeVartree` (see notes for the two
/// deliberate narrowings).
#[derive(Debug, Clone, Default)]
pub struct MemoryDb {
    /// The single `root` this snapshot stands in for (owned, so a
    /// snapshot value is portable as one unit).
    root: PathBuf,
    /// `(category, package) → installed versions`, recorded order.
    versions: std::collections::HashMap<(String, String), Vec<String>>,
    /// `(category, package, version) → CONTENTS-files list`.
    contents: std::collections::HashMap<(String, String, String), Vec<String>>,
    /// `(category, package, version) → direct dependents` (CPVs).
    dependents: std::collections::HashMap<(String, String, String), Vec<String>>,
}

impl PackagesDb for MemoryDb {
    fn installed_versions(&self, category: &str, package: &str) -> Vec<String> {
        self.versions
            .get(&(category.to_string(), package.to_string()))
            .cloned()
            .unwrap_or_default()
    }
    fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String> {
        self.contents
            .get(&(
                category.to_string(),
                package.to_string(),
                version.to_string(),
            ))
            .cloned()
            .unwrap_or_default()
    }
    fn reverse_dependents(
        &self,
        consumer_category: &str,
        consumer_package: &str,
        consumer_version: &str,
    ) -> Vec<String> {
        self.dependents
            .get(&(
                consumer_category.to_string(),
                consumer_package.to_string(),
                consumer_version.to_string(),
            ))
            .cloned()
            .unwrap_or_default()
    }
    fn root(&self) -> &Path {
        &self.root
    }
}

/// The filesystem-backed `RepoCache`: the flat md5-cache directory
/// (Portage `flat_hash.py`'s layout), with an ebuild-tree fallback when
/// the repo has no usable cache (see notes).
pub struct Md5Cache<'a> {
    repo_location: &'a Path,
    repo_name: &'a str,
}

impl<'a> Md5Cache<'a> {
    /// Wrap one repo location under its `repos.conf` name (the
    /// `::reponame` provenance `RepoCache::repo` reports).
    pub fn new(repo_location: &'a Path, repo_name: &'a str) -> Self {
        Self {
            repo_location,
            repo_name,
        }
    }
}
impl RepoCache for Md5Cache<'_> {
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        portage_repo::repo_aux_metadata(self.repo_location, category, pf)
            .map(|m| (*m).clone())
            .map_err(|e| e.to_string())
    }
    fn category(&self, category: &str) -> Vec<String> {
        if portage_repo::has_usable_md5_cache(self.repo_location) {
            let dir = self
                .repo_location
                .join("metadata")
                .join("md5-cache")
                .join(category);
            portage_util::read_dir_entries(&dir)
                .map(|it| {
                    it.into_iter()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| !n.contains('.'))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            // No cache dir: walk the `<cat>/<pkg>/<pf>.ebuild` tree for
            // each ebuild file's `<pkg>-<version>` stem.
            let dir = self.repo_location.join(category);
            let mut names: Vec<String> = portage_util::read_dir_entries(&dir)
                .map(|it| {
                    it.into_iter()
                        .filter(|e| e.path().is_dir())
                        .flat_map(|pkg_dir| {
                            portage_util::read_dir_entries(&pkg_dir.path())
                                .map(|files| {
                                    files
                                        .into_iter()
                                        .filter(|f| f.path().is_file())
                                        .filter_map(|f| {
                                            let name = f.file_name().to_string_lossy().into_owned();
                                            name.strip_suffix(".ebuild").map(str::to_string)
                                        })
                                        .collect::<Vec<String>>()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names.dedup();
            names
        }
    }
    fn repo(&self) -> &str {
        self.repo_name
    }
}

/// The in-memory `RepoCache` backend: the "second implementation per
/// slot", holding `Md5Cache`'s flat aux dicts in an owned map.
/// Portage: `portage/cache/volatile.py::database` (`deepcopy` on read and
/// write: `insert` clones in, [`RepoCache::metadata`] clones out).
#[derive(Debug, Clone, Default)]
pub struct VolatileCache {
    /// `(category, pf) → aux dict`, the `volatile._data` dict.
    entries: std::collections::HashMap<(String, String), std::collections::HashMap<String, String>>,
    /// The repo this cache stands in for (`::reponame`), for provenance.
    repo_name: String,
}

impl RepoCache for VolatileCache {
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        // A copy out, never a borrow of the store -- Portage
        // `volatile.__getitem__`'s `deepcopy`.
        self.entries
            .get(&(category.to_string(), pf.to_string()))
            .cloned()
            .ok_or_else(|| format!("volatile cache has no entry for {category}/{pf}"))
    }
    fn category(&self, category: &str) -> Vec<String> {
        let mut pfs: Vec<String> = self
            .entries
            .keys()
            .filter(|(c, _)| c == category)
            .map(|(_, pf)| pf.clone())
            .collect();
        pfs.sort();
        pfs
    }
    fn repo(&self) -> &str {
        &self.repo_name
    }
}

/// The current `Fetcher`: source via the shared `wget` transport
/// (`portage_fetch::download_via_wget`). One request, one `wget`
/// invocation; the already-materialized short-circuit and Manifest
/// verification live at the call sites, not here.
pub struct WgetFetcher;
impl Fetcher for WgetFetcher {
    fn fetch(&self, request: &FetchRequest<'_>) -> Result<(), String> {
        let distdir = request.dest.parent().unwrap_or_else(|| Path::new("."));
        portage_fetch::download_with_commands(portage_fetch::DownloadWithCommandsArgs {
            uri: request.uri,
            dest: request.dest,
            resume: request.resume,
            distdir,
            vars: request.vars,
            commands: request
                .commands
                .unwrap_or_else(|| portage_fetch::default_commands()),
        })
    }
}

/// The local `$PKGDIR` `BinpkgIndex`: Portage `bintree.py`'s local-store
/// candidates via `portage_repo::BinaryIndex`. One of two backends (the
/// other is [`RemoteBinhostIndex`]).
pub struct PkgdirBinIndex<'a> {
    /// The parsed `$PKGDIR` index and path for provenance.
    pub index: &'a portage_repo::BinaryIndex,
    /// The `$PKGDIR` directory this reads, for [`BinpkgIndex::source_name`].
    pub pkgdir: &'a Path,
}
impl BinpkgIndex for PkgdirBinIndex<'_> {
    fn candidates(&self, category: &str, package: &str) -> Vec<portage_repo::Candidate> {
        portage_repo::list_binary_candidates(self.index, category, package)
    }
    fn metadata(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        portage_repo::read_binary_metadata(self.index, category, package, version)
    }
    fn metadata_with_source(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<(String, std::collections::HashMap<String, String>)> {
        self.metadata(category, package, version)
            .map(|m| (self.pkgdir.to_string_lossy().into_owned(), m))
    }
    fn source_name(&self) -> String {
        self.pkgdir.to_string_lossy().into_owned()
    }
}

/// The remote `PORTAGE_BINHOST` `BinpkgIndex`: the synced-`Packages`
/// source behind `--getbinpkg`. The "second implementation per slot"
/// (see notes for the shadowing and remote-only rules).
pub struct RemoteBinhostIndex<'a> {
    /// The resolved binhost configuration, Portage `BinRepoConfig` values.
    pub config: &'a portage_profile::Config,
    /// The `root` whose `$PKGDIR` the sync lands in.
    pub root: &'a Path,
    /// The local `$PKGDIR` index (locally-provided remotes are shadowed
    /// out, Portage `bintree.isremote`).
    pub local: &'a portage_repo::BinaryIndex,
}
impl BinpkgIndex for RemoteBinhostIndex<'_> {
    fn candidates(&self, category: &str, package: &str) -> Vec<portage_repo::Candidate> {
        portage_repo::list_remote_binary_candidates(
            &self.config.binrepos,
            self.root,
            self.local,
            category,
            package,
        )
    }
    fn metadata(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        portage_repo::read_binary_metadata_any(portage_repo::ReadBinaryMetadataAnyArgs {
            config: self.config,
            root: self.root,
            local: self.local,
            category,
            package,
            version,
        })
    }
    fn metadata_with_source(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<(String, std::collections::HashMap<String, String>)> {
        // Remote-only, binrepos in order, first CPV hit wins, carrying
        // the owning binrepo's name for its `sync-uri`.
        self.metadata_with_source_instance(category, package, version, None)
    }
    fn metadata_with_source_instance(
        &self,
        category: &str,
        package: &str,
        version: &str,
        build_id: Option<&str>,
    ) -> Option<(String, std::collections::HashMap<String, String>)> {
        portage_repo::find_remote_binpkg_instance(portage_repo::FindRemoteBinpkgInstanceArgs {
            binrepos: &self.config.binrepos,
            root: self.root,
            category,
            package,
            version,
            build_id,
        })
        .map(|(binrepo, m)| (binrepo.name.clone(), m))
    }
    fn source_name(&self) -> String {
        self.config
            .binrepos
            .iter()
            .map(|b| b.name.clone())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The capped-jobs `SchedulerPolicy`: start another build while
/// `running < jobs` and under the `--load-average` ceiling (never gating
/// the first build). One of two implementations (the other is
/// [`UnlimitedPolicy`]); `run_build_scheduler` runs under one of them.
#[derive(Debug, Clone, Copy)]
pub struct LoadAwarePolicy {
    /// `--jobs` ceiling (`max_jobs`).
    jobs: usize,
    /// `--load-average` ceiling; `None` disables load gating.
    load_average: Option<f64>,
}
impl LoadAwarePolicy {
    /// A policy for `--jobs=jobs` with an optional `--load-average`
    /// ceiling -- the scheduler shape `run_build_scheduler` runs under.
    pub fn new(jobs: usize, load_average: Option<f64>) -> Self {
        Self { jobs, load_average }
    }
}
impl Default for LoadAwarePolicy {
    fn default() -> Self {
        // Portage's default scheduler is unbounded (bare `-j`); the
        // portable default here is serial and ungated.
        Self {
            jobs: 1,
            load_average: None,
        }
    }
}
impl SchedulerPolicy for LoadAwarePolicy {
    fn should_start(&self, running: usize, loadavg_1min: f64) -> bool {
        // The first build is always allowed; additional ones only under
        // the ceilings.
        running == 0
            || (running < self.jobs && self.load_average.is_none_or(|la| loadavg_1min <= la))
    }
    fn max_jobs(&self) -> usize {
        self.jobs
    }
}

/// The unlimited-jobs `SchedulerPolicy` (Portage's `max_jobs is True`,
/// a bare `-j`): no concurrency ceiling; `--load-average` still gates
/// *additional* builds. The scheduler slot's second implementation.
#[derive(Debug, Clone, Copy)]
pub struct UnlimitedPolicy {
    /// `--load-average` ceiling; `None` disables load gating.
    load_average: Option<f64>,
}
impl UnlimitedPolicy {
    /// An uncapped policy with an optional `--load-average` ceiling.
    pub fn new(load_average: Option<f64>) -> Self {
        Self { load_average }
    }
}
impl SchedulerPolicy for UnlimitedPolicy {
    fn should_start(&self, running: usize, loadavg_1min: f64) -> bool {
        // Like `LoadAwarePolicy` minus the ceiling: the first build is
        // always allowed, further ones only while under the load gate.
        running == 0 || self.load_average.is_none_or(|la| loadavg_1min <= la)
    }
    fn max_jobs(&self) -> usize {
        usize::MAX
    }
}

// ---------------------------------------------------------------------------
// Director
// ---------------------------------------------------------------------------

/// The `mrg` **director**: one solver, one installed-db, one repo cache,
/// one fetcher, one merge engine, one binary-package index, one news
/// selector, one scheduler policy -- held together, never implemented here.
/// Portage: `actions.py::action_build` (resolve, then hand the merge list
/// to the scheduler). V1 narrowness (see notes): the `-jN` DAG walk stays
/// in `emerge_build.rs`; the `mrg` applet still calls `pretend::run` directly.
pub struct Director<S, D, C, F, M, B, N, P> {
    /// Dependency-resolution strategy (the only slot used by `plan()`).
    pub solver: S,
    /// Already-installed packages (`/var/db/pkg` read side).
    pub packages_db: D,
    /// Repository metadata cache backend.
    pub repo_cache: C,
    /// `SRC_URI` materialization.
    pub fetcher: F,
    /// Copy/install execution.
    pub merge_engine: M,
    /// Binary-package candidate store (`$PKGDIR` / `PORTAGE_BINHOST`).
    pub binpkg_index: B,
    /// News-item (and future GLSA) relevance selector.
    pub news_selector: N,
    /// `-jN` build dispatch policy.
    pub scheduler_policy: P,
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    S: Resolver,
{
    /// Resolve `req` through the director's solver. Every later stage
    /// (fetch, merge) consumes that plan; none of them re-resolves.
    pub fn plan(
        &self,
        req: &portage_repo::ResolveRequest,
    ) -> Result<portage_repo::GraphResult, portage_repo::Error> {
        self.solver.resolve(req)
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    C: RepoCache,
{
    /// One package's aux dict through the director's repo cache.
    pub fn repo_metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        self.repo_cache.metadata(category, pf)
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    B: BinpkgIndex,
{
    /// One binary candidate's aux record through the director's index.
    pub fn binpkg_metadata(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        self.binpkg_index.metadata(category, package, version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::{GraphResult, ResolveRequest};
    use portage_util::TempDir;

    /// Arguments of [`write_vdb_entry`].
    struct WriteVdbEntryArgs<'a> {
        root: &'a Path,
        category: &'a str,
        package: &'a str,
        version: &'a str,
        slot: &'a str,
        files: &'a [(&'a str, &'a str)],
    }

    /// Write one vdb entry: `<root>/var/db/pkg/<cat>/<pkg>-<ver>/` with
    /// `SLOT` plus the given `(filename, content)` files.
    fn write_vdb_entry(args: WriteVdbEntryArgs<'_>) {
        let WriteVdbEntryArgs {
            root,
            category,
            package,
            version,
            slot,
            files,
        } = args;
        let dir = root
            .join("var/db/pkg")
            .join(category)
            .join(format!("{package}-{version}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SLOT"), slot).unwrap();
        for (name, content) in files {
            std::fs::write(dir.join(name), content).unwrap();
        }
    }

    /// `Resolver::resolve` returns portage-repo's plan type, so an
    /// alternate solver must speak Portage-facing data.
    #[test]
    fn resolver_speaks_graph_result() {
        let _plan: &dyn Fn(&ResolveRequest) -> Result<GraphResult, portage_repo::Error> =
            &|_req| Err(portage_repo::Error::Detail("inert pin".into()));
    }

    /// `PackagesDb` is three read queries plus root; a fake proves the
    /// director never reaches past them.
    #[test]
    fn packages_db_is_three_queries_plus_root() {
        let db = FakeDb {
            root: Path::new("/root"),
        };
        assert_eq!(db.installed_versions("dev-libs", "example"), vec!["1.0"]);
        assert_eq!(
            db.contents_files("dev-libs", "example", "1.0"),
            vec!["usr/lib/libx.a"]
        );
        assert_eq!(
            db.reverse_dependents("dev-libs", "example", "1.0"),
            vec!["app/other-1.0"]
        );
        assert_eq!(db.root().to_str(), Some("/root"));
    }

    struct FakeDb<'a> {
        root: &'a Path,
    }
    impl<'a> PackagesDb for FakeDb<'a> {
        fn installed_versions(&self, _c: &str, _p: &str) -> Vec<String> {
            vec!["1.0".to_string()]
        }
        fn contents_files(&self, _c: &str, _p: &str, _v: &str) -> Vec<String> {
            vec!["usr/lib/libx.a".to_string()]
        }
        fn reverse_dependents(&self, _c: &str, _p: &str, _v: &str) -> Vec<String> {
            vec!["app/other-1.0".to_string()]
        }
        fn root(&self) -> &Path {
            self.root
        }
    }

    /// `MergeEngine::execute` takes one [`MergeUnit`] under a
    /// [`MergeContext`]; a fake engine pins the seam.
    #[test]
    fn merge_engine_executes_one_unit_to_one_outcome() {
        struct FakeEngine;
        impl MergeEngine for FakeEngine {
            fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
                if unit.cpv == "dev-libs/broken-1.0" {
                    MergeOutcome::Failed("compile failed".to_string())
                } else {
                    MergeOutcome::Merged
                }
            }
        }
        let ctx = MergeContext {
            root: PathBuf::from("/root"),
            builddir: PathBuf::from("/var/tmp/portage"),
            jobs: 1,
            keep_going: false,
        };
        let engine = FakeEngine;
        let unit = MergeUnit {
            cpv: "dev-libs/example-1.0".to_string(),
            kind: MergeKind::Source,
            repo: Some("main".to_string()),
            root: PathBuf::from("/root"),
            replaces_same_slot: Some("0".to_string()),
            remote_binary: false,
            build_id: None,
            slot: Some("0".to_string()),
            sub_slot: None,
            use_flags: vec![("flag".to_string(), true)],
            progress: MergeProgress::single(),
        };
        assert_eq!(unit.kind, MergeKind::Source);
        assert_eq!(engine.execute(&unit, &ctx), MergeOutcome::Merged);
        let broken = MergeUnit {
            cpv: "dev-libs/broken-1.0".to_string(),
            kind: MergeKind::Binary,
            repo: None,
            root: PathBuf::from("/root"),
            replaces_same_slot: None,
            remote_binary: true,
            build_id: Some("1".to_string()),
            slot: None,
            sub_slot: None,
            use_flags: Vec::new(),
            progress: MergeProgress::single(),
        };
        assert!(matches!(
            engine.execute(&broken, &ctx),
            MergeOutcome::Failed(_)
        ));
    }

    /// The two engines split on the unit kind, exactly Portage
    /// `MergeListItem._start`'s dispatch (own kind `Skipped`, other `Failed`).
    #[test]
    fn merge_engine_source_and_binary_split_by_unit_kind() {
        let ctx = MergeContext {
            root: PathBuf::from("/root"),
            builddir: PathBuf::from("/var/tmp/portage"),
            jobs: 1,
            keep_going: false,
        };
        let source_unit = MergeUnit {
            cpv: "dev-libs/example-1.0".to_string(),
            kind: MergeKind::Source,
            repo: Some("main".to_string()),
            root: PathBuf::from("/root"),
            replaces_same_slot: None,
            remote_binary: false,
            build_id: None,
            slot: None,
            sub_slot: None,
            use_flags: Vec::new(),
            progress: MergeProgress::single(),
        };
        let binary_unit = MergeUnit {
            cpv: "dev-libs/example-1.0".to_string(),
            kind: MergeKind::Binary,
            repo: Some("main".to_string()),
            root: PathBuf::from("/root"),
            replaces_same_slot: None,
            remote_binary: false,
            build_id: None,
            slot: None,
            sub_slot: None,
            use_flags: Vec::new(),
            progress: MergeProgress::single(),
        };
        assert!(matches!(
            SourceMergeEngine.execute(&source_unit, &ctx),
            MergeOutcome::Skipped(_)
        ));
        assert!(matches!(
            SourceMergeEngine.execute(&binary_unit, &ctx),
            MergeOutcome::Failed(_)
        ));
        assert!(matches!(
            BinaryMergeEngine.execute(&binary_unit, &ctx),
            MergeOutcome::Skipped(_)
        ));
        assert!(matches!(
            BinaryMergeEngine.execute(&source_unit, &ctx),
            MergeOutcome::Failed(_)
        ));
    }
}
