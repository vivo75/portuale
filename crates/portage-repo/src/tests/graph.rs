use super::*;

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
