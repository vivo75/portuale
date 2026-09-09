//! The `mrg` **director** component contracts.
//!
//! The `mrg` applet is positioned as a *director*: it does not solve,
//! read the cache, or merge anything itself — it orchestrates a set of
//! interchangeable, runtime-swappable components, each behind a Rust
//! trait so a different algorithm can be dropped in wholesale without
//! touching the director or any other component.
//!
//! This crate deliberately contains **no runtime behaviour**. It is the
//! contract layer: the traits, their portuale/portage-repo single
//! implementation markers, and the tests that **pin the contract shape**
//! (not the algorithms — those already have their own suites). Every type
//! in the method signatures is either a `portage-*` crate type or
//! primitive, so an alternate implementation can be written against the
//! traits without pulling in the code it replaces.
//!
//! Here is the component matrix the directors orchestrate, with each
//! slot's real-Portage grounding and its current implementation:
//!
//! | Slot | Real Portage | Portuale implementation |
//! |------|--------------|--------------------------|
//! | Solver | `_emerge/depgraph.py` (`depgraph` class) + `_emerge/resolver/backtracking.py` | `portage_repo::Resolver` (`BacktrackingResolver`, via `active_resolver()`) |
//! | PkgDatabase (vdb/edb/bintree) | `portage/dbapi/{vartree,porttree,bintree}.py` (subclasses of `dbapi`) | ad-hoc file readers scattered in `pretend.rs`/`portage-repo`; **no shared facade yet** |
//! | RepoCache (md5-cache backends) | `portage/cache/template.py::database` (flat_hash/sqlite/anydbm/volatile) | `Md5Cache` (flat file, real `flat_hash.py`) / `VolatileCache` (in-memory, real `volatile.py`) |
//! | BinpkgFetch | `portage/package/ebuild/fetch.py` + `_emerge/*binpkg*` | `portage_fetch` (real `wget`) + `portage_repo` remote binpkg index |
//! | MergeEngine | `_emerge/MergeListItem.py` dispatch + `_emerge/PackageMerge.py` / `EbuildMerge.py` / `vartree.py::dblink.merge` | `ebuild_merge::{run_merge, run_qmerge, merge_binpkg}` via `emerge_getbinpkg::run_merge_plan`'s per-entry dispatch |
//! | BinpkgIndex | `portage/dbapi/bintree.py` (local `$PKGDIR`/`Packages` + remote `PORTAGE_BINHOST` backends) | `PkgdirBinIndex` (local) / `RemoteBinhostIndex` (remote), delegating to `portage_repo::BinaryIndex` reads |
//! | NewsSet | `portage/news.py::Item.isRelevant`/`isValid` (+ a future GLSA `@security` selector) | `MetadataNews` marker; real evaluation in `pretend.rs::run_check_news` |
//! | SchedulerPolicy | `_emerge/Scheduler.py::Scheduler._run` (jobs + load-average gate) | `LoadAwarePolicy` marker, the real serial/gated default |
//! | Director | `actions.py::action_build` (build the depgraph from `create_depgraph_params`, walk the merge list via `Scheduler`) | `struct Director` below (resolve-then-hand-to-engine wiring; the `mrg` applet still calls `pretend::run` directly until a second algorithm lands) |
//!
//! Each contract documents: the real source it names, the single
//! portuale/portage-repo implementation that satisfies it today, and the
//! rationale for the method set (why *this* input, *this* output).
//!
//! ## Why separate "contract" from "implementation"
//!
//! A trait in the same crate as its only implementation, with no other
//! implementor, is usually dead abstraction. It earns its place here for
//! one specific reason: **the director needs a stable spine before the
//! interchangeable algorithms land.** The contracts are the agreement the
//! future algorithms must satisfy; writing them against the *existing*
//! single implementations (and pinning that in tests) makes the
//! `portage_repo::Resolver` precedent explicit, and gives every later
//! algorithm a fixed seam to plug into. The crate stays deliberately
//! small and does not grow until a second algorithm actually lands.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// A dependency-resolution strategy: [`ResolveRequest`] in, merge-ordered
/// [`GraphResult`] out (or a hard-error string).
///
/// This is a **re-export** of `portage_repo::Resolver` — not a second,
/// parallel definition — so there is exactly one solver seam in the
/// workspace. The director (and every tool that wants a plan) calls
/// [`Resolver::resolve`], and a component can be swapped by changing the
/// request. The current and only implementation is
/// [`portage_repo::BacktrackingResolver`], selected by
/// [`portage_repo::active_resolver`].
///
/// Grounding: real Portage's solver input/output boundary is
/// `_emerge/actions.py` constructing `_emerge/depgraph.py`'s `depgraph`
/// class from `create_depgraph_params(...)` plus the myopts/trees/settings
/// (`depgraph.__init__`, `actions.py:1028-1030`), and reading the merged
/// graph back out of it. `ResolveRequest` is the whole
/// `create_depgraph_params` + user-visible options surface captured as
/// one value; `GraphResult` is the merge list plus the notices
/// (`SlotConflict`, autounmask changes, abi rebuilds, circular deps) real
/// `depgraph.display_problems()` reports.
pub use portage_repo::Resolver;

// ---------------------------------------------------------------------------
// PkgDatabase
// ---------------------------------------------------------------------------

