//! `emerge --getbinpkg` / `--getbinpkgonly` execution: refresh each remote
//! binhost's live index (in `--pretend` exactly like in a non-pretend merge),
//! resolve the graph, then merge every resolved entry -- per entry on its
//! `source` (a `Binary` candidate is downloaded+merged, anything else is
//! built+merged from source).
//!
//! Scope cuts a caller must know: a compressed remote index is tried
//! (`Packages.gz`/`Packages.zst`, then plain `Packages`); no `layout.conf`
//! negotiation (the index `PATH` is trusted); digest checks cover `SIZE` and
//! the `MD5`/`SHA1` fields present, plus a downloaded gpkg's internal
//! manifest and signature layer at merge time.

use crate::ebuild_merge::{self, MergeOptions};
use mrg_director::BinpkgIndex;
use portage_profile::{BinRepo, Config};
use portage_repo::{GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// Seconds since the epoch as `f64` (Portage's `time.time()`).
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Whether the local `<pkgdir>/Packages` index still lists this cpv. Like
/// [`local_index_record`] minus the basename tie-break (no file to name it
/// after): with several same-`CPV` stanzas the entry's `BUILD_ID` picks.
// Portage: _emerge/BinpkgVerifier.py, _start
fn local_index_lists_cpv(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> bool {
    let cpv = format!("{category}/{package}-{version}");
    let mut candidates = portage_repo::read_packages_index(pkgdir)
        .into_iter()
        .filter(|e| e.get("CPV").is_some_and(|c| c == &cpv))
        .peekable();
    if candidates.peek().is_none() {
        return false;
    }
    if let Some(want) = build_id.filter(|s| !s.is_empty()) {
        let narrowed: Vec<_> = candidates
            .filter(|e| e.get("BUILD_ID").is_some_and(|b| b == want))
            .collect();
        // A `BUILD_ID` matching nothing still leaves the plain `CPV` hit: Portage
        // resolves a plain string to the latest instance, so any surviving
        // instance keeps this true.
        if !narrowed.is_empty() {
            return true;
        }
    }
    true
}
