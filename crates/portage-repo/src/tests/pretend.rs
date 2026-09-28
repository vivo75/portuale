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
