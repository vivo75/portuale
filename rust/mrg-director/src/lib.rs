//! The `mrg` **director** component contracts.
//!
//! The `mrg` applet is positioned as a *director*: it does not solve,
//! read the cache, or merge anything itself — it orchestrates a set of
//! interchangeable, runtime-swappable components, each behind a Rust
//! trait so a different algorithm can be dropped in wholesale without
//! touching the director or any other component.
//!
//! This crate is the contract layer **plus the filesystem-backed
//! implementations the production paths run through**: the traits, their
//! portuale/portage-repo implementations, the `Director` wiring struct
//! (whose delegation methods route every stage through its slots), and
//! the tests that pin both the contract shape and the backed
//! implementations against real on-disk layouts (a temp vdb, a temp
//! md5-cache dir, a temp distdir). Every type in the method signatures
//! is either a `portage-*` crate type or primitive, so an alternate
//! implementation can be written against the traits without pulling in
//! the code it replaces.
//!
//! Here is the component matrix the directors orchestrate, with each
//! slot's real-Portage grounding and its current implementation:
//!
//! | Slot | Real Portage | Portuale implementation |
//! |------|--------------|--------------------------|
//! | Solver | `_emerge/depgraph.py` (`depgraph` class) + `_emerge/resolver/backtracking.py` | `portage_repo::Resolver` (`BacktrackingResolver`, via `active_resolver()`) |
//! | PkgDatabase (vdb/edb/bintree) | `portage/dbapi/{vartree,porttree,bintree}.py` (subclasses of `dbapi`) | `VdbReader` (filesystem vdb read side, live on the resolve/unmerge paths) / `MemoryDb` (in-memory snapshot, real `FakeVartree.py`) |
//! | RepoCache (md5-cache backends) | `portage/cache/template.py::database` (flat_hash/sqlite/anydbm/volatile) | `Md5Cache` (flat file, real `flat_hash.py`) / `VolatileCache` (in-memory, real `volatile.py`) |
//! | BinpkgFetch | `portage/package/ebuild/fetch.py` + `_emerge/*binpkg*` | `WgetFetcher` (real `wget` transport, shared `portage_fetch::download_via_wget` with `portuale::fetch`) + `portage_repo` remote binpkg index |
//! | MergeEngine | `_emerge/MergeListItem.py` dispatch + `_emerge/PackageMerge.py` / `EbuildMerge.py` / `vartree.py::dblink.merge` | `SourceMergeEngine` (`"ebuild"` arm) / `BinaryMergeEngine` (`"binary"` arm) kind routing + the binary crate's `RealSourceEngine`/`RealBinaryEngine` adapters executing through the seam |
//! | BinpkgIndex | `portage/dbapi/bintree.py` (local `$PKGDIR`/`Packages` + remote `PORTAGE_BINHOST` backends) | `PkgdirBinIndex` (local) / `RemoteBinhostIndex` (remote), delegating to `portage_repo::BinaryIndex` reads |
//! | NewsSet | `portage/news.py::Item.isRelevant`/`isValid` (+ a future GLSA `@security` selector) | `FilesystemNews` in the binary crate (real evaluation over `metadata/news`, live on the `--check-news` path); `MetadataNews` stays the state-free shape pin |
//! | SchedulerPolicy | `_emerge/Scheduler.py::Scheduler._run` (jobs + load-average gate) | `LoadAwarePolicy` (`--jobs=N`) / `UnlimitedPolicy` (bare `-j`, real `max_jobs is True`); `run_build_scheduler` runs under one of them |
//! | Director | `actions.py::action_build` (build the depgraph from `create_depgraph_params`, walk the merge list via `Scheduler`) | `struct Director` below (resolve-then-hand-to-engine wiring; merge dispatch and news evaluation run through its slots) |
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
//! implementations (and pinning that in tests) makes the
//! `portage_repo::Resolver` precedent explicit, and gives every later
//! algorithm a fixed seam to plug into. Slots whose production path runs
//! through the trait (`SchedulerPolicy` via `run_build_scheduler`,
//! `MergeEngine` via the merge dispatch, `NewsSelector` via
//! `--check-news`, `PackagesDb` via the unmerge/depclean reads) prove the
//! seam carries real traffic; the rest stay swappable behind `Director`'s
//! delegation methods until their second algorithm lands.

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

    /// The paths one installed CPV owns: its vdb `CONTENTS`
    /// `obj`/`sym`/`dir`/`dev`/`fif`/`bin` entries' path fields, with one
    /// leading `/` stripped (the vdb records `${ROOT}`-absolute paths,
    /// the seam speaks `${ROOT}`-relative ones). Empty for a CPV with
    /// nothing recorded.
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
/// `MirrorDistfiles`+`file_getsize`/Manifest)`). Portuale's
/// implementation is [`WgetFetcher`] below, running the shared
/// `portage_fetch::download_via_wget` transport (the same `FETCHCOMMAND`
/// `portuale::fetch::fetch_src_uri`'s own candidate loop runs). A
/// separate *remote binpkg* download (from a `PORTAGE_BINHOST`/
/// `binrepos.conf` `Packages` index, the `g` bracket column) lives in
/// `portage-repo` and is not named here — a director that needs it wraps
/// that function behind an identical shaped trait.
///
/// The trait takes the flattened per-file [`portage_fetch::SrcUriEntry`]
/// (its `uri` + `override_mirror`/`override_fetch` flags already
/// resolved by `flatten_src_uri`) rather than raw `SRC_URI` text, so a
/// fetch implementation owns the mirror/verification policy but not the
/// flag-aware parse.
///
/// Single by design (no second implementation planned): real has a
/// second fetch method -- the local-`fsmirror` copy (`fetch.py:1503`,
/// `/`-rooted `custommirrors["local"]`/`GENTOO_MIRRORS` dirs tried via
/// `shutil.copyfile` before any `FETCHCOMMAND`) -- but it verifies
/// against Manifest digests resolved outside this seam, and this trait
/// deliberately passes no Manifest context (only `entry` + `distdir`),
/// so no second transport can satisfy the verification clause from
/// inside a library crate. The optimization itself is out of scope in
/// portuale too (`resolve_mirror_candidates` documents the cut), so
/// there is nothing to factor out behind this seam either. (What *is*
/// newly real here versus the old marker: the download half. Manifest
/// digest verification stays at the `fetch_src_uri` call site, which
/// holds the `Manifest` entry -- see [`WgetFetcher`].)
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
    /// A binary unit whose binpkg is not under local `$PKGDIR` may be
    /// fetched from a binhost (`GraphEntry::remote_binary`, set by the
    /// resolver for a binhost-sourced candidate). Source units ignore it.
    pub remote_binary: bool,
    /// A build-id-qualified binary (`GraphEntry::build_id`): selects the
    /// exact `$PKGDIR` file among multi-instance same-version binpkgs.
    /// `None` means "the unqualified file". Source units ignore it.
    pub build_id: Option<String>,
    /// The resolved slot/sub-slot (`GraphEntry::slot`/`sub_slot`): the
    /// source engine threads them into the build-phase env (slot-qualified
    /// `package.env` matching). `None` behaves like the resolver's own
    /// `unwrap_or("0")` default.
    pub slot: Option<String>,
    /// See [`MergeUnit::slot`].
    pub sub_slot: Option<String>,
    /// The entry's resolved IUSE flags (`GraphEntry::use_flags_display`:
    /// `(flag, enabled)` pairs): the source engine exports the enabled
    /// ones as the build-phase `USE`. Empty means `USE=""` stands.
    pub use_flags: Vec<(String, bool)>,
}

