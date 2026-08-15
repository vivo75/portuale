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