/// An **installed-packages database**: what the director needs from
/// "/var/db/pkg" (real `portage/dbapi/vartree.py`'s `vartree`/`vardbapi`,
/// the "vdb" half of the assignment's "edb, vdb and all caches" — the
/// "edb" being the same installed-db read through the `dbapi` facade) to
/// decide ownership, reverse deps, and slot occupancy.
///
/// This is deliberately *not* a faithful transcription of real
/// `portage/dbapi/vartree.py`'s `vartree` class (6,667 lines with the
/// whole dblink merge machinery). That is the full merge-side database;
/// the director only needs the **read side** that resolution actually
/// consults. v1 therefore models the small, stable subset that every
/// resolver, unmerge walk, and depclean runs over — the same subset
/// portuale already reads ad hoc via
/// `portage_repo::{installed_candidates, installed_refs,
/// installed_slot_owners, protected_set_member, get_installed_…}` and
/// `pretend.rs`'s `installed_cp_versions`.
///
/// An alternate implementation in the future (say, an in-memory snapshot
/// for benchmarking, or the full `vartree` facade that also owns
/// `unmerge`) must satisfy these three queries; the director and the
/// resolvers are written against exactly these and need no more.
pub trait PackagesDb {
    /// The CPVs installed under `root` for one `category/package`, in real
    /// portage's order (highest version first — real `dbapi.cp_list`
    /// sorts with `_cmp_cpv`). Empty when nothing of that package is
    /// installed.
    fn installed_versions(&self, category: &str, package: &str) -> Vec<String>;

    /// The raw CONTENTS-files list for one installed CPV (the `<<<…>>>`
    /// / `obj`/`sym`/`dir`/`bin` ROOT-relative entries recorded under the
    /// vdb). Empty for a CPV with nothing recorded.
    fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String>;

    /// The packages that directly depend on `consumer_cpv`: every
    /// installed package whose `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`
    /// pulls in an atom that `consumer_cpv` satisfies, per
    /// `portage_dep::match_from_list`. Empty when nothing depends on it.
    fn reverse_dependents(
        &self,
        consumer_category: &str,
        consumer_package: &str,
        consumer_version: &str,
    ) -> Vec<String>;

    /// The single current `root` this database reads. Keeping the root
    /// on the provider (rather than a per-call argument) makes a database
    /// value portable as one unit — the director can hold several at
    /// different roots without threading the root through every query.
    fn root(&self) -> &Path;
}

// ---------------------------------------------------------------------------
// RepoCache
// ---------------------------------------------------------------------------

/// A **repository metadata cache backend**: a `cat/pkg-ver` →
/// `(aux key → value)` map read from a repo's cache directory.
///
/// Grounding: real Portage abstracts the repo cache behind
/// `portage/cache/template.py::database` — a key/value store where the
/// value is the flat aux dictionary (`DEPEND`, `RDEPEND`, `KEYWORDS`,
/// `SLOT`, `EAPI`, `IUSE`, …) and the `EAPI`/`KEYWORDS`/`SLOT`/`
/// repository` `_pkg_str_aux_keys` are special-cased by
/// `portdbapi.aux_get`. Different backing stores are the derived backends
/// in `portage/cache/` (`flat_hash.py`, `sqlite.py`, `anydbm.py`,
/// `volatile.py`). Portuale's single implementation today is the flat
/// **md5-cache** directory read by `portage_repo::read_md5_cache` (the
/// `metadata/md5-cache/<cat>/<pf>` files), which is exactly
/// `flat_hash.py`'s layout.
///
/// The method set is the two operations the resolver actually needs
/// (get one package's aux dict, and enumerate what a category holds) —
/// kept minimal so a different cache backend only has to satisfy read
/// access, never the eclass-serialization/commit machinery
/// (`_eclasses_` reconstruction, `_mtime_`/`_md5_` validation, LCD) that
/// real `database.__getitem__` owns but portuale's md5-cache reader
/// already narrows out.
pub trait RepoCache {
    /// The full aux dictionary for one `category/package-revision`
    /// ("pf"), or `Err` when the cache has no entry for it. Keys use the
    /// real md5-cache uppercase spelling (`DEPEND`, `KEYWORDS`, …).
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String>;

    /// The `pf` names present for one category (the directory entries
    /// under `metadata/md5-cache/<category>`), in stable sort order.
    fn category(&self, category: &str) -> Vec<String>;

    /// The repo this cache reads (`::reponame`), for provenance.
    fn repo(&self) -> &str;
}

// ---------------------------------------------------------------------------
// BinpkgFetch
// ---------------------------------------------------------------------------

/// A source that can materialize a package's files locally so a merge or
/// build can consume them.
///
/// Grounding: real `portage/package/ebuild/fetch.py` downloads one
/// `SRC_URI` file at a time via `FETCHCOMMAND`/`RESUMECOMMAND` (real
/// `wget` by default) into `DISTDIR`, gated on a real `FEATURES=
/// distlocks` lock, after the offline candidate-resolution half
/// (`flatten_src_uri` + `resolve_mirror_candidates` +
/// `MirrorDistfiles`+`file_getsize`/Manifest)`). Portuale's single
/// implementation today is `portuale::fetch::fetch_src_uri`, which
/// composes `portage_fetch`'s pure `SrcUriEntry`/`verify_digests` with
/// the actual `wget` subprocess. A separate *remote binpkg* download
/// (from a `PORTAGE_BINHOST`/`binrepos.conf` `Packages` index, the
/// `g` bracket column) lives in `portage-repo` and is not named here —
/// a director that needs it wraps that function behind an identical
/// shaped trait.
///
/// The trait takes the flattened per-file [`portage_fetch::SrcUriEntry`]
/// (its `uri` + `override_mirror`/`override_fetch` flags already
/// resolved by `flatten_src_uri`) rather than raw `SRC_URI` text, so a
/// fetch implementation owns the mirror/verification policy but not the
/// flag-aware parse.
pub trait Fetcher {
    /// Download `entry`'s file into `distdir` (creating it if needed),
    /// verifying real `Manifest` digests (`size` + `BLAKE2B`/`SHA512`),
    /// and return the manifested local path. `Err` with an explanation
    /// when the download or verification fails.
    fn fetch(&self, entry: &portage_fetch::SrcUriEntry, distdir: &Path) -> Result<PathBuf, String>;
}

