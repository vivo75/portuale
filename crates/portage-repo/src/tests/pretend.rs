use super::*;

/// Under `--usepkgonly` a dependency satisfied only by an installed
/// version with no binpkg still resolves `AlreadyInstalled`.
#[test]
fn usepkgonly_dependency_satisfied_by_an_installed_version_with_no_binpkg() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    // dev-libs/samepkg: installed (vdb), has an ebuild, has no binpkg.
    let call = |usepkgonly: bool, is_top_level: bool| {
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config()),
            },
            "dev-libs/samepkg",
            &SelectOptions {
                selective: false,
                is_top_level,
                usepkgonly,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend(dev-libs/samepkg) failed")
    };
    assert_eq!(
        call(true, false),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
    // A top-level atom still reports the missing binary.
    assert_eq!(call(true, true), PretendOutcome::NoVisibleCandidate);
    assert_eq!(
        call(false, false),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

/// A dependency atom `foo[bar]` whose installed `foo` lacks `bar`
/// reinstalls instead of resolving `AlreadyInstalled`.
#[test]
fn installed_dep_missing_a_use_flag_reinstalls_not_already_installed() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    // flipinstdep installed at 1.0 without `wantflag`; flipinstconsumer RDEPENDs `flipinstdep[wantflag]`.
    let call = |autounmask_use: bool| {
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config()),
            },
            "dev-libs/flipinstdep[wantflag]",
            &SelectOptions {
                selective: false,
                usepkg: false,
                autounmask_use,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend(flipinstdep[wantflag]) failed")
    };
    assert_eq!(
        call(true),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    // `--autounmask-use=n`: the `matched` filter drops the candidate entirely instead.
    assert_eq!(call(false), PretendOutcome::NoVisibleCandidate);
}

#[test]
fn emptytree_reinstalls_an_installed_dependency_that_would_otherwise_be_hidden() {
    // Under --emptytree an installed dep becomes a bare Reinstall
    // instead of AlreadyInstalled.
    assert_eq!(
        graph_empty("dev-libs/deeppkg")
            .into_iter()
            .find(|(name, _)| name == "dev-libs/deeppkg2")
            .map(|(_, o)| o),
        Some(PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec![],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        })
    );
}

