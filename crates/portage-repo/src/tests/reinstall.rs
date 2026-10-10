use super::*;

#[test]
fn candidate_iuse_and_use_reads_a_binarys_own_baked_use() {
    // A binary candidate carries the flags it was built with
    // (`binary_use`) over its own `iuse` -- no md5-cache read, no
    // profile recompute (real `pkg.use.enabled` for a built package).
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
    // `ebuild_visible_at` asks the TREE (not the pool): `newpkg` has
    // a visible ebuild, `rebuiltbinarypkg` does not (it is the
    // ebuild-gone bolt).
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

    // real `identical_binary` (same BUILD_TIME) vs
    // `identical_binary`+`_equiv_ebuild_visible` (differing, ebuild
    // gone, --update): the latter only fires under `--update`
    // (real's `not avoid_update`).
    let root = TempDir::new("portuale-btre").keep();
    let vdb = root.join("var/db/pkg/dev-libs/rebuiltbinarypkg-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("BUILD_TIME"), "1000\n").unwrap();
    let newpkg_vdb = root.join("var/db/pkg/dev-libs/newpkg-1.0");
    std::fs::create_dir_all(&newpkg_vdb).unwrap();
    std::fs::write(newpkg_vdb.join("BUILD_TIME"), "1000\n").unwrap();

    // Identical build time -> never reinstalls, --update or not.
    assert!(
        !binary_reinstall_warranted(
            &root,
            &repos,
            &config,
            "dev-libs",
            "rebuiltbinarypkg",
            "1.0",
            Some(1000),
            false,
            None,
            true,
        ),
        "identical BUILD_TIME is identical_binary"
    );
    // Differing, ebuild gone, but no --update -> avoid_update keeps
    // the installed package.
    assert!(
        !binary_reinstall_warranted(
            &root,
            &repos,
            &config,
            "dev-libs",
            "rebuiltbinarypkg",
            "1.0",
            Some(2000),
            false,
            None,
            false,
        ),
        "without --update the installed instance is kept"
    );
    // Differing, ebuild gone, --update -> reinstall.
    assert!(
        binary_reinstall_warranted(
            &root,
            &repos,
            &config,
            "dev-libs",
            "rebuiltbinarypkg",
            "1.0",
            Some(2000),
            false,
            None,
            true,
        ),
        "differing BUILD_TIME + ebuild gone + --update reinstalls"
    );
    // Differing but the ebuild IS visible (newpkg) -> no reinstall
    // without --rebuilt-binaries, regardless of --update.
    assert!(
        !binary_reinstall_warranted(
            &root,
            &repos,
            &config,
            "dev-libs",
            "newpkg",
            "1.0",
            Some(2000),
            false,
            None,
            true,
        ),
        "a visible ebuild keeps the installed instance"
    );
    // --rebuilt-binaries escalates: reinstall for a differing
    // BUILD_TIME even when the ebuild is visible.
    assert!(
        binary_reinstall_warranted(
            &root,
            &repos,
            &config,
            "dev-libs",
            "newpkg",
            "1.0",
            Some(2000),
            true,
            None,
            true,
        ),
        "--rebuilt-binaries reinstalls regardless of ebuild visibility"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn newuse_reinstalls_when_vdb_use_differs_from_current_use() {
    // dev-libs/reinstallpkg is installed at 1.0 with IUSE="foo" but an
    // empty vdb USE file (foo was off at merge time); the fixture
    // profile chain enables "foo" globally now, so --newuse must
    // report a Reinstall for the changed "foo" flag.
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
    // dev-libs/samepkg has no IUSE at all (declared or in the vdb),
    // so there's nothing for --newuse to detect a change in -- must
    // stay AlreadyInstalled even with --newuse enabled.
    assert_eq!(
        resolve_real_newuse("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newuse_forced_flags_suppresses_a_spurious_reinstall() {
    // dev-libs/usemaskreinstallpkg is installed with an empty vdb
    // IUSE, but its own ebuild now declares
    // IUSE="masked_newly_added_flag" -- a flag
    // fixtures/repo/profiles/base/use.mask masks off, so
    // it's never enabled either before or after. Without
    // reinstall_flags_for_use_change's own "flags -= forced_flags"
    // step (real depgraph.py's own line, ported exactly), this
    // would incorrectly report a Reinstall just because the flag
    // now exists in IUSE at all.
    assert_eq!(
        resolve_real_newuse("dev-libs", "usemaskreinstallpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_reinstalls_when_vdb_rdepend_differs_from_the_current_ebuild() {
    // dev-libs/changeddepspkg is installed with a vdb-recorded
    // RDEPEND="dev-libs/samepkg", but its current ebuild's own
    // RDEPEND is "dev-libs/newpkg" instead.
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
    // The changed dependency here is RDEPEND, which --with-bdeps=n
    // never excludes (only DEPEND/BDEPEND are ever dropped) -- so
    // this must still detect the change.
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
    // dev-libs/movedkeydepspkg's vdb recorded RDEPEND="dev-libs/samepkg";
    // its current ebuild has that exact atom in PDEPEND instead, with
    // nothing else on either side. The net atom set is identical, so
    // the pre-slice merged-into-one-string comparison saw no change;
    // the per-key comparison catches it (real `_changed_deps` compares
    // `built_deps` to `unbuilt_deps` element-wise).
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
    // dev-libs/slotopdepspkg's current ebuild has
    // RDEPEND="dev-libs/slotoptarget:="; its vdb recorded the built
    // form "dev-libs/slotoptarget:2=" (the slot it was merged
    // against). Real `strip_slots` normalizes the built `:2=` back to
    // `:=` before comparing, so this is NOT a changed dependency --
    // without that normalization every `:=` dep would spuriously
    // trigger a --changed-deps reinstall.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "slotopdepspkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_detects_an_any_of_group_reorder() {
    // dev-libs/anyofreorderdepspkg's vdb recorded
    // RDEPEND="|| ( dev-libs/reorderdepa dev-libs/reorderdepb )";
    // its current ebuild swaps the two alternatives. Real
    // `_changed_deps` compares structured `use_reduce(token_class=
    // Atom)` output, so the `||`-alternative order is significant --
    // the pre-slice flat set comparison missed it.
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
    // Faithful to real portage's own Python-list `!=`: order is
    // significant even in AND context, so dev-libs/orderchangeddepspkg
    // (vdb `reorderdepa reorderdepb`, ebuild `reorderdepb reorderdepa`)
    // is a change...
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
    // ...but a redundant-bracket difference is not (real `use_reduce`
    // collapses `( a b )` to `a b`): dev-libs/redundantbracketdepspkg
    // has vdb `reorderdepa reorderdepb` and ebuild
    // `( dev-libs/reorderdepa dev-libs/reorderdepb )`.
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
    // Real portage: "This is completely silent... if --changed-deps
    // ... is enabled" -- the Vec must stay empty even though the
    // underlying dependency change is real (changed_deps=true here
    // actually reinstalls it, proven by the second assertion).
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
    // dev-libs/libcnoisepkg's own vdb RDEPEND names sys-libs/glibc,
    // its current ebuild names sys-libs/musl instead -- both are
    // real virtual/libc providers (the fixture vdb's own
    // virtual/libc entry RDEPENDs on "|| ( sys-libs/glibc
    // sys-libs/musl )"), so real strip_libc_deps must strip both
    // before comparing, leaving only the identical
    // "dev-libs/samepkg" on each side -- no reinstall.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "libcnoisepkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_deps_does_not_fire_for_a_package_with_no_recorded_difference() {
    // dev-libs/samepkg's own vdb has no RDEPEND file at all, and its
    // current ebuild declares none either -- both sides flatten to
    // an empty set, so no reinstall is reported even with
    // --changed-deps enabled.
    assert_eq!(
        resolve_real_changed_deps("dev-libs", "samepkg", true),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newrepo_reinstalls_when_vdb_repository_differs_from_the_repo_currently_providing_it() {
    // dev-libs/newrepopkg is installed with a vdb "repository" file
    // recording "oldrepo", but the current best candidate for this
    // exact version lives in "testrepo" instead.
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
    // dev-libs/samerepopkg is installed with a vdb "repository" file
    // recording "testrepo" -- exactly matching the repo that
    // currently provides this version, so --newrepo must not fire.
    assert_eq!(
        resolve_real_newrepo("dev-libs", "samerepopkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn newrepo_fires_via_the_unknown_repo_sentinel_when_no_repository_file_was_ever_recorded() {
    // dev-libs/samepkg has no vdb "repository" file at all (real
    // portage predates this tracking, or a hand-installed/synthetic
    // entry) -- real portage.versions._unknown_repo ("__unknown__")
    // applies, which never equals a real repo name, so --newrepo
    // still fires even though nothing about this package actually
    // changed.
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
    // dev-libs/changedusepkg is installed with an empty vdb IUSE,
    // and its current ebuild now declares IUSE="brandnewflag" -- a
    // real, unmasked, not-globally-enabled flag. --newuse's own
    // presence-diff term reacts to the flag simply existing in IUSE
    // now, regardless of enablement; --changed-use's own, narrower
    // formula never looks at IUSE presence at all, only at
    // enablement of flags declared on both sides -- proving the two
    // are genuinely different checks.
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