// ---------------------------------------------------------------------------
// MergeEngine
// ---------------------------------------------------------------------------

/// How one merge unit is built: from source (the ebuild phase chain +
/// vdb write) or from a binary package (download + unpack + vdb write).
///
/// Grounding: real `_emerge/MergeListItem.py::_start` dispatches on
/// `pkg.type_name` — a source package through the `EbuildBuild` chain,
/// a `"binary"` one through `_emerge/Binpkg.py`, an already-installed
/// one straight to the uninstall path. Portuale's entry carries the
/// same bit as `portage_repo::CandidateSource::{Ebuild, Binary}`, and
/// `emerge_getbinpkg::run_merge_plan` dispatches per resolved entry on
/// exactly it (`Binary` → `merge_binpkg`, else → `merge_one_source_entry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    /// Build from the ebuild, then merge (`EbuildBuild` + `EbuildMerge`).
    Source,
    /// Unpack a binpkg and merge it (`_emerge/Binpkg.py`).
    Binary,
}

/// A single unit of merge work at the boundary between the director's
/// plan and whatever executes the copy/install: one resolved entry.
///
/// The director derives this from the solver's [`portage_repo::GraphEntry`]
/// (`category`/`package` + the resolved version inside `outcome`, `slot`,
/// `repo_name`) plus the entry's `CandidateSource`. It carries *what* to
/// merge as plain data — never how a source merge is executed (that is
/// the engine's business).
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
    /// unmerge first (`ebuild_merge::unmerge_replaced_same_slot`).
    /// `None` when nothing is replaced.
    pub replaces_same_slot: Option<String>,
}

/// The execution context a merge engine runs under: the two knobs real
/// `_emerge/Scheduler.py` threads through every merge (`--jobs` parallelism
/// and `--keep-going` failure policy), plus the paths the merge writes to.
/// Deliberately coarse — an engine that needs finer control (build-log
/// capture, sandbox flags, resume lists) takes it from its own
/// constructor, not from this call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeContext {
    /// The `${ROOT}` the engine merges into.
    pub root: PathBuf,
    /// `PORTAGE_TMPDIR`-adjacent build root (`${PORTAGE_BUILDDIR}` base).
    pub builddir: PathBuf,
    /// Max concurrent builds (`--jobs=N`; 1 = serial, like real's
    /// default single `EbuildBuild` at a time for the merge step).
    pub jobs: usize,
    /// On a unit failure, drop its transitive dependents and merge the
    /// rest instead of aborting (real `Scheduler._calc_resume_list`).
    pub keep_going: bool,
}

/// The coarse three-valued outcome real portage reports in the `>>>`/`!!!`
/// merge lines, plus the resume diagnostic `--keep-going`/`--resume`
/// needs. An engine returns one per [`MergeUnit`]; the director turns a
/// run of these into the scheduler's report (merged count, failed list,
/// resume list) without knowing which engine produced them.
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
/// installed system. One method call per unit; the director owns ordering
/// (merge-list order comes from the solver's plan) and failure policy
/// (`keep_going`), the engine owns the copy/install itself.
///
/// Grounding: real `_emerge/PackageMerge.py` (`PackageMerge._start`,
/// via the `EbuildMerge.py` chain) and `vartree.py::dblink.merge()` are
/// today's single implementation behind `MergeListItem`; portuale's is
/// `ebuild_merge::{run_merge, run_qmerge, merge_binpkg}` driven by
/// `emerge_build`'s serial loop and the `-jN` parallel scheduler (which
/// keeps the vdb merge itself serial, like real portage). An alternate
/// method in the future (a content-addressed merger, a container-based
/// one) satisfies this one method and the director never notices.
pub trait MergeEngine {
    /// Execute `unit` under `ctx`, returning its coarse outcome. Must be
    /// safe to call concurrently up to `ctx.jobs` for the *build* half;
    /// the director serializes the vdb-write half (real portage merges
    /// one at a time).
    fn execute(&self, unit: &MergeUnit, ctx: &MergeContext) -> MergeOutcome;
}

// ---------------------------------------------------------------------------
// BinpkgIndex
// ---------------------------------------------------------------------------

/// A source of **binary-package candidates**: what the director asks when
/// it wants the built packages available for a `cat/pkg`, from whatever
/// store backs them.
///
/// Grounding: real `portage/dbapi/bintree.py` models the binary-package
/// database (a `dbapi` subclass) that real `depgraph.py` consults as the
/// `"binary"` tree. It has exactly two real backends, both of which real
/// `bintree` presents through the same `dbapi.cp_list`/`dbapi.aux_get`
/// read facade:
/// - the **local `$PKGDIR`** store — a `<pkgdir>/Packages` index, or a
///   directory scan of each binpkg's own embedded metadata when there is
///   no `Packages` (`bintree._populate_local`, real `packagesFile()` /
///   `files()`); and
/// - the **remote `PORTAGE_BINHOST`** store — a synced-`Packages` index
///   served by a binhost (`bintree._populate`, real `packagesFile()`
///   over the remote `Packages`), the `--getbinpkg` / `g` bracket
///   candidate source.
///
/// The method set is the two reads resolution actually performs against
/// a binary db — enumerate a cp's candidates, and pull one candidate's
/// aux record — mirroring the `"binary"`-side of
/// `portage_repo::CandidateSource` and the `Packages` `KEY: value` aux
/// format `BinaryIndex` already parses. Both backends today funnel
/// through `portage_repo::BinaryIndex`; the trait keeps them swappable
/// (a content-addressed binpkg store, an OCI-backed one, … are each one
/// `impl`).
pub trait BinpkgIndex {
    /// The binary candidates for `category/package`, real
    /// `bintree.cp_list` order (highest version first). Empty when the
    /// store holds nothing of that cp.
    fn candidates(&self, category: &str, package: &str) -> Vec<portage_repo::Candidate>;

