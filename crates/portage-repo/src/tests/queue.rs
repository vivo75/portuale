use super::*;

#[test]
fn exclude_leaves_an_already_installed_package_alone_even_with_update() {
    // dev-libs/upgradepkg is installed at 1.0, a newer 2.0 is
    // visible -- without --exclude, --update offers the upgrade
    // (see the `upgrade` test below); --exclude matching it
    // overrides --update entirely, same as real
    // _want_update_pkg's/_replace_installed_atom's own
    // excluded-checked-first precedent.
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
    // Real WildcardPackageSet accepts wildcard atoms, not just plain
    // ones -- ported here as the same two-tier matches_config_entry
    // package.mask/.unmask already uses.
    assert_eq!(
        resolve_update_excluded("dev-libs", "upgradepkg", &["dev-libs/*".to_string()]),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn exclude_does_not_affect_a_non_matching_package() {
    // A --exclude atom for an unrelated package has no effect at
    // all -- --update still offers the upgrade normally.
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

/// instance's own slot (real's `pkg.slot_atom` node identity), not
/// the cp. `fixtures/var/db/pkg` has `dev-libs/slotdedup-1.0` (slot
/// 1, no deps) and `slotdedup-2.0` (slot 2, RDEPEND
/// `dev-libs/slotdedupmarker`) installed;
/// `fixtures/repo/dev-libs/slotdedupconsumer-1.0` RDEPENDs
/// `dev-libs/slotdedup:1 dev-libs/slotdedup:2` (slot 1 first). With
/// the old cp-keyed dedup the slot-2 visit was dropped, so its deps
/// were never walked and `slotdedupmarker` never entered the graph --
/// the #74 class B1 shape (`llvm-core/clang:22` shadowing `:23`,
/// losing myst-parser).
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

/// #148: a *surviving* slot conflict keeps both same-slot
/// instances as merge rows (real's `l0-fx-20260922T192350Z` cell
/// `-D dev-libs/slotusegroup`: `[ebuild N] slotusetarget-1.0`
/// *and* `-2.0`, rc 1). `dev-libs/slotusegroup` RDEPENDs
/// `slotuseplain slotusex slotusey`; slotusey/slotusex pull
/// `>=slotusetarget-1.0[y]` / `[x]` (2.0 lacks `y` in IUSE and
/// has `x` default-off) while slotuseplain pulls
/// `>=slotusetarget-2.0`. Before the fix the walk's
/// `resolved_slots` conflict arm recorded the `SlotConflict`
/// and continued past `GraphEntry` creation, so the graph held
/// one `slotusetarget` entry (1.0) and the merge list one row.
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
    // deeppkg (installed) RDEPENDs deeppkg2 (installed) RDEPENDs
    // newpkg (New). Under `--deep` alone deeppkg2 resolves to
    // AlreadyInstalled (the CLI filters it from the merge list).
    // `--reinstall-atoms dev-libs/deeppkg2` rewrites just that one to
    // a bare Reinstall, exactly like a scoped --emptytree; deeppkg
    // stays AlreadyInstalled and newpkg stays New.
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
    // dev-libs/deeppkg is already installed and stays AlreadyInstalled
    // (no --update); its own RDEPEND chain (-> deeppkg2 -> newpkg) is
    // never read at all without --deep, matching real portage's own
    // default (deep=0, permanently "too deep" at every depth -- see
    // `Deep::NotRequested`'s own doc comment).
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
    // Bare --deep: unlimited depth, so both already-installed steps
    // (deeppkg -> deeppkg2) get walked, reaching deeppkg2's own
    // RDEPEND on newpkg (New).
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
    // --deep=1: deeppkg (depth 0) recurses since 0 < 1, discovering
    // deeppkg2 at depth 1 -- but deeppkg2 itself does NOT recurse
    // (1 < 1 is false), so newpkg is never reached.
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
