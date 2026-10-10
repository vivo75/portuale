use super::*;

/// L1-a: under `--usepkgonly` the ebuild candidate pool is emptied
/// wholesale, so a dependency whose only satisfier is an installed
/// version with no binary package must still resolve `AlreadyInstalled`
/// (real `--usepkgonly` restricts the *merge* pool, not installed-
/// satisfaction) -- not `NoVisibleCandidate`, which in a real `-K`
/// merge lands on the resume list ("no visible ebuild to merge").
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
    // As a dependency under --usepkgonly: satisfied by the vdb.
    assert_eq!(
        call(true, false),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
    // A top-level --usepkgonly atom still reports the missing binary
    // (real requires something mergeable for a named package).
    assert_eq!(call(true, true), PretendOutcome::NoVisibleCandidate);
    // Without --usepkg the ebuild pool is intact -> the ordinary
    // installed-vs-ebuild path already handled it (unchanged).
    assert_eq!(
        call(false, false),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

/// L0 finding L: a dependency atom `foo[bar]` whose already-installed
/// `foo` lacks `bar` must not resolve `AlreadyInstalled` -- real
/// portage reinstalls `foo` with `bar` (and autounmasks it). Gated
/// on `autounmask_use`.
#[test]
fn installed_dep_missing_a_use_flag_reinstalls_not_already_installed() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    // dev-libs/flipinstdep: installed at 1.0 without `wantflag`
    // (in IUSE, default-off); `dev-libs/flipinstconsumer` RDEPENDs
    // `dev-libs/flipinstdep[wantflag]`.
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
                // is_top_level (a dependency)
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
    // `--autounmask-use=n`: the earlier `matched` filter drops the
    // candidate entirely instead (no flip allowed).
    assert_eq!(call(false), PretendOutcome::NoVisibleCandidate);
}

#[test]
fn emptytree_reinstalls_an_installed_dependency_that_would_otherwise_be_hidden() {
    // dev-libs/deeppkg2 is installed and satisfies deeppkg's RDEPEND,
    // so without --deep/--emptytree it's AlreadyInstalled (and not
    // shown). Under --emptytree it becomes a bare Reinstall (the dep
    // path runs `dependency_avoid_update_candidate` ->
    // `already_installed_or_reinstall(empty=true)`).
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
    // dev-libs/subslotpkg's own SLOT is "0/2" -- an exact sub-slot
    // match, unlike a slot-operator atom (":="/"pkg:slot="), which
    // `matches_slot`'s own doc comment already established needs no
    // candidate-side sub-slot data at all to match correctly. This
    // is the plain PMS 8.3.3 "cat/pkg:slot/sub-slot" restriction
    // form instead, which DOES need it -- see `Candidate::sub_slot`'s
    // own doc comment for the bug this closes.
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
    // Same candidate (SLOT "0/2"), but the atom itself asks for
    // sub-slot "3" -- a genuine mismatch, proving this is real
    // matching and not just "always accept regardless of sub-slot".
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

/// Backlog #212 (v2 `#24c`): only a `!=cpv[binary]` negative reads
/// as a `slot_operator_mask_built` record. A plain `!=cpv` (real's
/// slot-conflict / missing-dependency negatives), another package's
/// marked negative, and a positive marked atom all read as `None`;
/// [`is_binary_only_mask`] answers the same question for the
/// string-constraint readers.
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