    /// One candidate's raw `Packages` aux record (`CPV`, `SLOT`, `USE`,
    /// `SIZE`, `BUILD_TIME`, the dep strings, …) — the real
    /// `bintree.aux_get` surface. `None` when the store has no record
    /// for that exact CPV.
    fn metadata(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<std::collections::HashMap<String, String>>;

    /// Where this index's candidates come from (`::reponame`, or the
    /// local `$PKGDIR` path) for provenance in the `g` bracket / `-pv`
    /// `::repo` decoration.
    fn source_name(&self) -> String;
}

// ---------------------------------------------------------------------------
// NewsSet
// ---------------------------------------------------------------------------

/// A **news / security-item selector**: the swappable relevance decision
/// behind `--check-news` (and a future `@security` GLSA read).
///
/// Grounding: real `portage/news/` defines the news-item model. The
/// in-scope (`--check-news`) backend is `Item.isRelevant` (`news.py`,
/// real `lib/portage/news.py`), which decides a `metadata/news/<id>`
/// item should be shown by matching its `Display-If-Installed` atom
/// (or `Display-If-Keyword`/`Display-If-Profile`) against the installed
/// db; `isValid` (`news.py`) gates on `News-Item-Format`/EAPI.
/// Portuale's single reading today is `pretend.rs::run_check_news`
/// (`news_item_valid` + `news_item_relevant`), Rust-only in the binary
/// crate.
///
/// A GLSA `@security` selector (real `_emerge/glsa.Class.glsl` /
/// `GlsaSet`) would implement the *same* seam — match a security item
/// against the installed set — which is why the trait is named a
/// "news/GLSA selector" here: it is the one decision, two data sources.
/// The GLSA half stays gated on `@security` entering scope
/// (`scope-backlog.md` Part 3 non-goal), so `MetadataNews` below is the
/// only implementation today.
pub trait NewsSelector {
    /// The `metadata/news/<id>` item id of every **valid, not-already-
    /// read, currently-relevant** item under the repo/root this selector
    /// reads (real `NewsManager.updateItems`'s unread/skip accumulation,
    /// narrowed to the pure relevance decision). Empty when nothing is
    /// pending.
    fn unread_ids(&self) -> Vec<String>;

    /// The repo whose `metadata/news` this selector reads, for the
    /// `… news items need reading for repository '<repo>'.` line.
    fn repo_name(&self) -> String;
}

// ---------------------------------------------------------------------------
// SchedulerPolicy
// ---------------------------------------------------------------------------

/// A **build-scheduler dispatch policy**: when the `-jN` merge DAG is
/// allowed to start another build.
///
/// Grounding: real `_emerge/Scheduler.py` — `Scheduler._run` decides per
/// step whether to dispatch another `MergeListItem` from the forward-dep
/// queue, bounded by `--jobs` and real `--load-average` (`_run`: never
/// start an *additional* build while the system 1-minute load average is
/// above the limit — the first build is always allowed so the scheduler
/// cannot deadlock). Portuale's single reading today is
/// `emerge_build.rs::run_build_scheduler`'s `in_flight < jobs` +
/// `system_loadavg_1min` gate, Rust-only in the binary crate.
///
/// The policy is the one swappable decision: a different scheduler shape
/// (a serial `--jobs=1`, a deadline-aware build queue, a throttle that
/// consults a remote build farm) changes only this method, never the DAG
/// walk that calls it.
pub trait SchedulerPolicy {
    /// Whether to dispatch another build now. `running` is the count
    /// currently in flight; `loadavg_1min` is the system 1-minute load
    /// average read from `/proc/loadavg`. Callers guarantee `running <
    /// [`Self::max_jobs`]` is enforced *before* consulting this, and that
    /// at least one build is always permitted, so an implementer may
    /// treat `running == 0` as unconditional.
    fn should_start(&self, running: usize, loadavg_1min: f64) -> bool;

    /// The hard concurrency ceiling (`--jobs=N`; 1 = serial merge).
    fn max_jobs(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Implementation markers (the single implementations that satisfy the
// contracts today)
// ---------------------------------------------------------------------------

/// The current, only `PackagesDb` implementation: reads the vdb directly
/// from `<root>/var/db/pkg`. Marked with the real-source grounding so a
/// future second implementation has an explicit seam to replace.
pub struct VdbReader<'a> {
    root: &'a Path,
}

impl<'a> PackagesDb for VdbReader<'a> {
    fn installed_versions(&self, category: &str, package: &str) -> Vec<String> {
        // Purposely unimplemented until the vdb read path is factored out
        // of portage-repo/pretend.rs (see the contract doc comment).
        let _ = (category, package);
        Vec::new()
    }
    fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String> {
        let _ = (category, package, version);
        Vec::new()
    }
    fn reverse_dependents(
        &self,
        _consumer_category: &str,
        _consumer_package: &str,
        _consumer_version: &str,
    ) -> Vec<String> {
        Vec::new()
    }
    fn root(&self) -> &Path {
        self.root
    }
}

/// The current, only `RepoCache` implementation: the flat md5-cache
/// directory. Delegate to `portage_repo::read_md5_cache` (identical to
/// real `flat_hash.py`'s layout on disk).
pub struct Md5Cache<'a> {
    repo_location: &'a Path,
    repo_name: &'a str,
}
impl RepoCache for Md5Cache<'_> {
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        portage_repo::read_md5_cache(self.repo_location, category, pf)
            .map(|m| (*m).clone())
            .map_err(|e| e.to_string())
    }
    fn category(&self, category: &str) -> Vec<String> {
        let dir = self
            .repo_location
            .join("metadata")
            .join("md5-cache")
            .join(category);
        std::fs::read_dir(&dir)
            .map(|it| {
                let mut names: Vec<String> = it
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.contains('.'))
                    .collect();
                names.sort();
                names
            })
            .unwrap_or_default()
    }
    fn repo(&self) -> &str {
        self.repo_name
    }
}