#[test]
fn resolve_pretend_matches_a_sub_slot_restricted_atom_against_the_real_sub_slot() {
    // Exact sub-slot match on the plain PMS 8.3.3 `:slot/sub-slot`
    // form (unlike `:slot=` atoms, this needs candidate sub-slot data).
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let outcome = resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        "dev-libs/subslotpkg:0/2",
        &SelectOptions {
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .expect("resolve_pretend must succeed");
    assert_eq!(
        outcome,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn resolve_pretend_rejects_a_sub_slot_restricted_atom_that_does_not_match() {
    // Same candidate (SLOT "0/2") but sub-slot "3": a genuine
    // mismatch, not an always-accept path.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let outcome = resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        "dev-libs/subslotpkg:0/3",
        &SelectOptions {
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .expect("resolve_pretend must succeed");
    assert_eq!(outcome, PretendOutcome::NoVisibleCandidate);
}

/// Only a `!=cpv[binary]` negative reads as a
/// `slot_operator_mask_built` record; other negatives read as `None`.
#[test]
fn slot_operator_binary_masked_version_reads_only_marked_negatives() {
    assert_eq!(
        slot_operator_binary_masked_version(
            "!=dev-libs/maskchild-1.0[binary]",
            "dev-libs",
            "maskchild"
        ),
        Some("1.0".to_string())
    );
    assert_eq!(
        slot_operator_binary_masked_version("!=dev-libs/maskchild-1.0", "dev-libs", "maskchild"),
        None
    );
    assert_eq!(
        slot_operator_binary_masked_version(
            "!=dev-libs/other-1.0[binary]",
            "dev-libs",
            "maskchild"
        ),
        None
    );
    assert_eq!(
        slot_operator_binary_masked_version(
            "=dev-libs/maskchild-1.0[binary]",
            "dev-libs",
            "maskchild"
        ),
        None
    );
    assert!(is_binary_only_mask("!=dev-libs/maskchild-1.0[binary]"));
    assert!(!is_binary_only_mask("!=dev-libs/maskchild-1.0"));
    assert!(!is_binary_only_mask("=dev-libs/maskchild-1.0[binary]"));
}

/// A stale built slot-operator dep with no ebuild reports
/// `NoVisibleCandidate` at this level; the walk rewrites the
/// dependency case one layer up.
#[test]
fn stale_built_slot_operator_dep_with_no_ebuild_reports_at_this_level() {
    // The L1-a gate stands here: no usable ebuild means
    // `NoVisibleCandidate` even when the vdb could satisfy it.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let call = |is_top_level: bool| {
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config()),
            },
            "dev-libs/reinstslottarget:0/1=",
            &SelectOptions {
                newuse: true,
                update: true,
                with_bdeps: false,
                changed_slot: true,
                is_top_level,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend(dev-libs/reinstslottarget:0/1=) failed")
    };
    assert_eq!(call(false), PretendOutcome::NoVisibleCandidate);
    assert_eq!(call(true), PretendOutcome::NoVisibleCandidate);
}

/// Portage `_too_deep` (`depgraph.py:7359-7375`) as S2 defines it directly:
/// with no `--deep` every dep depth is too deep, unlimited `--deep` never
/// is, `--deep=N` draws the line past `N`. Deliberately not negated
/// `Deep::recurses_at` (that answers whether an installed package's own
/// deps are walked — a different question with a different default).
#[test]
fn update_too_deep_matches_the_portage_depth_rule() {
    assert!(!update_too_deep(Deep::NotRequested, 0));
    assert!(update_too_deep(Deep::NotRequested, 1));
    assert!(!update_too_deep(Deep::Unlimited, 1));
    assert!(!update_too_deep(Deep::Unlimited, 99));
    assert!(!update_too_deep(Deep::Bounded(1), 1));
    assert!(update_too_deep(Deep::Bounded(1), 2));
}

/// The `_iter_atoms_for_pkg` argument match behind the depth reset: a bare
/// argument names every installed version, anything else must really match.
#[test]
fn installed_matches_argument_names_only_matching_arguments() {
    let arg = |arg_atoms: &[String],
               category: &str,
               package: &str,
               version: &str,
               slot: &str,
               sub_slot: &str,
               repo_name: &str| {
        installed_matches_argument(InstalledArgumentMatchArgs {
            arg_atoms,
            category,
            package,
            version,
            slot,
            sub_slot,
            repo_name,
        })
    };
    let args = ["dev-libs/pg34-dlib".to_string()];
    assert!(arg(
        &args,
        "dev-libs",
        "pg34-dlib",
        "1",
        "0",
        "0",
        "testrepo"
    ));
    assert!(!arg(
        &args,
        "dev-libs",
        "pg34-dtop",
        "1",
        "0",
        "0",
        "testrepo"
    ));
    assert!(!arg(
        &[">=dev-libs/pg34-dlib-2.0".to_string()],
        "dev-libs",
        "pg34-dlib",
        "1",
        "0",
        "0",
        "testrepo"
    ));
    assert!(!arg(
        &["dev-libs/pg34-dlib:1".to_string()],
        "dev-libs",
        "pg34-dlib",
        "1",
        "0",
        "0",
        "testrepo"
    ));
}

/// A plain dependency under `--update` without `--deep` is not wanted for
/// update: the installed instance is kept (Portage's `:8445-8458` return,
/// whose rows the S0 `362-dtop-pvu` capture pins to `N pg34-dtop` alone).
#[test]
fn update_without_deep_keeps_an_installed_dependency() {
    assert_eq!(
        resolve_dep_update("dev-libs/pg34-dlib"),
        PretendOutcome::AlreadyInstalled {
            version: "1".to_string()
        }
    );
}

/// A keyword-masked installed dependency under `--update` is kept without
/// any visibility check (Portage's `:7972-7979` keep, whose rows the S0
/// `dbg-needskeywordmasked-pu` capture pins to `N needskeywordmasked`).
#[test]
fn update_without_deep_keeps_a_masked_installed_dependency() {
    assert_eq!(
        resolve_dep_update("dev-libs/keywordmaskedpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string()
        }
    );
}

/// A USE-dep satisfied by the installed instance's own recorded USE still
/// counts as installed (Portage's vdb-side check; the S0 `s0-a-maskedusedrift`
/// capture pins the parent-only rows for this shape).
#[test]
fn update_without_deep_keeps_an_installed_use_dep_satisfier() {
    assert_eq!(
        resolve_dep_update("dev-libs/keywordmaskedusepkg[flag]"),
        PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string()
        }
    );
}

/// An installed version with no tree candidate at all is still kept when
/// not wanted (Portage walks the vardb independently of the ebuild pool;
/// the S0 `s0-b-toolchainroot` capture pins `N plainleaf N toolchainroot`,
/// and the `360-gapp-pv` capture pins `N pg34-gapp` for the same shape).
#[test]
fn update_without_deep_keeps_an_installed_version_with_no_ebuild() {
    assert_eq!(
        resolve_dep_update("virtual/libc"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
    assert_eq!(
        resolve_dep_update("dev-libs/pg34-gone"),
        PretendOutcome::AlreadyInstalled {
            version: "1".to_string()
        }
    );
}

/// Naming the dependency as an argument resets its depth to 0, so it
/// updates after all (Portage's `reset_depth`; the S0 `362-dtop-pvu-argdlib`
/// capture pins `U pg34-dlib-2 N pg34-dtop`).
#[test]
fn an_argument_names_its_dependency_wanted_for_update() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let args = vec![
        "dev-libs/pg34-dtop".to_string(),
        "dev-libs/pg34-dlib".to_string(),
    ];
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config()),
            },
            "dev-libs/pg34-dlib",
            &SelectOptions {
                update: true,
                depth: 1,
                arg_atoms: &args,
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend(dev-libs/pg34-dlib) failed"),
        PretendOutcome::Upgrade {
            from: "1".to_string(),
            to: "2".to_string(),
        }
    );
}

