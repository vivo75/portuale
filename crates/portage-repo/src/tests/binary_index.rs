use super::*;

/// `build_local_binpkg_index` honours the CLI `$PKGDIR` scan when one exists.
#[test]
fn build_local_binpkg_index_uses_the_cli_scanned_binpkgs() {
    let config = portage_profile::Config {
        scanned_binpkgs: Some(vec![HashMap::from([
            ("CPV".to_string(), "dev-libs/memo-1.0".to_string()),
            ("SLOT".to_string(), "0".to_string()),
        ])]),
        ..portage_profile::Config::default()
    };
    let index = build_local_binpkg_index(&config);
    assert_eq!(
        list_binary_candidates(&index, "dev-libs", "memo")
            .iter()
            .map(|cand| cand.version.as_str())
            .collect::<Vec<_>>(),
        vec!["1.0"]
    );
}

#[test]
fn list_remote_binary_candidates_reads_each_binrepos_own_packages_index() {
    // Only in the binhost index: no ebuild, no local entry.
    let binrepos = vec![fixture_binhost()];
    let local = BinaryIndex::from_pkgdir(&fixtures_root().join("pkgdir"));
    let cands = list_remote_binary_candidates(
        &binrepos,
        Path::new("/"),
        &local,
        "dev-libs",
        "remotebinpkg",
    );
    assert_eq!(cands.len(), 1);
    let c = &cands[0];
    assert_eq!(c.version, "1.0");
    assert!(c.remote, "a binhost candidate is marked remote");
    assert_eq!(c.repo_name, "gentoo", "from the index's own REPO field");
    assert_eq!(c.source, CandidateSource::Binary);

    // Already-local entries are dropped (Portage `bintree.isremote`).
    let none = list_remote_binary_candidates(
        &binrepos,
        Path::new("/"),
        &BinaryIndex::from_pkgdir(&fixtures_root().join("binhost")),
        "dev-libs",
        "remotebinpkg",
    );
    assert!(none.is_empty());

    // No binrepos: no remote candidates.
    assert!(
        list_remote_binary_candidates(&[], Path::new("/"), &local, "dev-libs", "remotebinpkg",)
            .is_empty()
    );
}

#[test]
fn dedup_binary_instances_keeps_newest_build_time_per_group() {
    let bin = |ver: &str, bid: &str, bt: i64, use_flags: &[&str]| Candidate {
        version: ver.to_string(),
        keywords: vec!["amd64".to_string()],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: i32::MIN,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: "feat".to_string(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Binary,
        binary_use: Some(use_flags.iter().map(|s| s.to_string()).collect()),
        remote: true,
        build_id: Some(bid.to_string()),
        build_time: Some(bt),
        binary_deps: HashMap::new(),
    };
    let cfg = test_config();

    // Bare atom: newest BUILD_TIME wins.
    let atom = portage_dep::parse_atom("dev-libs/x").unwrap();
    let out = dedup_binary_instances(
        vec![
            bin("1.0", "1", 1000, &[]),
            bin("1.0", "3", 3000, &[]),
            bin("1.0", "2", 2000, &[]),
        ],
        &atom,
        &cfg,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].build_id.as_deref(), Some("3"));

    // Atom `[feat]`: the satisfying instance wins even when older.
    let atom_use = portage_dep::parse_atom("dev-libs/x[feat]").unwrap();
    let out = dedup_binary_instances(
        vec![bin("1.0", "1", 1000, &["feat"]), bin("1.0", "2", 2000, &[])],
        &atom_use,
        &cfg,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].build_id.as_deref(), Some("1"));

    // Ebuilds pass through untouched.
    let mut eb = bin("1.0", "9", 9000, &[]);
    eb.source = CandidateSource::Ebuild;
    eb.build_id = None;
    let out = dedup_binary_instances(vec![eb], &atom, &cfg);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source, CandidateSource::Ebuild);
}

/// A local `$PKGDIR` version hides only its own build, never a newer remote one.
#[test]
fn multi_instance_local_build_does_not_shadow_a_newer_remote_build() {
    let binrepo = in_memory_binhost_1010(
        "shadow",
        vec![
            multi_instance_entry_1010("6", 1000, "dev-libs/old"),
            multi_instance_entry_1010("7", 2000, "dev-libs/new"),
        ],
    );
    let local =
        BinaryIndex::from_entries(vec![multi_instance_entry_1010("6", 1000, "dev-libs/old")]);
    let cands =
        list_remote_binary_candidates(&[binrepo], Path::new("/"), &local, "dev-libs", "spectre");
    let ids: Vec<Option<&str>> = cands.iter().map(|c| c.build_id.as_deref()).collect();
    assert!(
        ids.contains(&Some("7")),
        "remote build 7 must survive the local build 6, got {ids:?}"
    );
    assert!(
        !ids.contains(&Some("6")),
        "remote build 6 is the already-downloaded local one, got {ids:?}"
    );
}

