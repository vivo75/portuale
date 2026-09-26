use super::*;

/// The fixture tree lives in the sibling pmtest checkout: `fixtures/` is a
/// symlink to `../pmtest/fixtures` (see `LLM/agent-context.md`, "Where the
/// tests live"). This test says why when the sibling is missing.
#[test]
fn fixtures_tree_is_the_pmtest_checkout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    assert!(
        root.is_dir(),
        "{} does not resolve -- `fixtures/` is a symlink to ../pmtest/fixtures: \
         check out the sibling pmtest repo next to this one",
        root.display()
    );
    // Markers, not a full inventory: enough to catch a tree that resolves but
    // is not the fixture tree.
    for marker in [
        "repo/profiles/make.defaults",
        "repo/metadata/md5-cache",
        "etc/portage/make.conf",
        "var/db/pkg",
    ] {
        assert!(
            root.join(marker).exists(),
            "fixtures tree at {} is missing {marker}: it resolves, but it is \
             not the pmtest fixture tree",
            root.display()
        );
    }
}

/// The shared field set is Portage's own 23 names (format version 1),
/// listed literally so a drift fails loudly.
#[test]
fn metadata_file_fields_match_portages_literal_set() {
    assert_eq!(METADATA_FILE_FORMAT_VERSION, 1);
    let mut got: Vec<&str> = METADATA_FILE_FIELDS.to_vec();
    got.sort_unstable();
    assert_eq!(
        got,
        vec![
            "BDEPEND",
            "BUILD_ID",
            "BUILD_TIME",
            "CHOST",
            "COUNTER",
            "DEFINED_PHASES",
            "DEPEND",
            "DESCRIPTION",
            "EAPI",
            "HOMEPAGE",
            "IDEPEND",
            "IUSE",
            "KEYWORDS",
            "LICENSE",
            "PDEPEND",
            "PROPERTIES",
            "PROVIDES",
            "RDEPEND",
            "REQUIRES",
            "RESTRICT",
            "SLOT",
            "USE",
            "repository",
        ]
    );
}

/// `in_metadata_file` is Portage's `_in_metadata_file()`: the line-oriented
/// `CONTENTS` and `NEEDED.*` fields are rejected from the snapshot.
#[test]
fn in_metadata_file_rejects_contents_and_needed() {
    assert!(!in_metadata_file("CONTENTS"));
    assert!(!in_metadata_file("NEEDED.ELF.2"));
    assert!(!in_metadata_file("_mtime_"));
    assert!(in_metadata_file("RDEPEND"));
    assert!(in_metadata_file("repository"));
}

