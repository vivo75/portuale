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
        eapi: "8".to_string(),
        binary_deps: HashMap::new(),
        user_patches: String::new(),
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
        eapi: "8".to_string(),
        binary_deps: HashMap::new(),
        user_patches: String::new(),
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

/// RAII reset for the `--binpkg-changed-deps` process-global override: the
/// CLI sets it once per run, so tests that need the explicit `=y` shape set
/// it and restore `None` (auto) on drop, even on panic. Only
/// `--usepkgonly` resolves can observe the difference (every other mode is
/// auto-on already), and no other test resolves `--usepkgonly` with a
/// deps-divergent binary, so the window is unobservable — except between
/// the guard holders themselves, which the mutex serializes.
static CHANGED_DEPS_OVERRIDE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct ChangedDepsOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}
impl ChangedDepsOverrideGuard {
    fn force_on() -> Self {
        let lock = CHANGED_DEPS_OVERRIDE_LOCK.lock().unwrap();
        set_binpkg_changed_deps_override(Some(true));
        ChangedDepsOverrideGuard { _lock: lock }
    }
}
impl Drop for ChangedDepsOverrideGuard {
    fn drop(&mut self) {
        set_binpkg_changed_deps_override(None);
    }
}

/// The fixture resolve config with the binhost the `Packages` index
/// actually lives at: `resolve_config` keeps the fixture binrepos.conf's
/// literal `${PORTAGE_CONFIGROOT}` placeholder (it expands from the process
/// env, unset under test), so point the request at the tree directly.
fn config_420_with_binhost() -> portage_profile::Config {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    config.binrepos = vec![fixture_binhost()];
    config
}

