use super::*;

#[test]
fn exclude_leaves_an_already_installed_package_alone_even_with_update() {
    // --exclude matching it overrides --update entirely.
    // Portage: _want_update_pkg / _replace_installed_atom (excluded-checked-first).
    assert_eq!(
        resolve_update_excluded(
            "dev-libs",
            "upgradepkg",
            &["dev-libs/upgradepkg".to_string()]
        ),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn exclude_matches_via_a_wildcard_atom_too() {
    // Portage's WildcardPackageSet accepts wildcard atoms too.
    assert_eq!(
        resolve_update_excluded("dev-libs", "upgradepkg", &["dev-libs/*".to_string()]),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn exclude_does_not_affect_a_non_matching_package() {
    // An unrelated --exclude atom changes nothing.
    assert_eq!(
        resolve_update_excluded(
            "dev-libs",
            "upgradepkg",
            &["dev-libs/does-not-exist".to_string()]
        ),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string()
        }
    );
}

/// Installed instances dedup by slot, not by cp, so every slot's
/// own deps are walked.
#[test]
fn other_outcomes_dedups_installed_instances_by_slot_not_cp() {
    let entries = graph_deep("dev-libs/slotdedupconsumer", Deep::Unlimited);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"dev-libs/slotdedupconsumer"), "{names:?}");
    assert!(
        names.contains(&"dev-libs/slotdedupmarker"),
        "slot 2's own deps must be walked: {names:?}"
    );
}

/// A surviving slot conflict keeps both same-slot instances as
/// merge rows.
#[test]
fn surviving_slot_conflict_graphs_both_same_slot_instances() {
    let entries = graph_deep("dev-libs/slotusegroup", Deep::Unlimited);
    let mut versions: Vec<&str> = entries
        .iter()
        .filter(|(name, _)| name == "dev-libs/slotusetarget")
        .map(|(_, outcome)| match outcome {
            PretendOutcome::New { version } => version.as_str(),
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                to.as_str()
            }
            PretendOutcome::Reinstall { version, .. } => version.as_str(),
            _ => panic!("slotusetarget entry is not merge-bound: {entries:?}"),
        })
        .collect();
    versions.sort_unstable();
    assert_eq!(
        versions,
        vec!["1.0", "2.0"],
        "both conflicting instances must be graphed: {entries:?}"
    );
}

#[test]
fn reinstall_atoms_forces_an_otherwise_already_installed_deep_dep_to_reinstall() {
    // `--reinstall-atoms dev-libs/deeppkg2` rewrites just that one to
    // a bare Reinstall, exactly like a scoped --emptytree.
    let bare = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let plain = graph_deep("dev-libs/deeppkg", Deep::Unlimited);
    assert_eq!(
        plain
            .iter()
            .find(|(n, _)| n == "dev-libs/deeppkg2")
            .map(|(_, o)| o.clone()),
        Some(PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        })
    );

    let forced = graph_deep_reinstall_atoms("dev-libs/deeppkg", &["dev-libs/deeppkg2"]);
    assert_eq!(
        forced,
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            ("dev-libs/deeppkg2".to_string(), bare),
            (
                "dev-libs/deeppkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
        ]
    );
}

#[test]
fn deep_not_requested_never_walks_an_already_installed_packages_own_dependencies() {
    // Without --deep an installed package's own RDEPEND chain is never
    // read (Portage's deep=0 default; see `Deep::NotRequested`).
    assert_eq!(
        graph_deep("dev-libs/deeppkg", Deep::NotRequested),
        vec![(
            "dev-libs/deeppkg".to_string(),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        )]
    );
}

#[test]
fn deep_unlimited_walks_the_whole_already_installed_chain() {
    // Bare --deep walks the whole installed chain down to newpkg.
    assert_eq!(
        graph_deep("dev-libs/deeppkg", Deep::Unlimited),
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/deeppkg2".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/deeppkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn deep_bounded_one_walks_exactly_one_level_of_already_installed_packages() {
    // --deep=1 walks exactly one level: deeppkg2 is found but never
    // recursed, so newpkg is never reached.
    assert_eq!(
        graph_deep("dev-libs/deeppkg", Deep::Bounded(1)),
        vec![
            (
                "dev-libs/deeppkg2".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/deeppkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}