impl MergeUnit {
    /// A source-build unit for `cpv` into `root` (no replace, local,
    /// unqualified -- the common case; the remaining fields are set
    /// directly when they differ).
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

/// The source-build `MergeEngine`: real `_emerge/MergeListItem.py`'s
/// `"ebuild"` arm (`EbuildBuild`, `MergeListItem.py:87-106`).
///
/// The real phase chain + vdb write live in the `portuale` binary
/// crate (`ebuild_merge::{run_merge, run_qmerge}` driven by
/// `emerge_build`'s serial loop and the `-jN` parallel scheduler, not
/// linkable from a library — the standing pattern); this marker exists
/// so the *trait* is exercised and the kind dispatch is pinned. A unit
/// of its own [`MergeKind::Source`] is declined as [`MergeOutcome::Skipped`]
/// (never merged, never failed); a [`MergeKind::Binary`] unit is
/// refused as [`MergeOutcome::Failed`] — the same `type_name` routing
/// real `MergeListItem._start` performs, split across the two engines.
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

/// The binary-unpack `MergeEngine`: real `_emerge/MergeListItem.py`'s
/// `"binary"` arm (`Binpkg`, `MergeListItem.py:108+`).
///
/// The real download + unpack + vdb write live in the `portuale`
/// binary crate (`emerge_getbinpkg::merge_binpkg` via
/// `run_merge_plan`'s per-entry dispatch, not linkable from a library);
/// this marker mirrors [`SourceMergeEngine`] for the other kind: a
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

/// The filesystem `PackagesDb` implementation: reads the vdb directly
/// from `<root>/var/db/pkg`. One of two implementations (the other is
/// [`MemoryDb` below); both satisfy the same three read queries. This is
/// the production read side: the unmerge/depclean/news paths consult the
/// installed db through this seam (`Director::installed_versions` /
/// `contents_files` / `reverse_dependents`), so a second backend
/// (`MemoryDb`, a snapshot) can replace the filesystem without touching
/// those call sites.
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
        // Real `dbapi.cp_list` sorts highest-first (`_cmp_cpv`); the vdb
        // scan itself yields `read_dir` order, so sort here to honour the
        // trait contract (same `vercmp` every other version ordering in
        // portuale uses).
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

/// The in-memory `PackagesDb` implementation: the "second
/// implementation per slot" for the installed-db slot, holding a
/// recorded snapshot of installed packages instead of reading
/// `<root>/var/db/pkg`.
///
/// Grounding: real `_emerge/FakeVartree.py::FakeVartree` — "an in-memory
/// copy of a vartree instance that provides all the interfaces required
/// for use by the depgraph", built so dependency calculations can run
/// without holding a lock on the vardb. Like it, this snapshot serves
/// resolution-shaped reads with no filesystem behind them (benchmarks,
/// tests, and callers that already hold the facts).
///
/// Two deliberate narrowings, both documented on the trait. First,
/// versions come back in the order the snapshot recorded them: callers
/// provide highest-first (real `dbapi.cp_list` sorts with `_cmp_cpv`),
/// the snapshot preserves. Second, reverse dependents are recorded
/// edges, not recomputed matches: real `FakeVartree` re-resolves dep
/// strings live, but recomputation needs `portage_dep` atom matching
/// (a dependency this contract crate deliberately does not take), so
/// the snapshot records each package's dependents as observed facts via
/// [`MemoryDb::add_package`].
#[derive(Debug, Clone, Default)]
pub struct MemoryDb {
    /// The single `root` this snapshot stands in for (owned — unlike
    /// [`VdbReader`'s] borrow — so a snapshot value is portable as one
    /// unit).
    root: PathBuf,
    /// `(category, package) → installed versions`, recorded order.
    versions: std::collections::HashMap<(String, String), Vec<String>>,
    /// `(category, package, version) → CONTENTS-files list`.
    contents: std::collections::HashMap<(String, String, String), Vec<String>>,
    /// `(category, package, version) → direct dependents` (CPVs).
    dependents: std::collections::HashMap<(String, String, String), Vec<String>>,
}

impl MemoryDb {
    /// An empty snapshot standing in for `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            versions: std::collections::HashMap::new(),
            contents: std::collections::HashMap::new(),
            dependents: std::collections::HashMap::new(),
        }
    }

    /// Record one installed package: its CONTENTS-files list and the
    /// CPVs that directly depend on it. Repeat calls for one
    /// `category/package` accumulate versions in call order — provide
    /// highest-first, real `dbapi.cp_list` order.
    pub fn add_package(
        &mut self,
        category: &str,
        package: &str,
        version: &str,
        contents_files: Vec<String>,
        depended_on_by: Vec<String>,
    ) {
        self.versions
            .entry((category.to_string(), package.to_string()))
            .or_default()
            .push(version.to_string());
        self.contents.insert(
            (
                category.to_string(),
                package.to_string(),
                version.to_string(),
            ),
            contents_files,
        );
        self.dependents.insert(
            (
                category.to_string(),
                package.to_string(),
                version.to_string(),
            ),
            depended_on_by,
        );
    }
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