/// Backlog #420 cell B: `--getbinpkgonly --binpkg-changed-deps=y
/// dev-libs/bcdeppkg` aborts with Portage's masked block (`masked by:
/// changed deps`), not the plain `no binary packages` miss. The tree
/// `RDEPEND`s `dev-libs/bcdepnew`; the binhost binary was built
/// `RDEPEND`ing `dev-libs/bcdepold`.
///
/// Portage 3.0.82.2, s10x/root420 cell B (rc 1, full argv `emerge -pv
/// --package-moves=n --usepkgonly=y --binpkg-changed-deps=y
/// dev-libs/t420cdep`, stderr `- dev-libs/t420cdep-1.0::t420repo (masked
/// by: changed deps)` with no chain lines). Red before the slice: the
/// stale binary vanished into `there are no binary packages to satisfy`.
#[test]
fn changed_deps_top_level_reports_the_masked_block() {
    let _guard = ChangedDepsOverrideGuard::force_on();
    let root = fixtures_root();
    let config = config_420_with_binhost();
    // The CLI folds `--getbinpkgonly` into `usepkgonly` + `getbinpkg`.
    let err = ResolveRequest {
        usepkgonly: true,
        getbinpkg: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/bcdeppkg".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect_err("the stale binary is the only candidate: top-level aborts");
    assert_eq!(
        err.to_string(),
        // The binary-pool header is Portage's own under `--usepkgonly`
        // (`All binary packages that could satisfy`, backlog #509 oracle
        // shape 03; depgraph.py 7018-7023): the #420 oracle pinned only
        // the `changed deps` row, which is unchanged.
        "\n!!! All binary packages that could satisfy \"dev-libs/bcdeppkg\" have been masked.\n\
         !!! One of the following masked packages is required to complete your request:\n\
         - dev-libs/bcdeppkg-1.0::gentoo (masked by: changed deps)\n\
         \n\
         For more information, see the MASKED PACKAGES section in the emerge\n\
         man page or refer to the Gentoo Handbook.\n"
    );
}

/// Backlog #420 cell C: the dependency disclosure merges the stale binary
/// rows into the masked block with the parent/argument chain (chain
/// rendering comes free from `finalize_dep_reports`). The parent resolves
/// from its own binhost row; the dep is the same stale `bcdeppkg` binary.
///
/// Portage 3.0.82.2, s10x/root420 cell C (rc 1, full argv `emerge -pv
/// --package-moves=n --usepkgonly=y --binpkg-changed-deps=y
/// dev-libs/t420cparent`): the (B) block plus `(dependency required by
/// "…t420cparent-1.0::t420repo" [binary])` / `(… "…t420cparent"
/// [argument])`. Red before the slice: the plain miss with the same chains.
#[test]
fn changed_deps_dependency_reports_the_masked_block() {
    let _guard = ChangedDepsOverrideGuard::force_on();
    let root = fixtures_root();
    let config = config_420_with_binhost();
    let result = ResolveRequest {
        usepkgonly: true,
        getbinpkg: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/bcdepparent".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("a dependency miss aborts with disclosures, not Err: {e}"));
    assert!(
        matches!(&result.outcome, ResolveOutcome::Aborted { .. }),
        "stale dep aborts: {:?}",
        result.outcome
    );
    assert_eq!(result.masked_deps.len(), 1);
    let rep = &result.masked_deps[0];
    assert_eq!(
        (
            rep.category.as_str(),
            rep.package.as_str(),
            rep.atom.as_str()
        ),
        ("dev-libs", "bcdeppkg", "dev-libs/bcdeppkg")
    );
    assert_eq!(
        rep.masked,
        vec![(
            "dev-libs/bcdeppkg-1.0::gentoo".to_string(),
            vec!["changed deps".to_string()]
        )]
    );
    assert_eq!(
        rep.chain,
        vec![
            (
                "dev-libs/bcdepparent-1.0::gentoo".to_string(),
                "binary".to_string()
            ),
            ("dev-libs/bcdepparent".to_string(), "argument".to_string()),
        ]
    );
}

/// Backlog #420 cell A: the dependency respect-use disclosure — a parent
/// binary whose dep's only builds are USE-mismatched reports Portage's
/// masked block, cpv-deduped to one row, instead of the plain miss. The
/// two-instance `Packages` is staged through `scanned_binpkgs` (the
/// `_t341_bid_cell_configroot` pytest helper builds the same index on
/// disk); the user line mismatches both builds like the #402 top-level
/// cell (`=dev-libs/t341-dep-1-2 x` with build 1 baked `x`, build 2 baked
/// empty).
///
/// Portage 3.0.82.2, s10x/root420 cell A (rc 1, full argv `emerge -pv
/// --package-moves=n --usepkgonly=y --binpkg-respect-use=y
/// dev-libs/t420parent`): `!!! All ebuilds that could satisfy
/// "dev-libs/t420dep" …` + `- dev-libs/t420dep-1::t420repo (masked by: use
/// flag configuration mismatch)` + the `[binary]`/`[argument]` chains. Red
/// before the slice: `there are no binary packages to satisfy` with the
/// same chains.
#[test]
fn respect_use_dependency_reports_the_masked_block() {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let stanza = |cpv: &str, extra: &[(&str, &str)]| {
        let mut m = HashMap::from([
            ("CPV".to_string(), cpv.to_string()),
            ("EAPI".to_string(), "8".to_string()),
            ("KEYWORDS".to_string(), "amd64".to_string()),
            ("SLOT".to_string(), "0".to_string()),
            ("REPO".to_string(), "t341".to_string()),
        ]);
        for (k, v) in extra {
            m.insert(k.to_string(), v.to_string());
        }
        m
    };
    config.scanned_binpkgs = Some(vec![
        stanza(
            "app-misc/t341-needdep-1",
            &[("RDEPEND", "dev-libs/t341-dep")],
        ),
        stanza(
            "dev-libs/t341-dep-1",
            &[
                ("SLOT", "0/1"),
                ("IUSE", "x"),
                ("USE", "x"),
                ("BUILD_ID", "1"),
                ("BUILD_TIME", "1000"),
            ],
        ),
        stanza(
            "dev-libs/t341-dep-1",
            &[
                ("SLOT", "0/1"),
                ("IUSE", "x"),
                ("USE", ""),
                ("BUILD_ID", "2"),
                ("BUILD_TIME", "2000"),
            ],
        ),
    ]);
    config.package_use_user = vec![("=dev-libs/t341-dep-1-2".to_string(), vec!["x".to_string()])];
    let result = ResolveRequest {
        usepkgonly: true,
        binpkg_respect_use: true,
        binpkg_respect_user_patches: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &["app-misc/t341-needdep".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("a dependency miss aborts with disclosures, not Err: {e}"));
    assert!(
        matches!(&result.outcome, ResolveOutcome::Aborted { .. }),
        "mismatched dep aborts: {:?}",
        result.outcome
    );
    assert_eq!(result.masked_deps.len(), 1);
    let rep = &result.masked_deps[0];
    assert_eq!(
        (
            rep.category.as_str(),
            rep.package.as_str(),
            rep.atom.as_str()
        ),
        ("dev-libs", "t341-dep", "dev-libs/t341-dep")
    );
    assert_eq!(
        rep.masked,
        vec![(
            "dev-libs/t341-dep-1::t341".to_string(),
            vec!["use flag configuration mismatch".to_string()]
        )]
    );
    assert_eq!(
        rep.chain,
        vec![
            (
                "app-misc/t341-needdep-1::t341".to_string(),
                "binary".to_string()
            ),
            ("app-misc/t341-needdep".to_string(), "argument".to_string()),
        ]
    );
}

#[test]
fn binary_deps_changed_answers_per_recorded_deps_even_when_asked_repeatedly() {
    // `dev-libs/schedok-1.0` has no dependencies in its ebuild. The answer is
    // memoised, so each recorded-deps variant must key separately and repeat
    // calls must agree.
    let repos = find_repos(&fixtures_root()).expect("fixture repos.conf resolves");
    let root = fixtures_root();
    let binary = |rdepend: &str| Candidate {
        version: "1.0".to_string(),
        keywords: vec!["amd64".to_string()],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: i32::MIN,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Binary,
        binary_use: Some(HashSet::new()),
        remote: false,
        build_id: Some("1".to_string()),
        build_time: Some(1),
        eapi: "8".to_string(),
        binary_deps: HashMap::from([("RDEPEND".to_string(), rdepend.to_string())]),
        user_patches: String::new(),
    };
    let ask = |c: &Candidate| {
        binary_deps_changed(BinaryDepsChangedArgs {
            root: &root,
            repos: &repos,
            candidate: c,
            category: "dev-libs",
            package: "schedok",
            with_bdeps: false,
        })
    };
    let same = binary("");
    let stale = binary("dev-libs/not-in-the-ebuild");
    for _ in 0..3 {
        assert!(!ask(&same));
        assert!(ask(&stale));
    }
}
