use super::*;

/// Backlog #129: `--buildpkgonly`'s per-entry env now matches
/// `package.env` the same way the merge scheduler's `entry_build_env`
/// does -- Phase 2 S0 cell A/F's own oracle (a matched `CFLAGS`/`CC`
/// wins over the run-wide value), applied to the third build path.
/// Direct unit test of `buildpkgonly_entry_build_env` (no real build),
/// the same pattern `entry_build_env_resolves_full_use_expand_and_
/// entry_identity` already uses for the merge-scheduler sibling.
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

    // An unmatched entry (different cpv) keeps the run-wide value --
    // #129 must not leak a matched entry's vars onto its neighbours.
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

/// Backlog #98/#129: a `package.env` `FEATURES` entry is an
/// incremental -- it folds onto the run-wide list
/// (`[run-wide, pkg, calling-env]`), it does not replace it.
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

/// #37 S2: with a resolved config in scope, `entry_build_env` threads
/// the full effective `USE` (implicit profile flags included), the
/// per-package `USE_EXPAND` values, and the entry's `SLOT`/repo
/// identity -- the S0 oracle's exact row set
/// (`TEST/findings/l2.md` "S0 recon (#37)").
#[allow(clippy::field_reassign_with_default)]
#[test]
fn entry_build_env_resolves_full_use_expand_and_entry_identity() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let mut config = portage_profile::Config::default();
    // The profile layers `effective_use_flags` replays: implicit
    // arch/elibc/kernel flags no package declares in IUSE.
    config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
    config.iuse_effective = [
        "abi_x86_64",
        "amd64",
        "elibc_glibc",
        "kernel_linux",
        "riscv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    config.use_expand = ["ABI_X86"].iter().map(|s| s.to_string()).collect();
    config
        .other_vars
        .insert("FEATURES".to_string(), "sandbox".to_string());
    let mut options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    options.build_env = portage_profile::phase_environ(&config, None);
    options.resolved_config = Some(std::sync::Arc::new(config));

    let mut entry = source_entry(
        "archusepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let env = entry_build_env(&options, &entry, &repos);
    // Later pairs win in both backends (`Command::envs` / successive
    // `export`s), so read the *last* pair: the run-wide base carries
    // empty `USE_EXPAND` placeholders that the per-entry rows override.
    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
    // `archusepkg` declares `IUSE="amd64 riscv"`; the implicit flags
    // come from the resolver's full effective set, not from IUSE.
    assert_eq!(
        get(&env, "USE"),
        Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
    );
    assert_eq!(
        get(&env, "IUSE_EFFECTIVE"),
        Some("abi_x86_64 amd64 elibc_glibc kernel_linux riscv")
    );
    assert_eq!(get(&env, "ABI_X86"), Some("64"));
    assert_eq!(get(&env, "SLOT"), Some("0"));
    assert_eq!(get(&env, "PORTAGE_REPO_REVISIONS"), Some("{}"));
    assert_eq!(get(&env, "PORTAGE_REPO_NAME"), Some("testrepo"));
    // The run-wide half rides along, folded `FEATURES` included.
    assert_eq!(get(&env, "FEATURES"), Some("sandbox"));
    // Real `environ_filter`/`AA`-pop rows stay out (S0 findings).
    for absent in ["O", "AA", "SRC_URI", "PORTAGE_USE"] {
        assert_eq!(get(&env, absent), None, "{absent}");
    }

    // A declared sub-slot renders `slot/sub_slot`; a missing
    // `entry.repo_name` falls back to the candidate's own repo.
    entry.sub_slot = Some("5".to_string());
    entry.repo_name = None;
    let env = entry_build_env(&options, &entry, &repos);
    assert_eq!(get(&env, "SLOT"), Some("0/5"));
    assert!(
        get(&env, "PORTAGE_REPO_NAME").is_some_and(|v| !v.is_empty()),
        "repo fallback missing"
    );
}

/// Backlog #98: the per-entry folded `FEATURES` list. A matched env
/// file value folds onto the run-wide list in `[run-wide, pkg,
/// calling-env]` order (S0 cells B/C); no match returns `None`, so
/// the run-wide value — and every gate reading it — stays untouched.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn entry_resolved_features_folds_the_matched_env_file_value() {
    let options_for = |features: &str, pkg_raw: &str| {
        let mut options = ebuild_merge::MergeOptions::default();
        options.features = features.to_string();
        if !pkg_raw.is_empty() {
            options.package_env_vars = vec![(
                "dev-libs/penvccpkg".to_string(),
                vec![("FEATURES".to_string(), pkg_raw.to_string())],
            )];
        }
        options
    };
    let entry = source_entry(
        "penvccpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    // No match: None, the run-wide value stands.
    assert_eq!(
        entry_resolved_features(&options_for("sandbox", ""), &entry, ""),
        None
    );
    // Env file only: the token joins the folded, sorted list (S0 C).
    assert_eq!(
        entry_resolved_features(
            &options_for("sandbox userfetch", "probe-feature"),
            &entry,
            ""
        ),
        Some("probe-feature sandbox userfetch".to_string())
    );
    // A `-tok` in the calling env prunes the pkg token (S0 B).
    assert_eq!(
        entry_resolved_features(
            &options_for("sandbox userfetch", "probe-feature"),
            &entry,
            "-probe-feature"
        ),
        Some("sandbox userfetch".to_string())
    );
    // A pkg `-tok` does not prune a calling-env token.
    assert_eq!(
        entry_resolved_features(
            &options_for("sandbox", "-userfetch probe-feature"),
            &entry,
            "userfetch"
        ),
        Some("probe-feature sandbox userfetch".to_string())
    );
    // A non-matching atom contributes nothing.
    let mut options = options_for("sandbox", "probe-feature");
    options.package_env_vars = vec![(
        "dev-libs/otherpkg".to_string(),
        vec![("FEATURES".to_string(), "probe-feature".to_string())],
    )];
    assert_eq!(entry_resolved_features(&options, &entry, ""), None);
}

/// Backlog #332: real `Scheduler._allocate_config`
/// (`Scheduler.py:1899-1913`) calls `config.reload()` for every
/// package task, which re-reads `env.d` (`config.py:2691-2699`).
/// `entry_build_env` is portuale's `_allocate_config` point: once
/// `<eroot>/etc/profile.env` changes after the config was resolved,
/// the env it returns carries the new variable; with `profile.env`
/// untouched it equals the pre-#332 env exactly (fast path).
/// Expected value from real source (`config.py:2691-2699`), the bed
/// finding being `PT_HELPER_DOINS` missing from later packages.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn entry_build_env_rereads_profile_env_changed_after_resolve() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let eroot = tempdir();
    fs::create_dir_all(eroot.join("etc")).unwrap();
    fs::write(eroot.join("etc/profile.env"), "export ENVDRELOAD_OLD='x'\n").unwrap();
    let mut config = portage_profile::Config::default();
    config.use_tokens = vec!["amd64".to_string()];
    config.iuse_effective = ["amd64"].iter().map(|s| s.to_string()).collect();
    config.envd_eroot = eroot.clone();
    config.envd_raw = "export ENVDRELOAD_OLD='x'\n".to_string();
    config
        .envd_vars
        .insert("ENVDRELOAD_OLD".to_string(), "x".to_string());
    config
        .other_vars
        .insert("ENVDRELOAD_OLD".to_string(), "x".to_string());
    let mut options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    options.build_env = run_wide_phase_env(&config);
    options.resolved_config = Some(std::sync::Arc::new(config));
    let entry = source_entry(
        "archusepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let before = entry_build_env(&options, &entry, &repos);
    assert_eq!(before, entry_build_env(&options, &entry, &repos));
    assert!(before.iter().all(|(k, _)| k != "ENVDRELOAD_VAR"));

    // A merge ran `env-update`: the file now carries a new variable.
    fs::write(
        eroot.join("etc/profile.env"),
        "export ENVDRELOAD_OLD='x'\nexport ENVDRELOAD_VAR='1'\n",
    )
    .unwrap();
    let after = entry_build_env(&options, &entry, &repos);
    let last = |k: &str| {
        after
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(last("ENVDRELOAD_VAR"), Some("1"));
    assert_eq!(last("ENVDRELOAD_OLD"), Some("x"));
    assert_eq!(last("SLOT"), Some("0"));
    let _ = fs::remove_dir_all(&eroot);
}