/// The download lookup falls back to the newest build when no `BUILD_ID` is asked.
#[test]
fn multi_instance_download_lookup_returns_the_newest_build() {
    let binrepo = in_memory_binhost_1010(
        "download",
        vec![
            multi_instance_entry_1010("6", 1000, "dev-libs/old"),
            multi_instance_entry_1010("7", 2000, "dev-libs/new"),
        ],
    );
    let binrepos = [binrepo];
    let (_, record) =
        find_remote_binpkg(&binrepos, Path::new("/"), "dev-libs", "spectre", "0.2.12")
            .expect("the binhost lists dev-libs/spectre-0.2.12");
    assert_eq!(
        record.get("BUILD_ID").map(String::as_str),
        Some("7"),
        "the newest build must be downloaded, got PATH {:?}",
        record.get("PATH")
    );
}

#[test]
fn quickpkg_direct_index_entries_reads_the_source_root_vdb() {
    // One installed package with an RDEPEND on newpkg.
    let src = fixtures_root().join("quickpkgroot");
    let entries = quickpkg_direct_index_entries(&src);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(
        e.get("CPV").map(String::as_str),
        Some("dev-libs/quickpkgdirectpkg-1.0")
    );
    assert_eq!(e.get("SLOT").map(String::as_str), Some("0"));
    assert_eq!(
        e.get("RDEPEND").map(String::as_str),
        Some("dev-libs/newpkg")
    );
    assert_eq!(e.get("REPO").map(String::as_str), Some("testrepo"));
    // An empty vdb file contributes no key.
    assert!(!e.contains_key("DEPEND"));

    // The synthesized record reads like a `Packages` entry.
    let index = BinaryIndex::from_entries(entries);
    let cands = list_binary_candidates(&index, "dev-libs", "quickpkgdirectpkg");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, CandidateSource::Binary);
    assert_eq!(cands[0].version, "1.0");
    assert_eq!(
        read_binary_metadata(&index, "dev-libs", "quickpkgdirectpkg", "1.0")
            .and_then(|m| m.get("RDEPEND").cloned())
            .as_deref(),
        Some("dev-libs/newpkg"),
    );

    // No vdb: empty list, not an error.
    assert!(quickpkg_direct_index_entries(&fixtures_root().join("distfiles")).is_empty());
}

#[test]
fn exclude_prevents_a_not_yet_installed_package_from_being_offered() {
    // No installed version: excluding leaves no candidate.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config())
            },
            "dev-libs/newpkg",
            &SelectOptions {
                excluded: &["dev-libs/newpkg".to_string()],
                is_top_level: true,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend must succeed"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn changed_slot_does_not_fire_for_a_package_with_no_recorded_difference() {
    // Matching SLOT: no reinstall even with --changed-slot.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &config,
                local_binpkg: &build_local_binpkg_index(&config)
            },
            "dev-libs/samepkg",
            &SelectOptions {
                changed_slot: true,
                is_top_level: true,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend must succeed"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_and_changed_slot_combine_in_one_reinstall_outcome() {
    // Portage treats --changed-deps and --changed-slot as independent triggers, so
    // both fields land on the same Reinstall outcome.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &config,
                local_binpkg: &build_local_binpkg_index(&config)
            },
            "dev-libs/changedslotpkg",
            &SelectOptions {
                changed_deps: true,
                changed_slot: true,
                is_top_level: true,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend must succeed"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn fixture_repo_constrained_atom_finds_the_named_repo_only() {
    // Overlay-only package: `::overlay` resolves it, `::testrepo` does not.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &config,
                local_binpkg: &build_local_binpkg_index(&config)
            },
            "dev-libs/overlayonlypkg::overlay",
            &SelectOptions {
                is_top_level: true,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend must succeed"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &config,
                local_binpkg: &build_local_binpkg_index(&config)
            },
            "dev-libs/overlayonlypkg::testrepo",
            &SelectOptions {
                is_top_level: true,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend must succeed"),
        PretendOutcome::NoVisibleCandidate
    );
}
