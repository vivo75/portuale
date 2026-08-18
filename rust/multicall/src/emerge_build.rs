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
// execution mode of its own).
//
// `GraphEntry` doesn't carry the winning candidate's own repo location
// (see its own doc comment -- deliberately not threaded through the
// whole graph-resolution/Python-mirror pair, which has no real-execution
// need for it at all). `locate_ebuild` re-derives it via
// `portage_repo::list_candidates`, the same repo/version lookup
// `resolve_pretend_graph` already did internally to pick this entry's
// winning version in the first place.
//
// KNOWN, DOCUMENTED GAPS (same "narrow v1, document the cut" pattern as
// every other real-execution slice in this pilot):
//   - No real `SRC_URI` fetch/unpack at all (see `ebuild_phases.rs`'s own
//     doc comment). Empirically checked (not assumed) what this actually
//     does to a real ebuild with a nonempty `SRC_URI`: this pilot's own
//     environment setup never populates `A`/`AA` from `SRC_URI` at all,
//     so real EAPI 0's own default `src_unpack` (`unpack ${A}`) runs
//     with nothing to unpack and silently *succeeds* -- it does NOT fail
//     the way real portage's own separate pre-phase fetch/distfile check
//     would (real `doebuild()` checks `SRC_URI` against `DISTDIR`
//     *before* ever running the ebuild's own phases at all, a mechanism
//     this pilot has no equivalent of). Left uncaught, this would
//     silently produce a real, valid-looking but functionally empty
//     binary package instead of erroring -- worse than a loud failure,
//     so `locate_ebuild`'s own caller refuses outright (see
//     `run_buildpkgonly`'s own real `SRC_URI` check) rather than letting
//     that happen. Real fetch+Manifest verification is a
//     separately-scoped follow-up; until then this is a hard refusal,
//     not a silent gap.
//   - A `CandidateSource::Binary` entry (would only appear via
//     `--usepkg`) is skipped outright -- it's already a binary, there is
//     nothing to build.
//   - Builds run strictly in `entries` order (no explicit reordering) --
//     safe only because the gate above already guarantees no ordering
//     constraint exists between any two needs-building entries.
//   - A build failure aborts immediately (no partial-graph continuation,
//     no cleanup of any already-built packages) -- this pilot's own
//     single-invocation-at-a-time CLI usage never needs the resume/retry
//     machinery real portage's own `--keep-going` provides.

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
/// doc comment for the full scope. Returns the first failure
/// encountered (message already includes which package failed), or
/// `Ok(())` once every entry has a real binary package on disk.
pub fn run_buildpkgonly(
    entries: &[GraphEntry],
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
) -> Result<(), String> {
    for entry in entries {
        if entry.source == CandidateSource::Binary {
            continue;
        }
        let Some(version) = entry_version(&entry.outcome) else {
            continue;
        };
        let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, version)
        else {
            return Err(format!(
                "{}/{}-{version}: could not locate its own ebuild file \
                 (repo layout changed since resolution?)",
                entry.category, entry.package
            ));
        };
        // See the module doc comment's own "KNOWN, DOCUMENTED GAPS"
        // entry on SRC_URI: silently letting this through would build a
        // real but functionally empty binary package instead of failing
        // loudly, so it's refused here instead.
        let pf = format!("{}-{version}", entry.package);
        let src_uri_nonempty =
            portage_repo::read_md5_cache(&candidate.repo_location, &entry.category, &pf)
                .ok()
                .and_then(|metadata| metadata.get("SRC_URI").cloned())
                .is_some_and(|s| !s.trim().is_empty());
        if src_uri_nonempty {
            return Err(format!(
                "{}/{}-{version}: has a real SRC_URI, but this pilot has no \
                 real fetch/unpack machinery (see emerge_build.rs's own \
                 module doc comment) -- refusing rather than silently \
                 building an empty package",
                entry.category, entry.package
            ));
        }
        let path = ebuild_path(&candidate, &entry.category, &entry.package, version);
        println!(
            ">>> Building binary for {}/{}-{version}...",
            entry.category, entry.package
        );
        match ebuild_package::run_package(&path, root, portage_tmpdir, options) {
            Ok(0) => {}
            Ok(_) => {
                return Err(format!(
                    "{}/{}-{version}: build failed",
                    entry.category, entry.package
                ))
            }
            Err(e) => {
                return Err(format!(
                    "{}/{}-{version}: {e}",
                    entry.category, entry.package
                ))
            }
        }
    }
    Ok(())
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
            },
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
        }];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
            },
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
    fn real_buildpkgonly_refuses_a_real_src_uri_instead_of_building_an_empty_package() {
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
        }];

        let result = run_buildpkgonly(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
            },
        );
        let err = result.expect_err("a real SRC_URI must be refused, not silently built");
        assert!(err.contains("dev-libs/fetchpkg-1.0"), "{err}");
        assert!(err.contains("SRC_URI"), "{err}");
        assert!(
            !pkgdir.join("dev-libs/fetchpkg-1.0.tbz2").exists(),
            "must not have built anything"
        );
    }
}