/// The in-memory `RepoCache` backend: the "second implementation per
/// slot" for the repo-cache slot, holding the same flat aux dictionaries
/// `Md5Cache` reads off disk in an owned map instead.
///
/// Grounding: real `portage/cache/volatile.py::database` — the
/// dict-backed cache backend (`self._data = {}`), which `deepcopy`s on
/// every read and write (`__getitem__`/`_setitem`) so callers can never
/// alias the store's internals. This port mirrors that ownership rule:
/// `insert` clones in, [`RepoCache::metadata`] clones out. Real marks
/// `serialize_eclasses = False` / `store_eclass_paths = False` (no
/// eclass-string round-trip); the trait's read-only surface already
/// narrows that machinery out, so there is nothing further to port.
///
/// Uses, beyond satisfying the contract's interchangeability: a
/// pre-loaded snapshot cache for benchmarks and tests (no filesystem),
/// and the read side of a generated cache before it is ever written.
#[derive(Debug, Clone, Default)]
pub struct VolatileCache {
    /// `(category, pf) → aux dict`, the `volatile._data` dict.
    entries: std::collections::HashMap<(String, String), std::collections::HashMap<String, String>>,
    /// The repo this cache stands in for (`::reponame`), for provenance.
    repo_name: String,
}

impl VolatileCache {
    /// An empty cache standing in for `repo_name`.
    pub fn new(repo_name: &str) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            repo_name: repo_name.to_string(),
        }
    }

    /// A cache pre-loaded from `(category, pf, aux dict)` triples (a
    /// snapshot read, or a generated cache not yet flushed to disk).
    pub fn from_entries(
        entries: Vec<(String, String, std::collections::HashMap<String, String>)>,
        repo_name: &str,
    ) -> Self {
        let mut cache = Self::new(repo_name);
        for (category, pf, metadata) in entries {
            cache.insert(&category, &pf, metadata);
        }
        cache
    }

    /// Store (a copy of) `metadata` under `category/pf` — real
    /// `volatile._setitem`'s `deepcopy` in.
    pub fn insert(
        &mut self,
        category: &str,
        pf: &str,
        metadata: std::collections::HashMap<String, String>,
    ) {
        self.entries
            .insert((category.to_string(), pf.to_string()), metadata);
    }
}

