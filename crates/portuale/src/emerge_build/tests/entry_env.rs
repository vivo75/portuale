use super::*;

/// `--buildpkgonly`'s per-entry env matches `package.env` the way the merge
/// scheduler's `entry_build_env` does. Expected value from the Phase 2 oracle.
#[test]
fn buildpkgonly_entry_build_env_matches_package_env_and_folds_flags() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let candidate = locate_candidate(&repos, "dev-libs", "penvbuildpkg", "1.0")
        .expect("penvbuildpkg-1.0 fixture candidate resolves");
    let entry = source_entry(
        "penvbuildpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let config = portage_profile::Config {
        package_env_vars: vec![(
            "dev-libs/penvbuildpkg".to_string(),
            vec![
                (
                    "CFLAGS".to_string(),
                    "-Os -march=buildpkgonlypenv".to_string(),
                ),
                ("CC".to_string(), "buildpkgonly-cc".to_string()),
            ],
        )],
        ..portage_profile::Config::default()
    };
    let run_wide = run_wide_phase_env(&config);
    let cpv_slot = entry_cpv_slot(&entry, "1.0");
    let build_env = buildpkgonly_entry_build_env(BuildpkgonlyEntryBuildEnvArgs {
        config: &config,
        repos: &repos,
        entry: &entry,
        candidate: &candidate,
        cpv_slot: &cpv_slot,
        run_wide: &run_wide,
    });
    let get = |key: &str| {
        build_env
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .next_back()
    };
    assert_eq!(
        get("CFLAGS").as_deref(),
        Some("-Os -march=buildpkgonlypenv")
    );
    assert_eq!(get("CC").as_deref(), Some("buildpkgonly-cc"));

    // An unmatched entry (different cpv) keeps the run-wide value.
    let other = source_entry(
        "newpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let other_candidate = locate_candidate(&repos, "dev-libs", "newpkg", "1.0")
        .expect("newpkg-1.0 fixture candidate resolves");
    let other_cpv_slot = entry_cpv_slot(&other, "1.0");
    let other_env = buildpkgonly_entry_build_env(BuildpkgonlyEntryBuildEnvArgs {
        config: &config,
        repos: &repos,
        entry: &other,
        candidate: &other_candidate,
        cpv_slot: &other_cpv_slot,
        run_wide: &run_wide,
    });
    assert_ne!(
        other_env
            .iter()
            .find(|(k, _)| k == "CFLAGS")
            .map(|(_, v)| v.as_str()),
        Some("-Os -march=buildpkgonlypenv"),
        "an unmatched neighbour must not see the matched entry's package.env value"
    );
}

/// A `package.env` `FEATURES` entry folds onto the run-wide list instead of
/// replacing it. Expected value from Portage source.
#[test]
fn buildpkgonly_entry_build_env_folds_matched_features() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let candidate = locate_candidate(&repos, "dev-libs", "penvbuildpkg", "1.0")
        .expect("penvbuildpkg-1.0 fixture candidate resolves");
    let entry = source_entry(
        "penvbuildpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let config = portage_profile::Config {
        package_env_vars: vec![(
            "dev-libs/penvbuildpkg".to_string(),
            vec![("FEATURES".to_string(), "splitdebug".to_string())],
        )],
        ..portage_profile::Config::default()
    };
    let run_wide = run_wide_phase_env(&config);
    let run_wide_features = run_wide
        .iter()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let cpv_slot = entry_cpv_slot(&entry, "1.0");
    let build_env = buildpkgonly_entry_build_env(BuildpkgonlyEntryBuildEnvArgs {
        config: &config,
        repos: &repos,
        entry: &entry,
        candidate: &candidate,
        cpv_slot: &cpv_slot,
        run_wide: &run_wide,
    });
    let features = build_env
        .iter()
        .filter(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .next_back()
        .expect("FEATURES is exported");
    assert!(
        features.split_whitespace().any(|t| t == "splitdebug"),
        "matched FEATURES must fold in, got {features:?}"
    );
    for tok in run_wide_features.split_whitespace() {
        assert!(
            features.split_whitespace().any(|t| t == tok),
            "the run-wide FEATURES token {tok:?} must survive the fold, got {features:?}"
        );
    }
}
