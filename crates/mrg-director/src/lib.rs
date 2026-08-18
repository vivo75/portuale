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

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::GraphResult;
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
}