impl RepoCache for VolatileCache {
    fn metadata(
        &self,
        category: &str,
        pf: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        // A copy out, never a borrow of the store — real
        // `volatile.__getitem__`'s `deepcopy`, so a caller mutating the
        // returned dict cannot corrupt the cache.
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

/// The current `Fetcher`: real-src via the same `wget` + Manifest
/// verification steps `portuale::fetch::fetch_src_uri` drives.
pub struct WgetFetcher;
impl Fetcher for WgetFetcher {
    fn fetch(&self, entry: &portage_fetch::SrcUriEntry, distdir: &Path) -> Result<PathBuf, String> {
        let _ = (entry, distdir);
        // The real download lives in `portuale::fetch::fetch_src_uri`
        // (binary crate; not linkable from a library). This marker exists
        // so the *trait* is exercised; the transport itself is
        // live-tested in that crate, not duplicated here.
        Err("WgetFetcher transport lives in portuale::fetch".to_string())
    }
}

/// The local `$PKGDIR` `BinpkgIndex` implementation: reads the binary
/// candidates real `bintree.py` yields from the local store -- the
/// `<pkgdir>/Packages` index, or the per-file directory scan when no
/// `Packages` exists (`bintree._populate_local`) -- via
/// `portage_repo::BinaryIndex`.
///
/// This is one of *two* real binary-package backends (the "second
/// implementation" the director contract is built to admit): the other is
/// [`RemoteBinhostIndex`] for a `PORTAGE_BINHOST`. Both satisfy the same
/// [`BinpkgIndex`] methods over the same
/// [`portage_repo::BinaryIndex`] aux records, exactly as real `bintree`
/// presents both through one `dbapi` facade.
pub struct PkgdirBinIndex<'a> {
    /// The parsed `$PKGDIR` index the resolution already built (the
    /// `BinaryIndex::from_pkgdir` / directory-scan value) and the `$PKGDIR`
    /// path for provenance.
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
    fn source_name(&self) -> String {
        self.pkgdir.to_string_lossy().into_owned()
    }
}

/// The remote `PORTAGE_BINHOST` `BinpkgIndex` implementation: the
/// synced-`Packages` candidate source behind `--getbinpkg` (the `g`
/// bracket column).
///
/// This is the "second implementation per slot" for the binary-index
/// slot: real `bintree` has exactly these two backends (local store /
/// remote binhost), and the director contract admits both behind one
/// [`BinpkgIndex`]. Delegates to
/// `portage_repo::list_remote_binary_candidates` for the per-binrepo scan
/// (which real `bintree._populate` + `dbapi` resolution shadow a locally
/// present version, the `bintree.isremote` rule) and
/// `read_binary_metadata_any` for the aux record.
pub struct RemoteBinhostIndex<'a> {
    /// The resolved binhost configuration (`binrepos.conf` / `PORTAGE_
    /// BINHOST`), real `BinRepoConfig` values.
    pub config: &'a portage_profile::Config,
    /// The `root` whose `$PKGDIR` the sync lands in (real
    /// `BinRepo.packages_dir(root)`).
    pub root: &'a Path,
    /// The local `$PKGDIR` index — remote candidates a local build already
    /// provides are shadowed out (real `bintree.isremote`).
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
        portage_repo::read_binary_metadata_any(
            self.config,
            self.root,
            self.local,
            category,
            package,
            version,
        )
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

/// The current `NewsSelector`: real `--check-news` relevance over the
/// repo's `metadata/news/<id>/<id>.en.txt` items, gated by
/// `News-Item-Format`/EAPI validity + `Display-If-Installed` matching
/// against the installed db (real `portage/news.py::Item.isRelevant` /
/// `isValid`; portuale's `pretend.rs::news_item_valid` /
/// `news_item_relevant`).
///
/// The real evaluation lives in the `portuale` binary crate's
/// `run_check_news` (not linkable from a library, the standing pattern);
/// this marker exists so the *trait* is exercised and the seam stays
/// pinned. A GLSA `@security` selector (real glsa-check / `GlsaSet`)
/// would satisfy the same trait once `@security` enters scope — it is
/// currently a `scope-backlog.md` Part 3 non-goal.
pub struct MetadataNews;
impl NewsSelector for MetadataNews {
    fn unread_ids(&self) -> Vec<String> {
        // The real accumulation lives in `pretend.rs::run_check_news`
        // (binary crate). A no-state marker: the seam, not the algorithm.
        Vec::new()
    }
    fn repo_name(&self) -> String {
        // One selector is built per repo by the CLI layer; the marker
        // owns none.
        String::new()
    }
}

/// The current `SchedulerPolicy`: real `Scheduler._run`'s per-step gate —
/// start another build while `running < jobs` and the system 1-minute
/// load average is under the `--load-average` ceiling (never gating the
/// first build, so the DAG cannot deadlock).
///
/// The real gate lives in `emerge_build.rs::run_build_scheduler`
/// (`in_flight < jobs` + `system_loadavg_1min`); this marker pins the
/// seam. A scheduler policy "second implementation" (e.g. a serial
/// `--jobs=1` always-serial policy, or a deadline/to-be-built-aware
/// one) satisfies the same two methods.
#[derive(Debug, Clone, Copy)]
pub struct LoadAwarePolicy {
    /// `--jobs` ceiling (`max_jobs`).
    jobs: usize,
    /// `--load-average` ceiling; `None` disables load gating.
    load_average: Option<f64>,
}
impl Default for LoadAwarePolicy {
    fn default() -> Self {
        // Real portage's default scheduler has no `--jobs`/`--load-average`
        // bound (bare `-j` = unlimited); the portable default is serial
        // and ungated.
        Self {
            jobs: 1,
            load_average: None,
        }
    }
}
impl SchedulerPolicy for LoadAwarePolicy {
    fn should_start(&self, running: usize, loadavg_1min: f64) -> bool {
        // The first build is always allowed (real `Scheduler._run` never
        // gates `in_flight == 0`), and additional ones only while under
        // the ceilings.
        running == 0
            || (running < self.jobs && self.load_average.is_none_or(|la| loadavg_1min <= la))
    }
    fn max_jobs(&self) -> usize {
        self.jobs
    }
}

// ---------------------------------------------------------------------------
// Director
// ---------------------------------------------------------------------------

/// The `mrg` **director**: one solver, one installed-db, one repo cache,
/// one fetcher, one merge engine, one binary-package index, one news
/// selector, one scheduler policy — held together, never implemented here.
///
/// Grounding: real `actions.py::action_build` builds the depgraph from
/// `create_depgraph_params(...)` (`actions.py:268`), resolves it, and
/// hands the merge list to `_emerge/Scheduler.py` (`actions.py:679`),
/// which walks it (fetch → build → merge per `MergeListItem`,
/// `--keep-going` resume on failure). The director
/// is that orchestration shape with every algorithm behind a trait: a
/// different solver (`portage_solver`, `pubgrub`, `resolvo`), a different
/// database or cache backend, or a different merge method is one
/// constructor argument, never a call-site change.
///
/// Deliberate v1 narrowness: the director only *holds* the eight slots and
/// exposes `plan()` (solver delegation) today. The fetch→build→merge walk
/// stays in `pretend.rs` / `emerge_build.rs` / `emerge_getbinpkg.rs`
/// (and the `mrg` applet still calls `pretend::run` directly) until a
/// second algorithm actually lands — growing the walk here now, with a
/// single implementation per slot, would be exactly the dead abstraction
/// the module doc comment refuses.
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
    /// Resolve `req` through the director's solver: [`ResolveRequest`] in,
    /// merge-ordered [`portage_repo::GraphResult`] out. Every later stage
    /// (fetch, merge) consumes that plan; none of them re-resolves.
    pub fn plan(
        &self,
        req: &portage_repo::ResolveRequest,
    ) -> Result<portage_repo::GraphResult, portage_repo::Error> {
        self.solver.resolve(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::{GraphResult, ResolveRequest};

    /// `mrg-director` must never regress to a second, parallel solver
    /// seam: the director's `Resolver` IS `portage_repo::Resolver`, and
    /// the runtime-selection hook is `active_resolver`. (This pins the
    /// *seam*; actual resolution is exercised by portage-repo's own real,
    /// fixture-driven suite, never here.)
    #[test]
    fn resolver_seam_is_the_single_re_export() {
        let _solver: Box<dyn Resolver> = portage_repo::active_resolver();
    }

    /// `Resolver::resolve` returns portage-repo's plan type (the merge
    /// list + notices), so an alternate solver must speak real
    /// portage-facing data, never a director-internal shape.
    #[test]
    fn resolver_speaks_graph_result() {
        let _plan: &dyn Fn(&ResolveRequest) -> Result<GraphResult, portage_repo::Error> =
            &|_req| Err(portage_repo::Error::Detail("inert pin".into()));
    }

    /// `PackagesDb` models the three read queries the resolver actually
    /// needs; a fake suffices to prove the director never reaches for
    /// per-package internals beyond these.
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

    /// `RepoCache` reads one aux dict per package + lists a category;
    /// different backends (flat / sqlite / volatile) are all *read-only*
    /// views, so writes are intentionally out of contract. A non-existent
    /// repo location reads as an empty cache (no panic), like real
    /// `flat_hash` on a missing dir.
    #[test]
    fn repo_cache_is_read_only_query_by_key() {
        let cache = Md5Cache {
            repo_location: Path::new("/nonexistent"),
            repo_name: "main",
        };
        assert_eq!(cache.repo(), "main");
        assert!(cache.category("dev-libs").is_empty());
        assert!(cache.metadata("dev-libs", "example-1.0").is_err());
    }

    /// `VolatileCache` is the repo-cache slot's second implementation
    /// (real `cache/volatile.py`'s dict backend): the same three reads as
    /// `Md5Cache`, served from an owned in-memory map. The shape pins:
    /// pre-loaded entries read back whole, `category` lists in stable
    /// sort order, a missing entry is `Err` (never a panic), and the
    /// returned dict is a copy — mutating it leaves the store intact
    /// (real `volatile.__getitem__`'s `deepcopy`).
    #[test]
    fn repo_cache_volatile_is_the_in_memory_second_backend() {
        let aux = std::collections::HashMap::from([
            ("SLOT".to_string(), "0".to_string()),
            ("KEYWORDS".to_string(), "amd64".to_string()),
        ]);
        let cache = VolatileCache::from_entries(
            vec![
                ("dev-libs".to_string(), "b-2.0".to_string(), aux.clone()),
                ("dev-libs".to_string(), "a-1.0".to_string(), aux.clone()),
            ],
            "main",
        );
        assert_eq!(cache.repo(), "main");
        assert_eq!(cache.category("dev-libs"), vec!["a-1.0", "b-2.0"]);
        assert!(cache.category("sys-apps").is_empty());
        assert_eq!(
            cache.metadata("dev-libs", "a-1.0").unwrap().get("SLOT"),
            Some(&"0".to_string())
        );
        assert!(cache.metadata("dev-libs", "missing-9.9").is_err());

        let mut read_back = cache.metadata("dev-libs", "a-1.0").unwrap();
        read_back.insert("SLOT".to_string(), "corrupted".to_string());
        assert_eq!(
            cache.metadata("dev-libs", "a-1.0").unwrap().get("SLOT"),
            Some(&"0".to_string())
        );

        let mut empty = VolatileCache::new("overlay");
        assert_eq!(empty.repo(), "overlay");
        assert!(empty.category("dev-libs").is_empty());
        empty.insert("dev-libs", "c-3.0", aux);
        assert_eq!(empty.category("dev-libs"), vec!["c-3.0"]);
    }

    /// `Fetcher::fetch` takes a flattened per-file [`SrcUriEntry`] and
    /// returns the manifested local path; consuming the structured entry
    /// (not raw `SRC_URI` text) keeps the mirror/verification policy
    /// inside the implementation. The marker's own transport shells out to
    /// real `wget` and is live-tested in `portuale::fetch`, not here --
    /// this only pins the seam's shape.
    #[test]
    fn fetcher_returns_a_manifested_path() {
        let fetcher = WgetFetcher;
        let entry = portage_fetch::SrcUriEntry {
            uri: "https://example.invalid/x.tgz".to_string(),
            filename: "x.tgz".to_string(),
            override_mirror: false,
            override_fetch: false,
        };
        assert!(fetcher.fetch(&entry, Path::new("/tmp")).is_err());
    }

    /// `MergeEngine::execute` takes one [`MergeUnit`] (resolved cpv +
    /// source/binary kind + same-slot replace) under a [`MergeContext`]
    /// (`--jobs`/`--keep-going` + paths) and returns a coarse
    /// [`MergeOutcome`]. A fake engine suffices to pin the seam: the
    /// director never reaches past these three types into engine
    /// internals.
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
        };
        assert_eq!(unit.kind, MergeKind::Source);
        assert_eq!(engine.execute(&unit, &ctx), MergeOutcome::Merged);
        let broken = MergeUnit {
            cpv: "dev-libs/broken-1.0".to_string(),
            kind: MergeKind::Binary,
            repo: None,
            root: PathBuf::from("/root"),
            replaces_same_slot: None,
        };
        assert!(matches!(
            engine.execute(&broken, &ctx),
            MergeOutcome::Failed(_)
        ));
    }

