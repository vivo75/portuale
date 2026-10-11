use super::*;

#[test]
fn candidate_iuse_and_use_reads_a_binarys_own_baked_use() {
    // A binary candidate carries its built-with flags over its own
    // `iuse` (Portage `pkg.use.enabled` for a built package).
    let c = Candidate {
        version: "1.0".to_string(),
        keywords: vec!["amd64".to_string()],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: i32::MIN,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: "feat other".to_string(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Binary,
        binary_use: Some(HashSet::from(["feat".to_string()])),
        remote: true,
        build_id: Some("1".to_string()),
        build_time: Some(1),
        binary_deps: HashMap::new(),
    };
    let (iuse, use_flags) = candidate_iuse_and_use(&c, "dev-libs", "x", &test_config()).unwrap();
    assert_eq!(
        iuse,
        HashSet::from(["feat".to_string(), "other".to_string()])
    );
    assert_eq!(*use_flags, HashSet::from(["feat".to_string()]));
}

#[test]
fn best_binary_build_time_is_the_newest_binary_of_the_version() {
    let mk = |source, ver: &str, bt: Option<i64>| Candidate {
        version: ver.to_string(),
        keywords: vec![],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: bt,
        binary_deps: HashMap::new(),
    };
    let cands = vec![
        mk(CandidateSource::Ebuild, "1.0", None),
        mk(CandidateSource::Binary, "1.0", Some(100)),
        mk(CandidateSource::Binary, "1.0", Some(300)),
        mk(CandidateSource::Binary, "2.0", Some(999)),
    ];
    assert_eq!(best_binary_build_time(&cands, "1.0"), Some(300));
    assert_eq!(best_binary_build_time(&cands, "3.0"), None);
}

#[test]
fn ebuild_visible_at_and_binary_reinstall_warranted_match_portage_identical_binary() {
    // `ebuild_visible_at` asks the tree: `newpkg` is visible, `rebuiltbinarypkg` is not.
    let repos = find_repos(&fixtures_root()).expect("fixture repos.conf resolves");
    let config = test_config();
    assert!(
        ebuild_visible_at(&repos, "dev-libs", "newpkg", "1.0", &config),
        "newpkg's ebuild is visible"
    );
    assert!(
        !ebuild_visible_at(&repos, "dev-libs", "rebuiltbinarypkg", "1.0", &config),
        "rebuiltbinarypkg has no ebuild"
    );

    // Portage `identical_binary` vs `identical_binary`+`_equiv_ebuild_visible`:
    // the latter only fires under `--update`.
    let root = TempDir::new("portuale-btre").keep();
    let vdb = root.join("var/db/pkg/dev-libs/rebuiltbinarypkg-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("BUILD_TIME"), "1000\n").unwrap();
    let newpkg_vdb = root.join("var/db/pkg/dev-libs/newpkg-1.0");
    std::fs::create_dir_all(&newpkg_vdb).unwrap();
    std::fs::write(newpkg_vdb.join("BUILD_TIME"), "1000\n").unwrap();

    // Identical build time -> never reinstalls, --update or not.
    assert!(
        !binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
            root: &root,
            repos: &repos,
            config: &config,
            category: "dev-libs",
            package: "rebuiltbinarypkg",
            version: "1.0",
            binary_build_time: Some(1000),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            update: true
        }),
        "identical BUILD_TIME is identical_binary"
    );
    // Differing, ebuild gone, but no --update -> avoid_update keeps
    // the installed package.
    assert!(
        !binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
            root: &root,
            repos: &repos,
            config: &config,
            category: "dev-libs",
            package: "rebuiltbinarypkg",
            version: "1.0",
            binary_build_time: Some(2000),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            update: false
        }),
        "without --update the installed instance is kept"
    );
    // Differing, ebuild gone, --update -> reinstall.
    assert!(
        binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
            root: &root,
            repos: &repos,
            config: &config,
            category: "dev-libs",
            package: "rebuiltbinarypkg",
            version: "1.0",
            binary_build_time: Some(2000),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            update: true
        }),
        "differing BUILD_TIME + ebuild gone + --update reinstalls"
    );
    // Differing but the ebuild IS visible (newpkg) -> no reinstall
    // without --rebuilt-binaries, regardless of --update.
    assert!(
        !binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
            root: &root,
            repos: &repos,
            config: &config,
            category: "dev-libs",
            package: "newpkg",
            version: "1.0",
            binary_build_time: Some(2000),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            update: true
        }),
        "a visible ebuild keeps the installed instance"
    );
    // --rebuilt-binaries escalates: reinstall for a differing
    // BUILD_TIME even when the ebuild is visible.
    assert!(
        binary_reinstall_warranted(BinaryReinstallWarrantedArgs {
            root: &root,
            repos: &repos,
            config: &config,
            category: "dev-libs",
            package: "newpkg",
            version: "1.0",
            binary_build_time: Some(2000),
            rebuilt_binaries: true,
            rebuilt_binaries_timestamp: None,
            update: true
        }),
        "--rebuilt-binaries reinstalls regardless of ebuild visibility"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn newuse_reinstalls_when_vdb_use_differs_from_current_use() {
    // reinstallpkg installed with foo off; the profile enables foo now,
    // so --newuse reports a Reinstall for the changed flag.
    assert_eq!(
        resolve_real_newuse("dev-libs", "reinstallpkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec!["foo".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn newuse_does_not_reinstall_when_use_has_not_changed() {
    // samepkg has no IUSE at all, so --newuse has nothing to detect.
    assert_eq!(
        resolve_real_newuse("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newuse_forced_flags_suppresses_a_spurious_reinstall() {
    // A newly added IUSE flag that use.mask forces off is never
    // enabled before or after, so no Reinstall (the forced-flags cut).
    // Portage: depgraph.py, reinstall_flags_for_use_change
    assert_eq!(
        resolve_real_newuse("dev-libs", "usemaskreinstallpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_reinstalls_when_vdb_rdepend_differs_from_the_current_ebuild() {
    // changeddepspkg's vdb RDEPEND names samepkg; its ebuild names newpkg.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "changeddepspkg", true),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn changed_deps_still_fires_when_with_bdeps_is_false() {
    // RDEPEND survives --with-bdeps=n (only DEPEND/BDEPEND drop).
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "changeddepspkg", false),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn changed_deps_detects_an_atom_moved_between_two_dep_keys() {
    // The same atom moved from RDEPEND to PDEPEND: the net set is
    // identical, but the per-key comparison catches it (Portage `_changed_deps`).
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "movedkeydepspkg", false),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn changed_deps_ignores_a_built_slot_operator_deps_resolved_slot() {
    // The vdb's built `:2=` normalizes back to `:=` before comparing
    // (Portage `strip_slots`), so this is NOT a changed dependency.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "slotopdepspkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_detects_an_any_of_group_reorder() {
    // Swapped `||` alternatives count: order is significant in
    // Portage's structured `_changed_deps` comparison.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "anyofreorderdepspkg", false),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn changed_deps_detects_a_plain_atom_reorder_but_not_a_redundant_bracket() {
    // Order is significant even in AND context (Portage's list `!=`)...
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "orderchangeddepspkg", false),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    // ...but redundant brackets are not (Portage `use_reduce` collapses them).
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "redundantbracketdepspkg", false),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_report_reports_without_reinstalling() {
    let result = graph_changed_deps_report("dev-libs/changeddepspkg", false, true);
    assert_eq!(
        result.changed_deps_report,
        vec![ChangedDepsReportEntry {
            category: "dev-libs".to_string(),
            package: "changeddepspkg".to_string(),
            version: "1.0".to_string(),
            repo_name: "testrepo".to_string(),
        }]
    );
    // Report-only: still AlreadyInstalled, never reinstalled.
    assert_eq!(
        result.entries[0].outcome,
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_report_is_silent_when_changed_deps_is_also_given() {
    // Silent when --changed-deps is also given; the second assert
    // proves the underlying change still reinstalls.
    let result = graph_changed_deps_report("dev-libs/changeddepspkg", true, true);
    assert!(result.changed_deps_report.is_empty());
    let changeddepspkg = result
        .entries
        .iter()
        .find(|e| e.package == "changeddepspkg")
        .expect("changeddepspkg entry");
    assert_eq!(
        changeddepspkg.outcome,
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn changed_deps_report_is_empty_without_the_flag() {
    let result = graph_changed_deps_report("dev-libs/changeddepspkg", false, false);
    assert!(result.changed_deps_report.is_empty());
}

#[test]
fn changed_deps_ignores_a_libc_only_dependency_change() {
    // glibc-vs-musl is libc noise both sides strip (Portage
    // `strip_libc_deps`), leaving identical deps: no reinstall.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "libcnoisepkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_does_not_fire_for_a_package_with_no_recorded_difference() {
    // No RDEPEND on either side flattens to an empty set: no reinstall.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "samepkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newrepo_reinstalls_when_vdb_repository_differs_from_the_repo_currently_providing_it() {
    // newrepopkg's vdb records "oldrepo"; the candidate lives in "testrepo".
    assert_eq!(
        resolve_real_newrepo("dev-libs", "newrepopkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: true,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn newrepo_does_not_fire_when_vdb_repository_matches() {
    // samerepopkg's vdb records "testrepo", matching the provider: no fire.
    assert_eq!(
        resolve_real_newrepo("dev-libs", "samerepopkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newrepo_fires_via_the_unknown_repo_sentinel_when_no_repository_file_was_ever_recorded() {
    // No vdb "repository" file: the `__unknown__` sentinel never
    // equals a repo name, so --newrepo fires (Portage `_unknown_repo`).
    assert_eq!(
        resolve_real_newrepo("dev-libs", "samepkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: true,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn split_slot_defaults_sub_slot_to_the_slot_itself_when_no_slash_is_present() {
    assert_eq!(split_slot("0"), ("0".to_string(), "0".to_string()));
    assert_eq!(split_slot("0/2"), ("0".to_string(), "2".to_string()));
    assert_eq!(split_slot(""), ("0".to_string(), "0".to_string()));
}

#[test]
fn newuse_vs_changed_use_diverge_on_a_newly_added_iuse_flag() {
    // --newuse reacts to a flag merely existing in IUSE now;
    // --changed-use only looks at enablement of shared flags.
    assert_eq!(
        resolve_real_newuse("dev-libs", "changedusepkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec!["brandnewflag".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    assert_eq!(
        resolve_real_changed_use("dev-libs", "changedusepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}
