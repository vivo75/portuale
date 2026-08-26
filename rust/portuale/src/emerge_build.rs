// Real `emerge --buildpkgonly` execution, WITHOUT `--pretend`: actually
// builds a binary package for every entry pretend.rs's own dry-run gate
// already proved is safe to build. `GraphResult::buildpkgonly_deps_unsatisfied`
// being `false` (the gate `pretend.rs` already checks before calling
// anything here) means no needs-building entry's own `required_by` set
// includes another needs-building entry -- i.e. none of them depend on
// each other at all, real `--buildpkgonly`'s whole point (every real
// dependency must already be satisfied by something already installed) --
// so there is no cross-entry build ordering to compute here, unlike a
// real merge's own topological sort.
//
// Reuses `ebuild_package::run_package` (task #105-#109) as-is: "package"
// IS the real, unmodified `doebuild()` action `--buildpkgonly` itself is
// built on (see `resolve_pretend_graph`'s own doc comment -- real
// `--buildpkgonly` is a resolution-time depgraph check, not a distinct
// execution mode of its own). `run_package`'s own `install` chain now
// really fetches a nonempty `SRC_URI` too (see `ebuild_phases::
// fetch_sources`/`crate::fetch`'s own module doc comments) -- this
// module used to refuse any entry with a real `SRC_URI` outright (no
// fetch machinery existed yet); that refusal is gone now that fetching
// is real, and a fetch/digest failure simply surfaces as an ordinary
// `run_package` error like any other build failure would.
//
// `GraphEntry` doesn't carry the winning candidate's own repo location
// (see its own doc comment -- deliberately not threaded through the
// whole graph-resolution/Python-mirror pair, which has no real-execution
// need for it at all). `locate_candidate` re-derives it via
// `portage_repo::list_candidates`, the same repo/version lookup
// `resolve_pretend_graph` already did internally to pick this entry's
// winning version in the first place.
//
// KNOWN, DOCUMENTED GAPS (same "narrow v1, document the cut" pattern as
// every other real-execution slice in this pilot):
//   - A `CandidateSource::Binary` entry (would only appear via
//     `--usepkg`) is skipped outright -- it's already a binary, there is
//     nothing to build.
//   - Builds run strictly in `entries` order (no explicit reordering) --
//     safe only because the gate above already guarantees no ordering
//     constraint exists between any two needs-building entries.
//   - A build failure aborts immediately, unless real `--keep-going` is
//     given (now real, see `run_buildpkgonly`'s own doc comment) -- no
//     cleanup of any already-built packages either way, this pilot's
//     own single-invocation-at-a-time CLI usage never needs partial-
//     build cleanup.