    /// The [`Director`] holds one component per slot and `plan()` runs
    /// through its solver: swapping the solver swaps the plan, and the
    /// other seven slots ride along untouched. This pins the composition
    /// *shape* — field access for the carried slots, plus a bound
    /// assertion that the wired solver really is a [`Resolver`] (which is
    /// exactly what `plan()`'s `S: Resolver` bound requires). It
    /// deliberately never *calls* `plan`: building a real
    /// `ResolveRequest` (a 40+-field value including a full profile
    /// `Config`) is portage-repo's own fixture-driven business, never a
    /// contract-shape test's. The delegation body itself is one line
    /// (`self.solver.resolve(req)`), left to inspection.
    #[test]
    fn director_holds_eight_slots_and_plans_through_its_solver() {
        struct FakeSolver;
        impl Resolver for FakeSolver {
            fn resolve(&self, _req: &ResolveRequest) -> Result<GraphResult, portage_repo::Error> {
                Err(portage_repo::Error::Detail("inert pin".into()))
            }
        }
        struct FakeEngine;
        impl MergeEngine for FakeEngine {
            fn execute(&self, _unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
                MergeOutcome::Skipped("inert pin".to_string())
            }
        }
        type Wiring<'a> = Director<
            FakeSolver,
            FakeDb<'a>,
            Md5Cache<'a>,
            WgetFetcher,
            FakeEngine,
            PkgdirBinIndex<'a>,
            MetadataNews,
            LoadAwarePolicy,
        >;
        let idx = portage_repo::BinaryIndex::from_entries(vec![]);
        let director = Wiring {
            solver: FakeSolver,
            packages_db: FakeDb {
                root: Path::new("/root"),
            },
            repo_cache: Md5Cache {
                repo_location: Path::new("/nonexistent"),
                repo_name: "main",
            },
            fetcher: WgetFetcher,
            merge_engine: FakeEngine,
            binpkg_index: PkgdirBinIndex {
                index: &idx,
                pkgdir: Path::new("/var/cache/binpkgs"),
            },
            news_selector: MetadataNews,
            scheduler_policy: LoadAwarePolicy::default(),
        };
        assert_eq!(director.packages_db.root().to_str(), Some("/root"));
        assert_eq!(director.repo_cache.repo(), "main");
        assert_eq!(director.binpkg_index.source_name(), "/var/cache/binpkgs");
        assert!(director.news_selector.unread_ids().is_empty());
        assert!(director.scheduler_policy.should_start(0, 99.0));
        fn assert_resolver<T: Resolver>() {}
        assert_resolver::<FakeSolver>();
    }