#[test]
fn emptytree_forces_an_installed_atom_to_a_bare_reinstall() {
    // Without --emptytree a selective resolve leaves it AlreadyInstalled;
    // --emptytree (Portage pops "selective" and forces the merge) makes it a
    // bare Reinstall.
    assert_eq!(
        resolve("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
    assert_eq!(
        resolve_empty("dev-libs", "samepkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec![],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn not_selective_top_level_atom_reinstalls_with_no_reason_when_nothing_else_changed() {
    assert_eq!(
        resolve_not_selective("dev-libs", "samepkg"),
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
}

#[test]
fn not_selective_top_level_atom_still_offers_a_newer_visible_version() {
    // Without `update`, Portage's avoid_update shortcut never fires for a
    // not-selective top-level atom, so the best-visible search finds 2.0.
    assert_eq!(
        resolve_not_selective("dev-libs", "upgradepkg"),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        }
    );
}

#[test]
fn selective_true_preserves_the_pre_existing_already_installed_outcome() {
    // Same fixture, selective=true: must still be AlreadyInstalled.
    assert_eq!(
        resolve("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn new_install() {
    assert_eq!(
        resolve("dev-libs", "newpkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn already_installed() {
    assert_eq!(
        resolve("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn without_update_an_installed_version_that_still_satisfies_the_atom_is_kept() {
    // Plain `emerge` with no --update never looks for a better version
    // (Portage's avoid_update).
    assert_eq!(
        resolve("dev-libs", "upgradepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn upgrade() {
    // Same fixture with --update: a genuine Upgrade (Portage's dont_miss_updates).
    assert_eq!(
        resolve_update("dev-libs", "upgradepkg"),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        }
    );
}

#[test]
fn downgrade() {
    // Only 1.0 is visible (2.0's ebuild is gone): Portage's in-slot `best()`
    // flags a genuine downgrade, and avoid_update needs a visible candidate.
    assert_eq!(
        resolve("dev-libs", "downgradepkg"),
        PretendOutcome::Downgrade {
            from: "2.0".to_string(),
            to: "1.0".to_string(),
        }
    );
}

#[test]
fn keyword_masked_but_installed_dependency_is_kept_not_downgraded() {
    // Installed ~amd64-only 2.0 with only 1.0 visible, reached as a DEPENDENCY
    // atom: Portage's early avoid_update return needs no visibility check,
    // so it stays installed.
    assert_eq!(
        resolve_as_dependency("dev-libs", "keywordmaskedpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string()
        }
    );
}

#[test]
fn keyword_masked_but_installed_dependency_still_requires_visibility_as_a_top_level_atom() {
    // Same fixture as a TOP-LEVEL atom: Portage's separate, later avoid_update
    // block DOES require visibility.
    assert_eq!(
        resolve("dev-libs", "keywordmaskedpkg"),
        PretendOutcome::Downgrade {
            from: "2.0".to_string(),
            to: "1.0".to_string(),
        }
    );
}

#[test]
fn masked_by_keywords_is_not_visible() {
    assert_eq!(
        resolve("dev-libs", "maskedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn nonexistent_package_is_no_visible_candidate() {
    assert_eq!(
        resolve("dev-libs", "does-not-exist"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn without_newuse_the_same_package_stays_already_installed() {
    // Without --newuse the USE mismatch goes undetected: stays AlreadyInstalled.
    assert_eq!(
        resolve_real("dev-libs", "reinstallpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn without_changed_deps_the_same_package_stays_already_installed() {
    assert_eq!(
        resolve_real("dev-libs", "changeddepspkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_slot_reinstalls_when_vdb_slot_differs_from_the_current_ebuild() {
    // dev-libs/changedslotpkg is installed with a vdb-recorded
    // SLOT="0", but its current ebuild's own SLOT is "0/2" instead
    // (an ABI-bump sub-slot change).
    assert_eq!(
        resolve_real_changed_slot("dev-libs", "changedslotpkg"),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
}

#[test]
fn without_changed_slot_the_same_package_stays_already_installed() {
    assert_eq!(
        resolve_real("dev-libs", "changedslotpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn changed_use_still_catches_an_enablement_change_on_a_shared_flag() {
    // dev-libs/reinstallpkg's own "foo" flag exists in IUSE on both
    // sides -- only its enablement changed, exactly the shared term
    // both --newuse and --changed-use compute the same way.
    assert_eq!(
        resolve_real_changed_use("dev-libs", "reinstallpkg"),
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
fn fixture_overlay_only_package_is_found() {
    // dev-libs/overlayonlypkg exists only in the overlay repo: proves the
    // overlay is actually searched, not just present in repos.conf.
    assert_eq!(
        resolve_real("dev-libs", "overlayonlypkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_best_version_wins_regardless_of_which_repo_has_it() {
    // -2.0 is in the overlay: the higher version wins anyway.
    assert_eq!(
        resolve_real("dev-libs", "overlaynewerpkg"),
        PretendOutcome::New {
            version: "2.0".to_string()
        }
    );
}

#[test]
fn fixture_package_mask_hides_with_no_unmask() {
    assert_eq!(
        resolve_real("dev-libs", "hardmaskedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_package_unmask_cancels_the_matching_mask() {
    assert_eq!(
        resolve_real("dev-libs", "maskedandunmaskedpkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_package_mask_minus_atom_removal_is_unaffected() {
    // Masked then un-masked via "-dev-libs/samepkg" within package.mask itself:
    // resolves normally, not as if ever masked.
    assert_eq!(
        resolve_real("dev-libs", "samepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_package_accept_keywords_wildcard_extends_visibility() {
    assert_eq!(
        resolve_real("dev-libs", "wildcardkeywordpkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_package_accept_keywords_double_star_accepts_no_keywords_package() {
    assert_eq!(
        resolve_real("dev-libs", "livekeywordpkg"),
        PretendOutcome::New {
            version: "9999".to_string()
        }
    );
}

#[test]
fn fixture_unrelated_masked_by_keywords_package_is_still_hidden() {
    // Regression guard: the "*"-scoped accept_keywords entry must not leak
    // onto dev-libs/maskedpkg.
    assert_eq!(
        resolve_real("dev-libs", "maskedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_eula_style_license_is_masked_by_the_portage_default_accept_license() {
    // No ACCEPT_LICENSE set anywhere: Portage's own "* -@EULA" default applies.
    assert_eq!(
        resolve_real("dev-libs", "eulapkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_any_of_license_group_is_visible_via_the_accepted_alternative() {
    assert_eq!(
        resolve_real("dev-libs", "anyoflicensepkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_package_license_unmasks_an_otherwise_eula_masked_package() {
    // package.license accepts SomeEula for this package specifically.
    assert_eq!(
        resolve_real("dev-libs", "packagelicensepkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_use_conditional_license_is_visible_when_the_flag_is_off() {
    assert_eq!(
        resolve_real("dev-libs", "uselicensepkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_use_conditional_license_is_masked_once_package_use_forces_the_flag_on() {
    // package.use forces the flag on, activating the conditional -- masked by
    // the same Portage "* -@EULA" default as dev-libs/eulapkg.
    assert_eq!(
        resolve_real("dev-libs", "uselicensepkgforced"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_properties_default_star_accepts_a_declared_property() {
    assert_eq!(
        resolve_real("dev-libs", "propertiespkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_package_properties_narrows_acceptance_for_one_package() {
    // package.properties revokes "interactive" for this package specifically,
    // despite the permissive global default.
    assert_eq!(
        resolve_real("dev-libs", "interactivepkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_package_accept_restrict_narrows_acceptance_for_one_package() {
    // Same "-token narrows despite a permissive global default" mechanism as
    // package.properties above.
    assert_eq!(
        resolve_real("dev-libs", "restrictedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

/// A sibling package sharing a name prefix ("foo-bar" installed) must not be
/// misread as an installed version of "foo" (needs the digit-start guard in
/// `strip_version_prefix`).
#[test]
fn sibling_package_prefix_does_not_contaminate_vdb_scan() {
    assert_eq!(
        resolve("dev-libs", "foo"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
    assert_eq!(
        resolve("dev-libs", "foo-bar"),
        PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string()
        }
    );
}

#[test]
fn use_dep_enforcement_matches_a_declared_and_enabled_flag() {
    // IUSE="foo missingflag" with "foo" enabled globally.
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[foo]"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn use_dep_enforcement_rejects_a_negated_but_actually_enabled_flag() {
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[-foo]"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn use_dep_enforcement_rejects_a_declared_but_disabled_flag() {
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[missingflag]"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn use_dep_enforcement_matches_a_negated_and_actually_disabled_flag() {
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[-missingflag]"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn use_dep_enforcement_rejects_an_undeclared_flag_with_no_default() {
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[nonexistentflag]"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn use_dep_enforcement_plus_default_rescues_an_undeclared_flag() {
    assert_eq!(
        resolve_real("dev-libs", "useflagpkg[nonexistentflag(+)]"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

/// `adopt_current` replaces the live node's params in place without adding a
/// node.
#[test]
fn backtracker_adopt_current_replaces_the_live_node() {
    let mut bt = Backtracker::new(20, BacktrackParams::default());
    bt.get().expect("root node");
    bt.adopt_current(BacktrackParams {
        depth: 7,
        ..Default::default()
    });
    assert_eq!(bt.get_best_run().depth, 7);
}

/// `get_best_run` returns the deepest terminal node, earliest on ties.
#[test]
fn backtracker_get_best_run_prefers_the_deepest_terminal() {
    let cp_a = ("dev-libs".to_string(), "aa".to_string());
    let cp_b = ("dev-libs".to_string(), "bb".to_string());
    let mut bt = Backtracker::new(20, BacktrackParams::default());
    bt.get().expect("root node");
    let mut a = BacktrackParams {
        depth: 2,
        mask_steps: 1,
        ..Default::default()
    };
    a.autounmask_use_config
        .insert(cp_a.clone(), HashMap::from([("flag".to_string(), true)]));
    bt.feedback(BacktrackFeedback::Config {
        params: Box::new(a),
    });
    bt.get().expect("first mask node");
    let mut b = BacktrackParams {
        depth: 2,
        mask_steps: 2,
        ..Default::default()
    };
    b.autounmask_use_config
        .insert(cp_b.clone(), HashMap::from([("flag".to_string(), true)]));
    bt.feedback(BacktrackFeedback::Config {
        params: Box::new(b),
    });
    // Both children are terminal at depth 3; the tie keeps the first.
    let best = bt.get_best_run();
    assert_eq!(best.depth, 3);
    assert!(
        best.autounmask_use_config.contains_key(&cp_a)
            && !best.autounmask_use_config.contains_key(&cp_b),
        "depth ties keep the first deepest terminal"
    );
}

/// The slot-conflict arm masks the target in a new node, replacing -- not
/// duplicating -- a mask for the same negative.
#[test]
fn backtracker_feedback_slot_conflict_replaces_the_same_mask() {
    let cp = ("dev-libs".to_string(), "maskpkg".to_string());
    let neg = format!("!={}/{}-2.0", cp.0, cp.1);
    let mut base = BacktrackParams::default();
    base.runtime_pkg_mask.insert(
        cp.clone(),
        vec![MaskEntry {
            neg: neg.clone(),
            reason: MaskReason::MissingDependency {
                parent: cp.clone(),
                atom: "dev-libs/owner".to_string(),
            },
        }],
    );
    let choices = vec![MaskChoice {
        target: (cp.0.clone(), cp.1.clone(), "2.0".to_string()),
        parents: vec![(
            (
                "dev-libs".to_string(),
                "owner".to_string(),
                "1.0".to_string(),
            ),
            "<dev-libs/maskpkg-2.0".to_string(),
        )],
        similar: vec![],
    }];
    let mut bt = Backtracker::new(20, BacktrackParams::default());
    bt.get().expect("root node");
    bt.feedback(BacktrackFeedback::SlotConflict {
        base: Box::new(base),
        choices,
    });
    let node = bt.get().expect("mask node queued");
    let bucket = node
        .runtime_pkg_mask
        .get(&cp)
        .expect("mask bucket survives");
    assert_eq!(bucket.len(), 1, "same negative is replaced, not pushed");
    assert!(
        matches!(bucket[0].reason, MaskReason::SlotConflict { .. }),
        "the reason is refreshed to the slot conflict"
    );
    assert_eq!(node.mask_steps, 1);
    assert_eq!(node.depth, 1);
}

/// The missing-dep arm latches the parent and masks it once.
#[test]
fn backtracker_feedback_missing_dep_latches_without_duplicating() {
    let owner = ("dev-libs".to_string(), "ownpkg".to_string());
    let entry = MaskEntry {
        neg: "!=dev-libs/ownpkg-9.9".to_string(),
        reason: MaskReason::MissingDependency {
            parent: owner.clone(),
            atom: "dev-libs/gone".to_string(),
        },
    };
    let mut base = BacktrackParams::default();
    base.runtime_pkg_mask
        .insert(owner.clone(), vec![entry.clone()]);
    let mut bt = Backtracker::new(20, BacktrackParams::default());
    bt.get().expect("root node");
    bt.feedback(BacktrackFeedback::MissingDep {
        base: Box::new(base),
        owner: owner.clone(),
        entry: entry.clone(),
    });
    let node = bt.get().expect("mask node queued");
    let bucket = node
        .runtime_pkg_mask
        .get(&owner)
        .expect("mask bucket survives");
    assert_eq!(bucket.len(), 1, "an already-masked parent is latched once");
    assert!(node.missing_dep_masked.contains(&entry.neg));
    assert_eq!(node.mask_steps, 1);
    assert_eq!(node.depth, 1);
}