use crate::ebuild_package::{self, PackageOptions};
use portage_repo::{Candidate, CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::{Path, PathBuf};

/// The version this entry would actually build at, or `None` for an
/// outcome real `--buildpkgonly` never builds anything for
/// (`AlreadyInstalled`/`NoVisibleCandidate` -- the latter can't reach
/// here at all, since it aborts the whole resolution before any
/// `GraphEntry` exists for it).
fn entry_version(outcome: &PretendOutcome) -> Option<&str> {
    match outcome {
        PretendOutcome::New { version } => Some(version),
        PretendOutcome::Upgrade { to, .. } => Some(to),
        PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::Reinstall { version, .. } => Some(version),
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => None,
    }
}

/// Re-finds the winning candidate for `category/package` at exactly
/// `version` -- the same repo/version lookup `resolve_pretend_graph`
/// already did internally to pick this entry's winning version in the
/// first place, just not retained on `GraphEntry` (see the module doc
/// comment). When more than one repo has this exact version (a real, if
/// rare, overlay-shadows-main-repo case), prefers the highest-priority
/// repo, the same tie-break `resolve_pretend`'s own candidate selection
/// already uses elsewhere in this crate.
fn locate_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> Option<Candidate> {
    let candidates = portage_repo::list_candidates(repos, category, package).ok()?;
    candidates
        .into_iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
}

/// Real doebuild()'s own `<repo>/<category>/<package>/<package>-<version>.ebuild`
/// path convention.
fn ebuild_path(candidate: &Candidate, category: &str, package: &str, version: &str) -> PathBuf {
    candidate
        .repo_location
        .join(category)
        .join(package)
        .join(format!("{package}-{version}.ebuild"))
}

/// Actually builds a binary package (never merges) for every entry in
/// `entries` that real `--buildpkgonly` would build -- see the module
/// doc comment for the full scope. Without `keep_going`, returns the
/// *first* failure encountered (message already includes which package
/// failed) and stops there, matching this pilot's own long-established
/// default. With real `--keep-going` (real `main.py`'s own `y_or_n`
/// option, narrowed by this pilot's own CLI transcription to the bare/
/// `y` form -- see `pretend.rs`'s own `keep_going` doc comment), every
/// entry is still attempted regardless of earlier failures -- safe here
/// specifically because the gate `pretend.rs` already checks before
/// calling this at all (`GraphResult::buildpkgonly_deps_unsatisfied`)
/// guarantees no entry depends on another, so unlike real portage's own
/// general `--keep-going` (which must also skip every *dependent* of a
/// failed package, tracked via real `Scheduler.py`'s own mergelist
/// recalculation), there is nothing here that a failure could ever
/// invalidate for a later entry. Failures are collected and returned
/// together at the end as a single combined error listing every one --
/// `Ok(())` only once every entry has a real binary package on disk.
pub fn run_buildpkgonly(
    entries: &[GraphEntry],
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
    keep_going: bool,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for entry in entries {
        if entry.source == CandidateSource::Binary {
            continue;
        }
        let Some(version) = entry_version(&entry.outcome) else {
            continue;
        };
        let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, version)
        else {
            let failure = format!(
                "{}/{}-{version}: could not locate its own ebuild file \
                 (repo layout changed since resolution?)",
                entry.category, entry.package
            );
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        };
        let path = ebuild_path(&candidate, &entry.category, &entry.package, version);
        println!(
            ">>> Building binary for {}/{}-{version}...",
            entry.category, entry.package
        );
        let failure = match ebuild_package::run_package(&path, root, portage_tmpdir, options) {
            Ok(0) => None,
            Ok(_) => Some(format!(
                "{}/{}-{version}: build failed",
                entry.category, entry.package
            )),
            Err(e) => Some(format!(
                "{}/{}-{version}: {e}",
                entry.category, entry.package
            )),
        };
        if let Some(failure) = failure {
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} package(s) failed to build (--keep-going):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::find_repos;
    use std::fs;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emerge_build_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn locate_candidate_finds_the_real_fixture_ebuild_file() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let candidate = locate_candidate(&repos, "dev-libs", "packagepkg", "1.0").unwrap();
        let path = ebuild_path(&candidate, "dev-libs", "packagepkg", "1.0");
        assert!(path.ends_with("dev-libs/packagepkg/packagepkg-1.0.ebuild"));
        assert!(path.is_file(), "{path:?} should exist");
    }

    #[test]
    fn locate_candidate_is_none_for_a_version_that_does_not_exist() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        assert!(locate_candidate(&repos, "dev-libs", "packagepkg", "99.0").is_none());
    }

    #[test]
    fn run_buildpkgonly_skips_already_installed_and_no_visible_candidate() {
        // Neither outcome ever has a resolvable version (see
        // entry_version), so this must return Ok(()) without attempting
        // any real execution at all -- proven by using a nonexistent
        // ROOT/PORTAGE_TMPDIR/PackageOptions that would fail loudly if
        // touched.
        let entries = vec![
            GraphEntry {
                category: "dev-libs".into(),
                package: "samepkg".into(),
                outcome: PretendOutcome::AlreadyInstalled {
                    version: "1.0".into(),
                },
                blockers: vec![],
                slot: None,
                use_flags_display: vec![],
                required_by: vec![],
                source: CandidateSource::Ebuild,
                provenance: Default::default(),
                keyword_suggestion: None,
                use_suggestion: None,
                parent_use_suggestion: None,
                targets_running_root: false,
            },
            GraphEntry {
                category: "dev-libs".into(),
                package: "nosuchpkg".into(),
                outcome: PretendOutcome::NoVisibleCandidate,
                blockers: vec![],
                slot: None,
                use_flags_display: vec![],
                required_by: vec![],
                source: CandidateSource::Ebuild,
                provenance: Default::default(),
                keyword_suggestion: None,
                use_suggestion: None,
                parent_use_suggestion: None,
                targets_running_root: false,
            },
        ];
        let bogus = PathBuf::from("/nonexistent/does/not/exist");
        let result = run_buildpkgonly(
            &entries,
            &[],
            &bogus,
            &bogus,
            &PackageOptions {
                debug: false,
                pkgdir: bogus.clone(),
                distdir: bogus.clone(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn real_buildpkgonly_builds_a_real_binary_package_end_to_end() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![GraphEntry {
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            use_flags_display: vec![],
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
        }];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        assert!(result.is_ok(), "{result:?}");

        let tbz2 = pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
        assert!(tbz2.is_file(), "{tbz2:?} should exist");
        let bytes = fs::read(&tbz2).unwrap();
        assert!(
            bytes.windows(8).any(|w| w == b"XPAKPACK"),
            "missing real XPAK magic bytes"
        );

        let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
        assert!(packages.contains("CPV: dev-libs/packagepkg-1.0"));
    }

    #[test]
    fn real_buildpkgonly_refuses_a_real_src_uri_with_no_manifest_entry() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![GraphEntry {
            category: "dev-libs".into(),
            package: "fetchpkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            use_flags_display: vec![],
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
        }];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        // `fetchpkg`'s own fixture has a real, nonempty SRC_URI but no
        // Manifest entry at all -- refused before any network access is
        // even attempted (see `crate::fetch::fetch_src_uri`'s own doc
        // comment: unverifiable content is worse than a loud failure).
        let err = result.expect_err("an unverifiable SRC_URI must be refused");
        assert!(err.contains("no Manifest entry"), "{err}");
        assert!(
            !pkgdir.join("dev-libs/fetchpkg-1.0.tbz2").exists(),
            "must not have built anything"
        );
    }

    fn buildpkgonly_entry(category: &str, package: &str, version: &str) -> GraphEntry {
        GraphEntry {
            category: category.into(),
            package: package.into(),
            outcome: PretendOutcome::New {
                version: version.into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            use_flags_display: vec![],
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
        }
    }

    /// Without real `--keep-going`, a failing entry stops the whole run
    /// immediately -- a later, independently-buildable entry in the same
    /// list never even gets attempted.
    #[test]
    fn real_buildpkgonly_without_keep_going_stops_at_the_first_failure() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        // fetchpkg (no Manifest entry, always fails) listed *before*
        // packagepkg (builds cleanly) -- proves packagepkg is never even
        // attempted once fetchpkg fails.
        let entries = vec![
            buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
            buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
        ];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        let err = result.expect_err("fetchpkg must still fail");
        assert!(err.contains("no Manifest entry"), "{err}");
        assert!(
            !pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists(),
            "packagepkg must never be attempted once fetchpkg fails without --keep-going"
        );
    }

    /// With real `--keep-going`, a failing entry does *not* stop the
    /// run -- packagepkg still gets built despite fetchpkg's own
    /// failure, and the final error names both entries.
    #[test]
    fn real_buildpkgonly_with_keep_going_builds_past_a_failure() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![
            buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
            buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
        ];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            true,
        );
        let err = result.expect_err("fetchpkg still fails overall");
        assert!(err.contains("dev-libs/fetchpkg-1.0"), "{err}");
        assert!(err.contains("no Manifest entry"), "{err}");

        assert!(
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file(),
            "packagepkg must still be built with --keep-going, despite fetchpkg's own failure"
        );
    }
}