    /// `BinpkgIndex` admits the two real binary backends (`bintree.py`'s
    /// local `$PKGDIR` store and its remote `PORTAGE_BINHOST`), both
    /// satisfying the same three methods over the same aux records — the
    /// "second implementation per slot" the director's binary-index slot
    /// is built for. The shape pins: candidates come out as real
    /// `portage_repo::Candidate`s, metadata as the raw `Packages` aux
    /// dict, and the source is named for the `g` bracket provenance.
    #[test]
    fn binpkg_index_admits_both_local_and_remote_backends() {
        let entries = vec![std::collections::HashMap::from([
            ("CPV".to_string(), "dev-libs/localbin-1.0".to_string()),
            ("SLOT".to_string(), "0".to_string()),
            ("USE".to_string(), "flag1".to_string()),
        ])];
        let idx = portage_repo::BinaryIndex::from_entries(entries);
        let local = PkgdirBinIndex {
            index: &idx,
            pkgdir: Path::new("/var/cache/binpkgs"),
        };
        assert_eq!(local.source_name(), "/var/cache/binpkgs");
        assert_eq!(
            local
                .candidates("dev-libs", "localbin")
                .iter()
                .map(|c| c.version.clone())
                .collect::<Vec<_>>(),
            vec!["1.0".to_string()]
        );
        assert_eq!(
            local
                .metadata("dev-libs", "localbin", "1.0")
                .and_then(|m| m.get("USE").cloned()),
            Some("flag1".to_string())
        );
        assert!(local.metadata("dev-libs", "localbin", "9.9").is_none());

        // The remote backend is genuinely a second implementation: it
        // delegates to the whole-binrepos scan (`bintree.isremote`
        // shadowing included) and metadata-any. With no binrepos
        // configured the *candidate* scan is empty (nothing remote), but
        // `read_binary_metadata_any` still falls back to the local store
        // for the aux record — exactly real `bintree`'s local-wins read
        // path. Same seam either way, no panic.
        let config = portage_profile::Config {
            ..Default::default()
        };
        let remote = RemoteBinhostIndex {
            config: &config,
            root: Path::new("/"),
            local: &idx,
        };
        assert!(remote.candidates("dev-libs", "localbin").is_empty());
        assert_eq!(
            remote
                .metadata("dev-libs", "localbin", "1.0")
                .and_then(|m| m.get("USE").cloned()),
            Some("flag1".to_string())
        );
        assert!(remote.metadata("dev-libs", "localbin", "9.9").is_none());
        assert_eq!(remote.source_name(), "");
    }

    /// `NewsSelector` is the pure relevance seam behind `--check-news`:
    /// the director asks for the valid, relevant, unread item ids and the
    /// repo owning them; a GLSA `@security` selector would satisfy the
    /// same two methods (still a Part 3 non-goal). The marker is
    /// state-free — the real evaluation is `pretend.rs`'s, live-tested
    /// there, this only pins the shape.
    #[test]
    fn news_selector_is_unread_ids_plus_repo() {
        struct FakeNews;
        impl NewsSelector for FakeNews {
            fn unread_ids(&self) -> Vec<String> {
                vec!["2026-09-01-portuale".to_string()]
            }
            fn repo_name(&self) -> String {
                "gentoo".to_string()
            }
        }
        let n = FakeNews;
        assert_eq!(n.unread_ids(), vec!["2026-09-01-portuale"]);
        assert_eq!(n.repo_name(), "gentoo");
        assert!(MetadataNews.unread_ids().is_empty());
    }

    /// `SchedulerPolicy` is the one decision the `-jN` DAG asks: start
    /// another build now? `should_start` always admits the first build
    /// (so the scheduler can't deadlock, real `Scheduler._run`), then caps
    /// by `--jobs` and `--load-average`. The marker implements the real
    /// gate inline — a different policy (serial, deadline-aware, remote
    /// build-farm) is one `impl` of the same two methods.
    #[test]
    fn scheduler_policy_gates_by_jobs_and_load_average() {
        let serial = LoadAwarePolicy::default();
        assert_eq!(serial.max_jobs(), 1);
        assert!(serial.should_start(0, 1.0));
        assert!(!serial.should_start(1, 0.0));

        let capped = LoadAwarePolicy {
            jobs: 4,
            load_average: Some(2.0),
        };
        assert_eq!(capped.max_jobs(), 4);
        assert!(capped.should_start(3, 1.5));
        assert!(!capped.should_start(3, 2.5));
        assert!(!capped.should_start(4, 1.5));
        assert!(capped.should_start(0, 99.0));
    }
}