/// The current `Fetcher`: real-src via the shared `wget` transport
/// (`portage_fetch::download_via_wget`, the same `FETCHCOMMAND` the
/// `portuale::fetch::fetch_src_uri` candidate loop runs).
///
/// An already-materialized `distdir/<filename>` is returned as-is (the
/// same already-fetched short-circuit real `fetch.py`'s own
/// `_check_distfile` gives before ever spawning `FETCHCOMMAND`);
/// otherwise the entry's own `uri` is downloaded fresh (non-resume --
/// resume applies to a partial left by an earlier candidate, which only
/// the full candidate loop in `portuale::fetch` can see).
///
/// Deliberate narrowing, documented on the trait: Manifest digest
/// verification stays at the `fetch_src_uri` call site (it needs the
/// `Manifest` entry for the file, context this seam deliberately does
/// not pass), so a second transport behind this seam downloads but never
/// verifies on its own.
pub struct WgetFetcher;
impl Fetcher for WgetFetcher {
    fn fetch(&self, entry: &portage_fetch::SrcUriEntry, distdir: &Path) -> Result<PathBuf, String> {
        std::fs::create_dir_all(distdir).map_err(|e| format!("{}: {e}", distdir.display()))?;
        let dest = distdir.join(&entry.filename);
        if dest.is_file() {
            return Ok(dest);
        }
        portage_fetch::download_via_wget(&entry.uri, &dest, false)?;
        Ok(dest)
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

/// The capped-jobs `SchedulerPolicy`: real `Scheduler._run`'s per-step
/// gate -- start another build while `running < jobs` and the system
/// 1-minute load average is under the `--load-average` ceiling (never
/// gating the first build, so the DAG cannot deadlock). One of two
/// implementations (the other is [`UnlimitedPolicy`], real's
/// `max_jobs is True` branch); `run_build_scheduler` runs under one of
/// them, chosen by the `--jobs` spelling.
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

/// The unlimited-jobs `SchedulerPolicy`: real `Scheduler` with
/// `max_jobs is True` (a bare `--jobs`/`-j`, which portuale maps to
/// `usize::MAX`) -- no concurrency ceiling at all, while the
/// `--load-average` gate still holds off *additional* builds (real
/// `PollScheduler._can_add_job` applies the load check whenever
/// `max_jobs is True or max_jobs > 1`).
///
/// This is the scheduler slot's second implementation: the same two
/// methods as [`LoadAwarePolicy`], the capped-vs-uncapped shapes real
/// itself branches on, and the policy `run_build_scheduler` runs under
/// for a bare `-j`.
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
/// Deliberate v1 narrowness: the fetch→build→merge walk's per-entry
/// dispatch and the `--check-news` unread computation run through these
/// slots (`emerge_getbinpkg::run_merge_plan` executes each entry's
/// `MergeUnit` via its engine; `pretend.rs::run_check_news` reads each
/// repo's unread ids via its news selector); the `-jN` DAG walk itself
/// stays in `emerge_build.rs` under the scheduler policy, and the `mrg`
/// applet still calls `pretend::run` directly until a second algorithm
/// actually lands in one of the remaining read slots.
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

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    D: PackagesDb,
{
    /// The installed versions of `category/package` through the
    /// director's installed-db (real `vartree` read side).
    pub fn installed_versions(&self, category: &str, package: &str) -> Vec<String> {
        self.packages_db.installed_versions(category, package)
    }

    /// The `CONTENTS` paths one installed CPV owns, through the
    /// director's installed-db.
    pub fn contents_files(&self, category: &str, package: &str, version: &str) -> Vec<String> {
        self.packages_db.contents_files(category, package, version)
    }

    /// The installed packages directly depending on one installed CPV,
    /// through the director's installed-db.
    pub fn reverse_dependents(&self, category: &str, package: &str, version: &str) -> Vec<String> {
        self.packages_db
            .reverse_dependents(category, package, version)
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

    /// One category's `pf` listing through the director's repo cache.
    pub fn repo_category(&self, category: &str) -> Vec<String> {
        self.repo_cache.category(category)
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    F: Fetcher,
{
    /// Materialize one `SRC_URI` file through the director's fetcher.
    pub fn fetch(
        &self,
        entry: &portage_fetch::SrcUriEntry,
        distdir: &Path,
    ) -> Result<PathBuf, String> {
        self.fetcher.fetch(entry, distdir)
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    M: MergeEngine,
{
    /// Execute one merge unit through the director's merge engine.
    pub fn execute(&self, unit: &MergeUnit, ctx: &MergeContext) -> MergeOutcome {
        self.merge_engine.execute(unit, ctx)
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    B: BinpkgIndex,
{
    /// The binary candidates for `category/package` through the
    /// director's binary-package index.
    pub fn binpkg_candidates(&self, category: &str, package: &str) -> Vec<portage_repo::Candidate> {
        self.binpkg_index.candidates(category, package)
    }

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

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    N: NewsSelector,
{
    /// The valid, relevant, unread news ids through the director's news
    /// selector.
    pub fn unread_news(&self) -> Vec<String> {
        self.news_selector.unread_ids()
    }
}

impl<S, D, C, F, M, B, N, P> Director<S, D, C, F, M, B, N, P>
where
    P: SchedulerPolicy,
{
    /// Whether the `-jN` DAG may start another build now, through the
    /// director's scheduler policy.
    pub fn should_start(&self, running: usize, loadavg_1min: f64) -> bool {
        self.scheduler_policy.should_start(running, loadavg_1min)
    }

    /// The hard concurrency ceiling through the director's policy.
    pub fn max_jobs(&self) -> usize {
        self.scheduler_policy.max_jobs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::{GraphResult, ResolveRequest};

    /// A fresh scratch dir per test (process id + nanos, so parallel
    /// `cargo test` workers never share one).
    fn tempdir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write one vdb entry: `<root>/var/db/pkg/<cat>/<pkg>-<ver>/` with
    /// the given `SLOT` plus `(filename, content)` files (`CONTENTS`,
    /// `USE`, `RDEPEND`, …).
    fn write_vdb_entry(
        root: &Path,
        category: &str,
        package: &str,
        version: &str,
        slot: &str,
        files: &[(&str, &str)],
    ) {
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

    /// `MemoryDb` is the installed-db slot's second implementation (real
    /// `_emerge/FakeVartree.py`'s in-memory vartree copy): the same three
    /// read queries as the filesystem vdb reader, served from a recorded
    /// snapshot. The shape pins: versions come back in recorded
    /// (caller-provided highest-first) order across repeat `add_package`
    /// calls, contents and reverse edges read back per version, unknown
    /// packages/versions read as empty (never a panic), and the snapshot
    /// owns its root.
    #[test]
    fn packages_db_memory_is_the_snapshot_second_backend() {
        let mut db = MemoryDb::new(Path::new("/root"));
        assert_eq!(db.root().to_str(), Some("/root"));
        assert!(db.installed_versions("dev-libs", "example").is_empty());
        db.add_package(
            "dev-libs",
            "example",
            "2.0",
            vec!["usr/lib/libx.so.2".to_string()],
            vec!["app/other-1.0".to_string()],
        );
        db.add_package(
            "dev-libs",
            "example",
            "1.0",
            vec!["usr/lib/libx.a".to_string()],
            Vec::new(),
        );
        assert_eq!(
            db.installed_versions("dev-libs", "example"),
            vec!["2.0", "1.0"]
        );
        assert_eq!(
            db.contents_files("dev-libs", "example", "2.0"),
            vec!["usr/lib/libx.so.2"]
        );
        assert_eq!(
            db.reverse_dependents("dev-libs", "example", "2.0"),
            vec!["app/other-1.0"]
        );
        assert!(
            db.reverse_dependents("dev-libs", "example", "1.0")
                .is_empty()
        );
        assert!(db.contents_files("dev-libs", "example", "9.9").is_empty());
        assert!(db.installed_versions("sys-apps", "missing").is_empty());
    }

    /// `VdbReader` is the installed-db slot's live implementation: the
    /// same three reads as `MemoryDb`, served from a real
    /// `<root>/var/db/pkg` tree. Versions come out highest-first (real
    /// `dbapi.cp_list` order), CONTENTS paths are the owned
    /// `obj`/`sym`/`dir` entries, reverse dependents are recomputed atom
    /// matches (a `dev-libs/consumer` whose vdb `RDEPEND` names
    /// `dev-libs/example` shows up for `example`, never for itself), and
    /// unknown packages/versions read as empty, never a panic.
    #[test]
    fn packages_db_vdb_reader_reads_a_real_vdb_tree() {
        let root = tempdir("mrg_director_vdb");
        write_vdb_entry(
            &root,
            "dev-libs",
            "example",
            "1.0",
            "0",
            &[
                (
                    "CONTENTS",
                    "obj /usr/lib/libx.a abc 123\nsym /usr/lib/libx.so -> libx.a 123\ndir /usr/lib\n",
                ),
                ("USE", ""),
                ("RDEPEND", ""),
            ],
        );
        write_vdb_entry(
            &root,
            "dev-libs",
            "example",
            "2.0",
            "0",
            &[("CONTENTS", "obj /usr/lib/libx.so.2 def 456\n")],
        );
        write_vdb_entry(
            &root,
            "dev-libs",
            "consumer",
            "1.0",
            "0",
            &[
                ("CONTENTS", "obj /usr/lib/libc.a 000 1\n"),
                ("USE", ""),
                ("RDEPEND", "dev-libs/example"),
            ],
        );
        let db = VdbReader::new(&root);
        assert_eq!(
            db.root().to_str(),
            Some(root.to_str().unwrap()),
            "the snapshot owns its root like MemoryDb does"
        );
        assert_eq!(
            db.installed_versions("dev-libs", "example"),
            vec!["2.0", "1.0"],
            "highest-first, real dbapi.cp_list order"
        );
        assert_eq!(
            db.contents_files("dev-libs", "example", "1.0"),
            vec!["usr/lib/libx.a", "usr/lib/libx.so", "usr/lib"],
        );
        assert_eq!(
            db.reverse_dependents("dev-libs", "example", "2.0"),
            vec!["dev-libs/consumer-1.0"],
        );
        assert!(
            db.reverse_dependents("dev-libs", "consumer", "1.0")
                .is_empty()
        );
        assert!(db.installed_versions("sys-apps", "missing").is_empty());
        assert!(db.contents_files("dev-libs", "example", "9.9").is_empty());
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
        // Unfetchable host, nothing pre-materialized: the shared `wget`
        // transport fails, and the seam reports it (no panic, no
        // half-written file left behind).
        let dir = tempdir("mrg_director_fetch");
        assert!(fetcher.fetch(&entry, &dir).is_err());
        assert!(!dir.join("x.tgz").exists());

        // An already-materialized `distdir/<filename>` is returned as-is
        // without touching the network (real `_check_distfile`'s own
        // already-fetched short-circuit).
        std::fs::write(dir.join("x.tgz"), b"already here").unwrap();
        assert_eq!(fetcher.fetch(&entry, &dir).unwrap(), dir.join("x.tgz"));

        // The unit constructors carry the common-case defaults (local,
        // unqualified, no replace).
        let unit = MergeUnit::source("dev-libs/example-1.0", Path::new("/root"));
        assert_eq!(unit.kind, MergeKind::Source);
        assert!(!unit.remote_binary);
        assert!(unit.build_id.is_none());
        let unit = MergeUnit::binary("dev-libs/example-1.0", Path::new("/root"));
        assert_eq!(unit.kind, MergeKind::Binary);
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
            remote_binary: false,
            build_id: None,
            slot: Some("0".to_string()),
            sub_slot: None,
            use_flags: vec![("flag".to_string(), true)],
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
        };
        assert!(matches!(
            engine.execute(&broken, &ctx),
            MergeOutcome::Failed(_)
        ));
    }

    /// The merge-engine slot's two implementations split on the unit
    /// kind, exactly real `MergeListItem._start`'s `type_name` dispatch
    /// (`"ebuild"` → `EbuildBuild`, `"binary"` → `Binpkg`): each engine
    /// declines its own kind as [`MergeOutcome::Skipped`] (the real
    /// execution lives in the `portuale` binary crate, the standing
    /// pattern) and refuses the other kind as [`MergeOutcome::Failed`].
    /// The shape pins the routing, not the merge itself.
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

    /// Every `Director` delegation method routes through its slot: the
    /// installed-db reads, the repo-cache reads, the fetch, the merge
    /// execution, the binpkg-index reads, the news ids, and the scheduler
    /// gates all answer from the held components. This pins that the
    /// director is live wiring, not a field bag -- the production paths
    /// (`run_merge_plan`, `run_check_news`, the unmerge reads) call these
    /// same methods rather than reaching past the director.
    #[test]
    fn director_delegates_every_stage_through_its_slots() {
        struct FakeSolver;
        impl Resolver for FakeSolver {
            fn resolve(&self, _req: &ResolveRequest) -> Result<GraphResult, portage_repo::Error> {
                Err(portage_repo::Error::Detail("inert pin".into()))
            }
        }
        struct RoutingEngine;
        impl MergeEngine for RoutingEngine {
            fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
                if unit.cpv.contains("example") {
                    MergeOutcome::Merged
                } else {
                    MergeOutcome::Failed("unknown unit".to_string())
                }
            }
        }
        struct FakeNews;
        impl NewsSelector for FakeNews {
            fn unread_ids(&self) -> Vec<String> {
                vec!["2026-09-01-wired".to_string()]
            }
            fn repo_name(&self) -> String {
                "wired".to_string()
            }
        }
        let aux = std::collections::HashMap::from([("SLOT".to_string(), "0".to_string())]);
        let cache = VolatileCache::from_entries(
            vec![("dev-libs".to_string(), "a-1.0".to_string(), aux)],
            "wired",
        );
        let entries = vec![std::collections::HashMap::from([
            ("CPV".to_string(), "dev-libs/wired-1.0".to_string()),
            ("SLOT".to_string(), "0".to_string()),
        ])];
        let idx = portage_repo::BinaryIndex::from_entries(entries);
        let mut db = MemoryDb::new(Path::new("/root"));
        db.add_package(
            "dev-libs",
            "wired",
            "1.0",
            vec!["usr/lib/libw.a".to_string()],
            vec!["app/dep-1.0".to_string()],
        );
        type Wiring = Director<
            FakeSolver,
            MemoryDb,
            VolatileCache,
            WgetFetcher,
            RoutingEngine,
            PkgdirBinIndex<'static>,
            FakeNews,
            UnlimitedPolicy,
        >;
        // `PkgdirBinIndex` borrows its index and pkgdir; leak both so the
        // director can own a `'static` view in this shape test.
        let idx: &'static portage_repo::BinaryIndex = Box::leak(Box::new(idx));
        let pkgdir: &'static Path = Box::leak(Box::new(PathBuf::from("/var/cache/binpkgs")));
        let director = Wiring {
            solver: FakeSolver,
            packages_db: db,
            repo_cache: cache,
            fetcher: WgetFetcher,
            merge_engine: RoutingEngine,
            binpkg_index: PkgdirBinIndex { index: idx, pkgdir },
            news_selector: FakeNews,
            scheduler_policy: UnlimitedPolicy::new(None),
        };
        assert_eq!(
            director.installed_versions("dev-libs", "wired"),
            vec!["1.0"]
        );
        assert_eq!(
            director.contents_files("dev-libs", "wired", "1.0"),
            vec!["usr/lib/libw.a"]
        );
        assert_eq!(
            director.reverse_dependents("dev-libs", "wired", "1.0"),
            vec!["app/dep-1.0"]
        );
        assert_eq!(
            director
                .repo_metadata("dev-libs", "a-1.0")
                .unwrap()
                .get("SLOT"),
            Some(&"0".to_string())
        );
        assert_eq!(director.repo_category("dev-libs"), vec!["a-1.0"]);
        assert_eq!(
            director
                .binpkg_candidates("dev-libs", "wired")
                .iter()
                .map(|c| c.version.clone())
                .collect::<Vec<_>>(),
            vec!["1.0"]
        );
        assert!(
            director
                .binpkg_metadata("dev-libs", "wired", "1.0")
                .is_some()
        );
        let ctx = MergeContext {
            root: PathBuf::from("/root"),
            builddir: PathBuf::from("/var/tmp/portage"),
            jobs: 1,
            keep_going: false,
        };
        assert_eq!(
            director.execute(
                &MergeUnit::source("dev-libs/example-1.0", Path::new("/root")),
                &ctx
            ),
            MergeOutcome::Merged
        );
        assert!(matches!(
            director.execute(
                &MergeUnit::source("dev-libs/other-1.0", Path::new("/root")),
                &ctx
            ),
            MergeOutcome::Failed(_)
        ));
        assert_eq!(director.unread_news(), vec!["2026-09-01-wired"]);
        assert!(director.should_start(64, 0.0));
        assert_eq!(director.max_jobs(), usize::MAX);
        // A fetch through the director's fetcher against a
        // pre-materialized distdir file never touches the network.
        let distdir = tempdir("mrg_director_delegation");
        std::fs::write(distdir.join("w.tgz"), b"pre-materialized").unwrap();
        let entry = portage_fetch::SrcUriEntry {
            uri: "https://example.invalid/w.tgz".to_string(),
            filename: "w.tgz".to_string(),
            override_mirror: false,
            override_fetch: false,
        };
        assert_eq!(
            director.fetch(&entry, &distdir).unwrap(),
            distdir.join("w.tgz")
        );
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

    /// `UnlimitedPolicy` is the scheduler slot's second implementation
    /// (real's `max_jobs is True`, a bare `--jobs`/`-j`): no concurrency
    /// ceiling (`max_jobs` is `usize::MAX`, the value the CLI maps bare
    /// `-j` to), while the `--load-average` gate still holds off
    /// *additional* builds exactly like the capped policy (real
    /// `PollScheduler._can_add_job` load-checks whenever `max_jobs is
    /// True or max_jobs > 1`).
    #[test]
    fn scheduler_policy_unlimited_never_caps_but_still_load_gates() {
        let unlimited = UnlimitedPolicy::new(Some(2.0));
        assert_eq!(unlimited.max_jobs(), usize::MAX);
        assert!(unlimited.should_start(0, 99.0));
        assert!(unlimited.should_start(1000, 1.5));
        assert!(!unlimited.should_start(1000, 2.5));

        let ungated = UnlimitedPolicy::new(None);
        assert!(ungated.should_start(usize::MAX - 1, f64::INFINITY));

        // The constructor the scheduler runs under carries both knobs.
        let wired = LoadAwarePolicy::new(4, Some(2.0));
        assert_eq!(wired.max_jobs(), 4);
        assert!(wired.should_start(3, 1.5));
        assert!(!wired.should_start(3, 2.5));
    }
}