/// `--deep=1` reaches a depth-1 dependency, so it updates (Portage's
/// `_too_deep`; the reused `362-dtop-pvu-deep1` capture pins
/// `U pg34-dlib-2 N pg34-dtop`).
#[test]
fn deep_one_reaches_a_depth_one_dependency() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    assert_eq!(
        resolve_pretend(
            &SelectSource {
                repos: &repos,
                root: &root,
                config: &test_config(),
                local_binpkg: &build_local_binpkg_index(&test_config()),
            },
            "dev-libs/pg34-dlib",
            &SelectOptions {
                update: true,
                depth: 1,
                deep: Deep::Bounded(1),
                ..SelectOptions::default()
            },
        )
        .expect("resolve_pretend(dev-libs/pg34-dlib) failed"),
        PretendOutcome::Upgrade {
            from: "1".to_string(),
            to: "2".to_string(),
        }
    );
}

/// A USE-dep flag no tree candidate carries in IUSE never resolves to the
/// installed instance, even when the vdb-recorded USE has it: Portage
/// reports the dep unsatisfiable (`Missing IUSE`) instead. Oracle: host
/// Portage 3.0.82.2 `s2-c-needsbuilt-pu` in `LLM/evidence/2026-10-16-359-exec/`
/// (rc 1, `no ebuilds built with USE flags ... [divergedflag]`).
#[test]
fn update_skips_an_installed_use_dep_satisfier_unknown_to_the_tree() {
    assert_eq!(
        resolve_dep_update("dev-libs/builtusedivergedep[divergedflag]"),
        PretendOutcome::NoVisibleCandidate
    );
}
