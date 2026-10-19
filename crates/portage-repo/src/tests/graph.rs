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
fn emptytree_downgrades_a_keyword_masked_installed_dependency_to_the_visible_stable() {
    // Under `--emptytree` Portage drops every installed candidate wholesale,
    // so the best visible stable wins -> downgrade 2.0 -> 1.0.
    let entries = graph_empty("dev-libs/needskeywordmasked");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/keywordmaskedpkg".to_string(),
                PretendOutcome::Downgrade {
                    from: "2.0".to_string(),
                    to: "1.0".to_string(),
                }
            ),
            (
                "dev-libs/needskeywordmasked".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
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
fn changed_use_reinstall_still_recurses_into_its_own_dependencies() {
    // Same proof as newuse_reinstall_still_recurses_into_its_own_dependencies
    // below, but via --changed-use instead of --newuse.
    assert_eq!(
        graph_real_changed_use("dev-libs/reinstallpkg"),
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/reinstallpkg".to_string(),
                PretendOutcome::Reinstall {
                    version: "1.0".to_string(),
                    changed_flags: vec!["foo".to_string()],
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: false,
                }
            )
        ]
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
fn fixture_same_version_tie_across_repos_is_broken_toward_higher_priority() {
    // Resolving it must pull in newpkg, proving the overlay's copy is read.
    let full_names: Vec<String> = graph_entries_real("dev-libs/overlaytiepkg")
        .into_iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(
        full_names,
        vec!["dev-libs/newpkg", "dev-libs/overlaytiepkg"]
    );
}

#[test]
fn fixture_solvable_slot_conflict_is_reconciled_by_backtracking() {
    // The walk resolves last-declared-first (Portage's stack), so the old
    // consumer's `<2.0` graphs 1.0 first and the bare dep reuses it: no
    // conflict ever forms, at any backtrack budget.
    let result = graph_result_real("dev-libs/slotconflictparent");
    let full_names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(
        full_names,
        vec![
            "dev-libs/slotconflicttarget",
            "dev-libs/slotconflictnewconsumer",
            "dev-libs/slotconflictoldconsumer",
            "dev-libs/slotconflictparent"
        ]
    );
    assert_eq!(result.slot_conflicts, vec![]);
    let target = result
        .entries
        .iter()
        .find(|e| e.package == "slotconflicttarget")
        .expect("slotconflicttarget entry");
    assert_eq!(
        target.outcome,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn backtrack_zero_feeds_satisfiable_reverse_dep_pins_in_pass() {
    // `--backtrack=0` must not skip the reverse-dependency feed loop:
    // Portage calls `_complete_graph()` with no backtracking gate, so a
    // satisfiable installed-consumer pin enforces in-pass.
    //
    // The fixture world file does not carry the consumer, so the seed is set
    // directly.
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
    config.complete_seed_atoms = vec!["dev-libs/r25consumer".to_string()];
    let result = ResolveRequest {
        newuse: true,
        update: true,
        deep: Deep::Unlimited,
        backtrack_max: 0,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/r25target".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving r25target failed: {e}"));
    // The lib upgrade is withheld: no merge of any r25lib version.
    assert!(
        result.entries.iter().all(|e| {
            e.package != "r25lib" || matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })
        }),
        "r25lib must settle installed, not merge: {:?}",
        result
            .entries
            .iter()
            .filter(|e| e.package == "r25lib")
            .collect::<Vec<_>>()
    );
    // Portage's silence: no slot-operator withhold rows at bt0 (the
    // minimizer/probe path writes no missed-update state).
    assert_eq!(result.skipped_updates, vec![]);
    assert_eq!(result.backtrack_restarts, 0);
    assert_eq!(result.slot_conflicts, vec![]);
    // The rest of Portage's merge set is intact.
    let versions: HashMap<(&str, &str), String> = result
        .entries
        .iter()
        .filter_map(|e| match &e.outcome {
            PretendOutcome::Upgrade { to, .. } => {
                Some(((e.category.as_str(), e.package.as_str()), format!("U {to}")))
            }
            PretendOutcome::New { version } => Some((
                (e.category.as_str(), e.package.as_str()),
                format!("N {version}"),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        versions.get(&("dev-libs", "r25up")).map(String::as_str),
        Some("U 2.0"),
        "r25up still upgrades"
    );
    assert_eq!(
        versions.get(&("dev-libs", "r25target")).map(String::as_str),
        Some("N 1.0"),
        "r25target still merges"
    );
}

#[test]
fn two_atom_old_new_withholds_with_a_skip_notice_at_any_budget() {
    // At `--backtrack=0` and the default budget alike, no conflict survives
    // and exactly one skip row names oldconsumer's pin (staged oracle).
    for bt in [0, 10] {
        let result = graph_result_real_atoms(
            &[
                "dev-libs/slotconflictoldconsumer".to_string(),
                "dev-libs/slotconflictnewconsumer".to_string(),
            ],
            bt,
        );
        assert!(
            result.slot_conflicts.is_empty(),
            "bt={bt}: expected no surviving conflict, got {:?}",
            result.slot_conflicts
        );
        let target = result
            .entries
            .iter()
            .find(|e| e.package == "slotconflicttarget")
            .expect("slotconflicttarget entry");
        assert_eq!(
            target.outcome,
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
        assert_eq!(result.skipped_updates.len(), 1, "bt={bt}");
        let s = &result.skipped_updates[0];
        assert_eq!(s.skipped_version, "2.0");
        assert_eq!(s.atom, "<dev-libs/slotconflicttarget-2.0");
        assert!(s.consumer_cpv.contains("slotconflictoldconsumer-1.0"));
    }
}

#[test]
fn fixture_unsolvable_slot_conflict_is_resolved_by_masking_a_puller_version() {
    // No bttarget version satisfies both pullers, so the retry masks 2.0 and
    // falls back to btconsumer-1.0 + bttarget-1.0 via Portage's feedback paths.
    let result = graph_result_real("dev-libs/btparent");
    assert_eq!(result.slot_conflicts, vec![]);
    let ver = |pkg: &str| {
        result
            .entries
            .iter()
            .find(|e| e.package == pkg)
            .map(|e| match &e.outcome {
                PretendOutcome::New { version } => version.clone(),
                other => panic!("{pkg}: unexpected {other:?}"),
            })
            .unwrap_or_else(|| panic!("no entry for {pkg}"))
    };
    assert_eq!(ver("btconsumer"), "1.0");
    assert_eq!(ver("bttarget"), "1.0");
}

#[test]
fn fixture_masking_trial_is_reverted_when_it_makes_a_dependency_unsatisfiable() {
    // Masking only turns the >=2.0 dep into an NVC, so the trial reverts and
    // the original conflict reports unchanged.
    let result = graph_result_real("dev-libs/slotconflictunsolvable");
    assert_one_conflict(AssertOneConflictArgs {
        result: &result,
        category: "dev-libs",
        package: "slotconflicttarget",
        slot: "0",
        resolved_version: "1.0",
        conflicting_atom: ">=dev-libs/slotconflicttarget-2.0",
    });
    assert!(
        !result
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate)),
        "the reverted graph must not carry the trial's NoVisibleCandidate"
    );
}

#[test]
fn fixture_slot_conflict_carries_per_instance_pulled_in_by_data() {
    // The SlotConflict names every conflicting version and, per version, the
    // (parent cpv, atom) that pulled it (Portage's per-slot_atom).
    let result = graph_result_real("dev-libs/slotconflictunsolvable");
    assert_eq!(result.slot_conflicts.len(), 1);
    let c = &result.slot_conflicts[0];
    assert_eq!(
        c.instances
            .iter()
            .map(|i| i.version.as_str())
            .collect::<Vec<_>>(),
        vec!["1.0", "2.0"]
    );
    assert_eq!(
        c.instances[0]
            .parents
            .iter()
            .map(|p| (p.parent_cpv.as_str(), p.atom.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "dev-libs/slotconflictoldpin-1.0:0/0::testrepo",
            "<dev-libs/slotconflicttarget-2.0"
        )]
    );
    assert_eq!(
        c.instances[1]
            .parents
            .iter()
            .map(|p| (p.parent_cpv.as_str(), p.atom.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "dev-libs/slotconflictnewpin-1.0:0/0::testrepo",
            ">=dev-libs/slotconflicttarget-2.0"
        )]
    );
    // No IUSE anywhere: every instance/parent renders `USE="" ELIBC="glibc"`
    // (Portage's `pkg_use_display` bare group plus the implicit USE_EXPAND).
    assert!(c.instances.iter().all(|i| i.use_display
        == vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ]));
}

#[test]
fn fixture_slot_conflict_carries_pkg_use_display() {
    // scusetarget IUSE="+scuon scuoff": both instances render
    // `USE="scuon -scuoff" ELIBC="glibc"` (Portage's `pkg_use_display`).
    let result = graph_result_real("dev-libs/scuseparent");
    assert_eq!(result.slot_conflicts.len(), 1);
    let c = &result.slot_conflicts[0];
    for inst in &c.instances {
        assert_eq!(
            inst.use_display,
            vec![
                ("USE".to_string(), "scuon -scuoff".to_string()),
                ("ELIBC".to_string(), "glibc".to_string()),
            ]
        );
    }
    // Instance order follows the walk: the oldpin's `<2.0` resolves first, so
    // 1.0 sorts first (Portage's notice order).
    assert_eq!(
        c.instances[0].parents[0].use_display,
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ]
    );
    assert_eq!(
        c.instances[1].parents[0].use_display,
        vec![
            ("USE".to_string(), "scupin".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ]
    );
}

#[test]
fn fixture_unsolvable_slot_conflict_survives_backtracking_and_is_reported() {
    // No single version satisfies both pullers, so no retry is attempted and
    // the SlotConflict reports unchanged.
    let result = graph_result_real("dev-libs/slotconflictunsolvable");
    assert_one_conflict(AssertOneConflictArgs {
        result: &result,
        category: "dev-libs",
        package: "slotconflicttarget",
        slot: "0",
        resolved_version: "1.0",
        conflicting_atom: ">=dev-libs/slotconflicttarget-2.0",
    });
}

#[test]
fn fixture_different_slots_of_the_same_package_coexist_without_conflict() {
    // Different slots of the same package coexist (like dev-lang/python:3.11
    // and :3.12): both appear as independent entries.
    let result = graph_result_real("dev-libs/multislotparent");
    let full_names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(
        full_names,
        vec![
            "dev-libs/multislotpkg",
            "dev-libs/multislotpkg",
            "dev-libs/multislotparent",
        ]
    );
    let mut slots: Vec<Option<String>> = result
        .entries
        .iter()
        .filter(|e| e.package == "multislotpkg")
        .map(|e| e.slot.clone())
        .collect();
    slots.sort();
    assert_eq!(slots, vec![Some("0".to_string()), Some("1".to_string())]);
    assert!(result.slot_conflicts.is_empty());
}

#[test]
fn fixture_new_slot_install_is_new_not_an_upgrade() {
    // Requesting `:1` matches only -2.0, whose own slot has nothing installed:
    // a `New` with `new_slot=True`, NOT an `Upgrade` from 1.0.
    let entries = graph_entries_real("dev-libs/newslotpkg:1");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].outcome,
        PretendOutcome::New {
            version: "2.0".to_string()
        }
    );
    assert!(entries[0].new_slot);
    assert_eq!(entries[0].slot.as_deref(), Some("1"));
}

#[test]
fn fixture_in_slot_request_is_not_flagged_new_slot() {
    // Requesting the installed slot stays an in-slot outcome, never `new_slot`.
    let entries = graph_entries_real("dev-libs/newslotpkg:0");
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].new_slot);
    assert_eq!(
        entries[0].outcome,
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_interactive_bracket_column_tracks_evaluated_properties() {
    // Unconditional PROPERTIES=interactive on a merge-bound pkg -> the
    // interactive bracket (Portage's output.py attr_display).
    let entries = graph_entries_real("dev-libs/interactivemergepkg");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].interactive);
    assert!(matches!(entries[0].outcome, PretendOutcome::New { .. }));

    // `interactive` behind a `gtk?` conditional with `gtk` disabled is gated
    // out by USE-conditional evaluation.
    let entries = graph_entries_real("dev-libs/interactivecondpkg");
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].interactive);
    // (The merge-bound `Reinstall` case is covered end to end by the contract
    // suite; this helper is selective, so a bare installed atom resolves to
    // `AlreadyInstalled` here, which carries no bracket.)
}

#[test]
fn fixture_fetch_restrict_column_tracks_distdir_state() {
    // Both fixtures are `RESTRICT="fetch"`. The distdir holds `frs-1.0.tar.gz`
    // at its Manifest size but not `frm-1.0.tar.gz` (Portage's fetch_restrict).
    let entries = graph_entries_real("dev-libs/fetchrestrictsatisfiedpkg");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].fetch_restrict);
    assert!(entries[0].fetch_restrict_satisfied);

    let entries = graph_entries_real("dev-libs/fetchrestrictmissingpkg");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].fetch_restrict);
    assert!(!entries[0].fetch_restrict_satisfied);

    // A package with no `fetch` in RESTRICT never sets the flag.
    let entries = graph_entries_real("dev-libs/newpkg");
    assert!(!entries[0].fetch_restrict);
    assert!(!entries[0].fetch_restrict_satisfied);
}

#[test]
fn fixture_virtual_resolves_through_ordinary_category_and_any_of_machinery() {
    // No PROVIDE mechanism, no dedicated virtuals code: the installed
    // alternative wins over the first-listed one (Portage's dep_zapdeps
    // preference, like `virtual/wine` -> installed `wine-staging`).
    let entries = graph_entries_real("virtual/texteditor");
    let full_names: Vec<String> = entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(full_names, vec!["dev-libs/samepkg", "virtual/texteditor"]);
    assert_eq!(
        entries
            .iter()
            .find(|e| e.package == "samepkg")
            .expect("samepkg entry")
            .outcome,
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
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
fn fixture_provenance_records_the_matching_mask_and_unmask_entries() {
    let entries = graph_entries_real("dev-libs/maskedandunmaskedpkg");
    assert_eq!(
        entries[0].provenance.mask_entry.as_deref(),
        Some("dev-libs/maskedandunmaskedpkg")
    );
    assert_eq!(
        entries[0].provenance.unmask_entry.as_deref(),
        Some("dev-libs/maskedandunmaskedpkg")
    );
}

#[test]
fn fixture_provenance_records_no_mask_entry_when_never_masked() {
    let entries = graph_entries_real("dev-libs/wildcardkeywordpkg");
    assert_eq!(entries[0].provenance.mask_entry, None);
    assert_eq!(entries[0].provenance.unmask_entry, None);
}

#[test]
fn fixture_provenance_records_the_keyword_entry_that_was_actually_needed() {
    // dev-libs/wildcardkeywordpkg is ~amd64-only, accepted only via
    // the "*/wildcardkeywordpkg ~amd64" package.accept_keywords entry
    // (see fixtures/etc/portage/package.accept_keywords) -- the
    // global ACCEPT_KEYWORDS alone doesn't accept it.
    let entries = graph_entries_real("dev-libs/wildcardkeywordpkg");
    assert_eq!(
        entries[0].provenance.keyword_entry.as_deref(),
        Some("*/wildcardkeywordpkg")
    );
}

#[test]
fn fixture_provenance_records_no_keyword_entry_when_global_keywords_suffice() {
    // dev-libs/newpkg is a plain stable-amd64 package needing no
    // package.accept_keywords help at all.
    let entries = graph_entries_real("dev-libs/newpkg");
    assert_eq!(entries[0].provenance.keyword_entry, None);
}

#[test]
fn keyword_mask_marker_classifies_the_bracket_column() {
    // Plain stable-amd64: no marker (globally keyword-visible).
    assert_eq!(graph_entries_real("dev-libs/newpkg")[0].keyword_mask, None);
    // ~amd64, visible only via a package.accept_keywords entry -- a
    // testing keyword for our own arch -> '~'.
    assert_eq!(
        graph_entries_real("dev-libs/wildcardkeywordpkg")[0].keyword_mask,
        Some('~')
    );
    assert_eq!(
        graph_entries_real("dev-libs/bareacceptkeywordspkg")[0].keyword_mask,
        Some('~')
    );
    // ~arm64, visible only via "~*" -- a different arch's testing
    // keyword -> '*' (Portage's get_keyword_mask "missing").
    assert_eq!(
        graph_entries_real("dev-libs/tildestarkeywordpkg")[0].keyword_mask,
        Some('*')
    );
    // package.mask'd then package.unmask'd -> '#' (Portage's isHardMasked
    // ignores package.unmask), and it wins over any keyword state.
    assert_eq!(
        graph_entries_real("dev-libs/maskedandunmaskedpkg")[0].keyword_mask,
        Some('#')
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

/// `--onlydeps-with-rdeps` / `--onlydeps-with-ideps` blank an onlydeps root's
/// dep classes like Portage (the root itself is always resolved; only the
/// display layer suppresses it).
#[test]
fn onlydeps_with_rdeps_and_ideps_blank_the_root_dep_classes() {
    let names = |entries: Vec<(String, PretendOutcome)>| {
        let mut names: Vec<String> = entries.into_iter().map(|(n, _)| n).collect();
        names.sort();
        names
    };
    // Default (rdeps on): every class walked.
    assert_eq!(
        names(graph_onlydeps("dev-libs/odw0a", true, false)),
        vec![
            "dev-libs/odw0a",
            "dev-libs/odw0b",
            "dev-libs/odw0c",
            "dev-libs/odw0d",
            "dev-libs/odw0e",
            "dev-libs/odw0f",
        ],
        "default"
    );
    // with-rdeps=n: build-time keys (DEPEND B, BDEPEND F) only.
    assert_eq!(
        names(graph_onlydeps("dev-libs/odw0a", false, false)),
        vec!["dev-libs/odw0a", "dev-libs/odw0b", "dev-libs/odw0f"],
        "with-rdeps=n"
    );
    // with-rdeps=n + with-ideps=y: IDEPEND E restored.
    assert_eq!(
        names(graph_onlydeps("dev-libs/odw0a", false, true)),
        vec![
            "dev-libs/odw0a",
            "dev-libs/odw0b",
            "dev-libs/odw0e",
            "dev-libs/odw0f",
        ],
        "with-rdeps=n with-ideps=y"
    );
}

/// An EAPI-7 root's `IDEPEND` never reaches the walk (Portage's `bin/ebuild.sh`
/// unsets it in the depend phase).
#[test]
fn onlydeps_eapi7_root_ignores_idepend() {
    let mut names: Vec<String> = graph_onlydeps("dev-libs/odw0g", true, false)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "dev-libs/odw0b",
            "dev-libs/odw0c",
            "dev-libs/odw0d",
            "dev-libs/odw0g",
        ]
    );
}

/// An installed `||` alternative's `[use]` deps are matched against its
/// recorded vdb USE, not the tree candidate's profile-derived USE (Portage's
/// composite-db vardb cover plus the built-package branch reading `USE`).
#[test]
fn disjunction_preference_matches_an_installed_alternatives_vdb_use() {
    let entries = graph("dev-libs/altconsumer");
    assert!(
        entries
            .iter()
            .any(|(name, _)| name == "dev-libs/altconsumer"),
        "{entries:?}"
    );
    let altprov_merges: Vec<&(String, PretendOutcome)> = entries
        .iter()
        .filter(|(name, outcome)| {
            name == "dev-libs/altprov"
                && matches!(
                    outcome,
                    PretendOutcome::New { .. }
                        | PretendOutcome::Upgrade { .. }
                        | PretendOutcome::Downgrade { .. }
                        | PretendOutcome::Reinstall { .. }
                )
        })
        .collect();
    assert!(
        altprov_merges.is_empty(),
        "the installed altprov:1.1[flip] must win the || group: {entries:?}"
    );
}

#[test]
fn exclude_threads_through_dependency_recursion_not_just_top_level() {
    // --exclude must still leave it alone despite --update, proving the flag
    // threads uniformly through the whole BFS, not just top-level atoms.
    let root = fixtures_root();
    let entries: Vec<(String, PretendOutcome)> = ResolveRequest {
        update: true,
        excluded: vec!["dev-libs/upgradepkg".to_string()],
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/withdeps".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect();
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/upgradepkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/withdeps".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn reinstall_atoms_accepts_a_wildcard_atom() {
    let forced = graph_deep_reinstall_atoms("dev-libs/deeppkg", &["dev-libs/*"]);
    assert!(forced.iter().any(|(n, _)| n == "dev-libs/deeppkg2"));
}

#[test]
fn rebuild_if_unbuilt_does_not_rebuild_a_consumer_missing_from_the_graph() {
    // Portage `_rebuild_config.add` records a rebuild edge only for a parent
    // already in the graph; `rebuildconsumer` is reached by nothing here, so
    // even with the trigger merging it gets no `R` row. Oracle: Portage
    // 3.0.82.2 `s0-b-rebuild-unbuilt` (`U rebuildtrigger-2.0` alone).
    let g = graph_rebuild_if(GraphRebuildIfArgs {
        atom_str: "dev-libs/rebuildtrigger",
        unbuilt: true,
        new_rev: false,
        new_ver: false,
        rebuild_exclude: &[],
        rebuild_ignore: &[],
    });
    assert!(
        !g.iter().any(|(n, _)| n == "dev-libs/rebuildconsumer"),
        "{g:?}"
    );
    assert!(
        g.iter()
            .any(|(n, o)| n == "dev-libs/rebuildtrigger"
                && matches!(o, PretendOutcome::Upgrade { .. })),
        "{g:?}"
    );
}

#[test]
fn rebuild_if_new_ver_ignores_a_same_version_dep() {
    // rebuildnochange's best tree version equals the installed one.
    // Even forced to reinstall (--reinstall-atoms), that is not a
    // "new version"/"new revision" -> its consumer is NOT rebuilt
    // under --rebuild-if-new-ver / -new-rev.
    let root = fixtures_root();
    for (nr, nv) in [(true, false), (false, true)] {
        let g = ResolveRequest {
            reinstall_atoms: vec!["dev-libs/rebuildnochange".to_string()],
            rebuild_if_new_rev: nr,
            rebuild_if_new_ver: nv,
            ..ResolveRequest::new(
                &root,
                &root,
                &["dev-libs/rebuildnochange".to_string()],
                &test_config(),
                &fixtures_root().join("distfiles"),
            )
        }
        .resolve()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect::<Vec<_>>();
        assert!(
            g.iter().any(|(n, _)| n == "dev-libs/rebuildnochange"),
            "the dep itself should be forced into the graph: {g:?}"
        );
        assert!(
            !g.iter()
                .any(|(n, _)| n == "dev-libs/rebuildnochangeconsumer"),
            "no rebuild for a same-version dep: {g:?}"
        );
    }
}

#[test]
fn rebuild_exclude_and_ignore_suppress_the_rebuild() {
    // --rebuild-exclude matches the parent; --rebuild-ignore matches
    // the merged dep. Either one keeps rebuildconsumer out.
    let ex = graph_rebuild_if(GraphRebuildIfArgs {
        atom_str: "dev-libs/rebuildtrigger",
        unbuilt: true,
        new_rev: false,
        new_ver: false,
        rebuild_exclude: &["dev-libs/rebuildconsumer"],
        rebuild_ignore: &[],
    });
    assert!(!ex.iter().any(|(n, _)| n == "dev-libs/rebuildconsumer"));
    let ig = graph_rebuild_if(GraphRebuildIfArgs {
        atom_str: "dev-libs/rebuildtrigger",
        unbuilt: true,
        new_rev: false,
        new_ver: false,
        rebuild_exclude: &[],
        rebuild_ignore: &["dev-libs/rebuildtrigger"],
    });
    assert!(!ig.iter().any(|(n, _)| n == "dev-libs/rebuildconsumer"));
}

#[test]
fn dynamic_deps_picks_ebuild_vs_vdb_deps_for_an_already_installed_deep_walk() {
    // Installed with a vdb RDEPEND but a current-ebuild RDEPEND naming someone
    // else: dynamic walks the ebuild, =n walks the vdb snapshot.
    let root = fixtures_root();
    let call = |dynamic_deps: bool| -> Vec<String> {
        ResolveRequest {
            deep: Deep::Unlimited,
            dynamic_deps,
            ..ResolveRequest::new(
                &root,
                &root,
                &["dev-libs/changeddepspkg".to_string()],
                &test_config(),
                &fixtures_root().join("distfiles"),
            )
        }
        .resolve()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect()
    };
    // dynamic (default): the current ebuild's RDEPEND -> newpkg (New).
    assert!(call(true).iter().any(|n| n == "dev-libs/newpkg"));
    // =n: the vdb RDEPEND -> samepkg (installed) -> newpkg not pulled.
    assert!(!call(false).iter().any(|n| n == "dev-libs/newpkg"));
}

#[test]
fn default_dynamic_deps_walks_the_live_ebuild_alone() {
    // No vdb `EAPI` file, so the installed EAPI reads as `"0"` and the overlay
    // never applies here: the default walks the live ebuild alone, `=n`
    // walks the vdb snapshot alone.
    let root = fixtures_root();
    let call = |dynamic_deps: bool, ignore_built: bool| -> Vec<String> {
        ResolveRequest {
            deep: Deep::Unlimited,
            ignore_built_slot_operator_deps: ignore_built,
            dynamic_deps,
            ..ResolveRequest::new(
                &root,
                &root,
                &["dev-libs/builtbindpkg".to_string()],
                &test_config(),
                &fixtures_root().join("distfiles"),
            )
        }
        .resolve()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect()
    };
    let default = call(true, false);
    assert!(
        default.iter().any(|n| n == "dev-libs/newpkg"),
        "live ebuild dep: {default:?}"
    );
    assert!(
        !default.iter().any(|n| n == "dev-libs/builtbindtarget"),
        "no append: the vdb-only binding is not walked: {default:?}"
    );
    let static_ = call(false, false);
    assert!(
        !static_.iter().any(|n| n == "dev-libs/newpkg"),
        "=n reads the vdb snapshot, not the ebuild: {static_:?}"
    );
    assert!(
        static_.iter().any(|n| n == "dev-libs/builtbindtarget"),
        "=n still walks the recorded built binding: {static_:?}"
    );
    let ignored = call(true, true);
    assert!(
        ignored.iter().any(|n| n == "dev-libs/newpkg"),
        "append suppressed, live dep intact: {ignored:?}"
    );
    assert!(
        !ignored.iter().any(|n| n == "dev-libs/builtbindtarget"),
        "ignore flag changes nothing here: {ignored:?}"
    );
}

#[test]
fn deep_walk_evaluates_flag_deps_against_the_installed_vdb_use_not_effective() {
    // The conditional is evaluated against the recorded vdb USE (Portage's
    // `_pkg_use_enabled` for a built package), so the target is NOT pulled.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let names: Vec<String> = ResolveRequest {
        deep: Deep::Unlimited,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/deepvdbuseconsumer".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap()
    .entries
    .into_iter()
    .map(|e| format!("{}/{}", e.category, e.package))
    .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/deepvdbuseconsumer"),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "dev-libs/deepvdbusetarget"),
        "vdb USE lacks `wantdep`, so the gated dep must not be walked: {names:?}"
    );
}

#[test]
fn deep_walk_evaluates_conditional_use_deps_of_installed_parents() {
    // Each installed parent carries `~child-1.0[flip=]`; the token is evaluated
    // against the parent's recorded vdb USE (Portage's `_pkg_use_enabled`)
    // before candidate selection.
    //
    // Arm A (parent built -flip, child installed +flip and profile-pinned
    // +flip): `[-flip]` is unsatisfiable, so the child surfaces unsatisfied
    // for the abort path to report.
    let arm_a = resolve_deep_consumer("dev-libs/deepusedepconsumer", false);
    assert!(
        arm_a
            .iter()
            .any(|(n, _, _)| n == "dev-libs/deepusedepconsumer"),
        "{arm_a:?}"
    );
    assert!(
        arm_a
            .iter()
            .any(|(n, _, _)| n == "dev-libs/deepusedepchild"),
        "evaluated [-flip] is not satisfied by the installed +flip child, \
         so the child must surface (unsatisfied), not vanish: {arm_a:?}"
    );
    assert!(
        !arm_a
            .iter()
            .any(|(n, o, _)| n == "dev-libs/deepusedepchild"
                && (o == "AlreadyInstalled" || o == "New")),
        "the +flip instance satisfies neither [-flip] nor a fresh build \
         under the profile pin: {arm_a:?}"
    );
    //
    // Arm B (same parent shape, child not profile-pinned): `[-flip]` reaches
    // candidate selection -- the child rebuilds with flip off.
    let arm_b = resolve_deep_consumer("dev-libs/deepusedepbconsumer", false);
    assert!(
        arm_b
            .iter()
            .any(|(n, o, _)| n == "dev-libs/deepusedepbchild" && o == "Reinstall"),
        "evaluated [-flip] must rebuild the installed +flip child with \
         flip off: {arm_b:?}"
    );
    //
    // Arm C (control -- parent built +flip): the atom evaluates to `[flip]`,
    // which the installed child satisfies, so nothing merges.
    let arm_c = resolve_deep_consumer("dev-libs/deepusedepokconsumer", false);
    assert!(
        arm_c
            .iter()
            .any(|(n, o, _)| n == "dev-libs/deepusedepokconsumer" && o == "New"),
        "{arm_c:?}"
    );
    assert!(
        !arm_c
            .iter()
            .any(|(_, o, _)| o != "New" && o != "AlreadyInstalled"),
        "control: a satisfied evaluated dep merges nothing: {arm_c:?}"
    );
}

#[test]
fn root_deps_walk_evaluates_conditional_use_deps_before_the_satisfied_check() {
    // With `--root-deps` the producers must evaluate the atom to `[-flip]`
    // *and* enforce it against the running-root instance (Portage rebuilds
    // it with flip off).
    let with_root_deps = resolve_deep_consumer("dev-libs/deeprootdepconsumer", true);
    assert!(
        with_root_deps
            .iter()
            .any(|(n, o, running)| n == "dev-libs/deeprootdepchild"
                && o == "Reinstall"
                && *running),
        "evaluated [-flip] is not satisfied by the running root's +flip \
         child, so it resolves as a running-root rebuild: {with_root_deps:?}"
    );
    //
    // Control without `--root-deps`: the producers are a strict no-op and the
    // ordinary walk rebuilds the child against the target root instead.
    let without = resolve_deep_consumer("dev-libs/deeprootdepconsumer", false);
    assert!(
        without
            .iter()
            .any(|(n, o, running)| n == "dev-libs/deeprootdepchild"
                && o == "Reinstall"
                && !*running),
        "control: without --root-deps the same rebuild targets ROOT: {without:?}"
    );
}

/// A `--root-deps` running-root build entry's `use_flags_display` carries the
/// rebuild's flipped flag for the renderer's `-flip*` column.
#[test]
fn root_deps_build_entry_displays_the_rebuild_use_column() {
    let root = fixtures_root();
    let config = test_config();
    let entries = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/deeprootdepconsumer".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries;

    let child = entries
        .iter()
        .find(|e| e.package == "deeprootdepchild")
        .expect("the running-root rebuild entry is present");
    assert!(
        child.targets_running_root,
        "the rebuild is the running-root build entry: {child:?}"
    );
    // Installed +flip (vdb USE="flip"), rebuilt with flip off: the pair the
    // renderer's `-flip*` comes from.
    assert_eq!(
        child.use_flags_display,
        vec![("flip".to_string(), false)],
        "the rebuild's flipped flag must reach the display (#141)"
    );
    assert_eq!(child.slot.as_deref(), Some("0"));
}

#[test]
fn emptytree_reinstalls_the_whole_deep_dependency_tree() {
    // --emptytree forces `deep` on AND turns each installed dep into a
    // Reinstall, so the whole chain shows.
    let entries = graph_empty("dev-libs/deeppkg");
    let bare = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: vec![],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            ("dev-libs/deeppkg2".to_string(), bare.clone()),
            ("dev-libs/deeppkg".to_string(), bare)
        ]
    );
}

#[test]
fn with_bdeps_default_true_walks_depend_and_bdepend_of_an_already_installed_package() {
    // with_bdeps defaults to true (Portage's --with-bdeps=auto/y): all three
    // dep classes walk under --deep.
    // Merge order follows Portage's discovery order (RDEPEND, IDEPEND,
    // PDEPEND, DEPEND, BDEPEND).
    assert_eq!(
        graph_deep_with_bdeps("dev-libs/withbdepspkg", true),
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/builddeponlypkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/hostdeponlypkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/withbdepspkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn with_bdeps_false_skips_depend_and_bdepend_but_not_rdepend() {
    // --with-bdeps=n only ever drops DEPEND/BDEPEND for an already-built
    // package; RDEPEND still shows up.
    assert_eq!(
        graph_deep_with_bdeps("dev-libs/withbdepspkg", false),
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/withbdepspkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

/// An unsatisfied BDEPEND with a visible ebuild gets its own running-root
/// `New` entry under `--root-deps` (Portage's "recursively pull in and build
/// a new package against the running root").
#[test]
fn root_deps_adds_a_real_build_entry_for_an_unsatisfied_bdepend_with_a_visible_ebuild() {
    let root = fixtures_root();
    let entries = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/rootdepsbuildpkg".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries;

    let names: Vec<(String, bool)> = entries
        .iter()
        .map(|e| {
            (
                format!("{}/{}", e.category, e.package),
                e.targets_running_root,
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("dev-libs/rootdepsbuildtool".to_string(), true),
            ("dev-libs/rootdepsbuildpkg".to_string(), false)
        ]
    );

    let build_entry = entries
        .iter()
        .find(|e| e.package == "rootdepsbuildtool")
        .unwrap();
    assert_eq!(
        build_entry.outcome,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
    assert_eq!(
        build_entry.required_by,
        vec![("dev-libs".to_string(), "rootdepsbuildpkg".to_string())]
    );
}

/// Recursive successor to the test above: all four resolve, the three build
/// entries carry `targets_running_root: true`, each names its immediate
/// requester, and `rdrlib` proves RDEPEND walks against the running root too.
#[test]
fn root_deps_recursion_walks_a_build_entrys_own_build_and_runtime_deps() {
    let root = fixtures_root();
    let entries = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/rdrapp".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries;

    // (name, targets_running_root, sole immediate requester or "")
    let seen: Vec<(String, bool, String)> = entries
        .iter()
        .map(|e| {
            let requester = match e.required_by.as_slice() {
                [] => String::new(),
                [(c, p)] => format!("{c}/{p}"),
                many => format!("{many:?}"),
            };
            (
                format!("{}/{}", e.category, e.package),
                e.targets_running_root,
                requester,
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (
                "dev-libs/rdrtooldep".to_string(),
                true,
                "dev-libs/rdrtool".to_string(),
            ),
            (
                "dev-libs/rdrlib".to_string(),
                true,
                "dev-libs/rdrtool".to_string(),
            ),
            (
                "dev-libs/rdrtool".to_string(),
                true,
                "dev-libs/rdrapp".to_string(),
            ),
            ("dev-libs/rdrapp".to_string(), false, String::new())
        ]
    );
}

/// `IDEPEND` of a running-root build entry is walked against the running root
/// too (Portage: `IDEPEND` always targets the running root).
#[test]
fn root_deps_recursion_walks_a_build_entrys_own_idepend() {
    let root = fixtures_root();
    let names: Vec<(String, bool)> = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/rdriapp".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries
    .iter()
    .map(|e| {
        (
            format!("{}/{}", e.category, e.package),
            e.targets_running_root,
        )
    })
    .collect();
    assert_eq!(
        names,
        vec![
            ("dev-libs/rdrilib".to_string(), true),
            ("dev-libs/rdritool".to_string(), true),
            ("dev-libs/rdriapp".to_string(), false)
        ]
    );
}

/// A *top-level* package's own `IDEPEND` resolves against the running root too
/// (Portage: `IDEPEND` always targets the running root for every package).
#[test]
fn root_deps_top_level_idepend_resolves_against_the_running_root() {
    let root = fixtures_root();
    let run = |root_deps_running_root: Option<&Path>| -> Vec<(String, bool)> {
        ResolveRequest {
            deep: Deep::Unlimited,
            root_deps_running_root: root_deps_running_root.map(std::path::Path::to_path_buf),
            ..ResolveRequest::new(
                &root,
                &root,
                &["dev-libs/topidepapp".to_string()],
                &test_config(),
                &fixtures_root().join("distfiles"),
            )
        }
        .resolve()
        .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
        .entries
        .iter()
        .map(|e| {
            (
                format!("{}/{}", e.category, e.package),
                e.targets_running_root,
            )
        })
        .collect()
    };
    assert_eq!(
        run(None),
        vec![
            ("dev-libs/topideplib".to_string(), false),
            ("dev-libs/topidepapp".to_string(), false)
        ]
    );
    assert_eq!(
        run(Some(&root)),
        vec![
            ("dev-libs/topideplib".to_string(), true),
            ("dev-libs/topidepapp".to_string(), false),
        ]
    );
}

/// The g216 shape under default staging: the running-root build dep merges
/// before its target-root consumer, and `g216comp` lands TWICE (one row per
/// root, like Portage's display list).
#[test]
fn cross_root_bdepend_self_branch_merges_in_both_roots() {
    let root = fixtures_root();
    // Hermetic running root: nothing installed there, so every build dep
    // selects from the tree.
    let running = TempDir::new("portuale-242-xc-empty-running-root").keep();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let result = ResolveRequest {
        root_deps_running_root: Some(running.to_path_buf()),
        backtrack_max: 20,
        ..ResolveRequest::new(
            &root,
            &root,
            &["app-misc/g216top".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving app-misc/g216top failed: {e}"));
    // Portage spends its single backtrack retry on the serialize abort.
    assert!(result.backtrack_restarts >= 1);
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    // Merge order runs across roots: the running-root build dep merges before
    // its target-root consumer, and `g216comp` lands twice.
    let rows: Vec<(String, String, bool)> = result
        .entries
        .iter()
        .map(|e| {
            (
                e.category.clone(),
                e.package.clone(),
                e.targets_running_root,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("dev-lang".to_string(), "g216boot".to_string(), true),
            ("dev-lang".to_string(), "g216comp".to_string(), true),
            ("dev-lang".to_string(), "g216comp".to_string(), false),
            ("dev-libs".to_string(), "g216mid".to_string(), false),
            ("app-misc".to_string(), "g216top".to_string(), false),
        ]
    );
}

/// The cycle guard: each cycle node appears exactly once, not overflow the
/// stack (the shared seen set is inserted before a package's own deps walk).
#[test]
fn root_deps_recursion_terminates_on_a_mutual_bdepend_cycle() {
    let root = fixtures_root();
    let names: Vec<String> = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/rdrcyc".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries
    .iter()
    .map(|e| format!("{}/{}", e.category, e.package))
    .collect();
    assert_eq!(
        names,
        vec![
            "dev-libs/rdrcycb".to_string(),
            "dev-libs/rdrcyca".to_string(),
            "dev-libs/rdrcyc".to_string()
        ]
    );
}

/// An unbuildable, not-installed dep of a running-root build entry surfaces as
/// its own `NoVisibleCandidate` entry instead of being silently swallowed.
#[test]
fn root_deps_recursion_surfaces_an_unbuildable_build_dep() {
    let root = fixtures_root();
    let entry = ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: Some(root.to_path_buf()),
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/rdrmiss".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolve_pretend_graph failed: {e}"))
    .entries
    .into_iter()
    .find(|e| e.package == "rdrnothere")
    .expect("rdrnothere should appear as a NoVisibleCandidate entry");
    assert_eq!(entry.outcome, PretendOutcome::NoVisibleCandidate);
    assert!(entry.targets_running_root);
    assert_eq!(
        entry.required_by,
        vec![("dev-libs".to_string(), "rdrmisstool".to_string())]
    );
}

#[test]
fn deep_is_ignored_when_nodeps_disables_the_dependency_walk_entirely() {
    // --nodeps trumps --deep (Portage pops "recurse" outright, checked before
    // `deep` is ever consulted).
    let root = fixtures_root();
    let entries: Vec<(String, PretendOutcome)> = ResolveRequest {
        nodeps: true,
        deep: Deep::Unlimited,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/deeppkg".to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("resolve_pretend_graph must succeed")
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect();
    assert_eq!(
        entries,
        vec![(
            "dev-libs/deeppkg".to_string(),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        )]
    );
}

#[test]
fn recursion_basic_chain() {
    // Without --update it stays AlreadyInstalled, reached here as a dependency
    // instead of a top-level atom.
    let entries = graph("dev-libs/withdeps");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/upgradepkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/withdeps".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn recursion_keeps_a_keyword_masked_but_installed_dependency_instead_of_downgrading() {
    // Reached only as a dependency (the live `sys-fs/fuse` -> `sys-libs/liburing`
    // shape): stays installed instead of downgrading.
    let entries = graph("dev-libs/needskeywordmasked");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/keywordmaskedpkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "2.0".to_string()
                }
            ),
            (
                "dev-libs/needskeywordmasked".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn recursion_keeps_a_keyword_masked_but_installed_dependency_with_a_satisfied_use_dep() {
    // A genuine USE-dep on the same keyword-masked-but-installed shape, checked
    // against the vdb-recorded USE via `use_deps_satisfied` (the live
    // `sys-fs/fuse` -> `sys-libs/liburing:=[...]` shape).
    let entries = graph("dev-libs/needskeywordmaskeduse");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/keywordmaskedusepkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "2.0".to_string()
                }
            ),
            (
                "dev-libs/needskeywordmaskeduse".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn recursion_keeps_an_installed_dependency_whose_use_dep_flag_is_only_in_its_built_use() {
    // The installed 1.0 has vdb USE="divergedflag" but empty vdb IUSE, and the
    // current ebuild dropped the flag entirely -- yet the installed version
    // satisfies the atom (Portage's `_iuse_implicit_built`).
    let entries = graph("dev-libs/needsbuiltusediverge");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/builtusedivergedep".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/needsbuiltusediverge".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn recursion_basic_chain_with_update_keeps_the_dependency() {
    // Same fixture with --update, reached only as a dependency: Portage
    // does NOT want it updated (`_want_update_pkg` false without `--deep`:
    // `depgraph.py:7304-7333`), so the `:8445-8458` return keeps the
    // installed instance and no `U` row prints. Oracle: Portage 3.0.82.2
    // `362-withdeps-pu` (`N newpkg`, `N withdeps`).
    let entries = graph_update("dev-libs/withdeps");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/upgradepkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/withdeps".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn update_without_deep_keeps_a_masked_installed_dependency_silent() {
    // The `:7972-7979` masked-installed keep needs no visibility check, so
    // the dep stays installed (silent in display) instead of downgrading.
    // Oracle: Portage 3.0.82.2 `s0-b-needskeywordmasked`
    // (`N needskeywordmasked` as the only row).
    let entries = graph_update("dev-libs/needskeywordmasked");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/keywordmaskedpkg".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "2.0".to_string()
                }
            ),
            (
                "dev-libs/needskeywordmasked".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn update_without_deep_keeps_an_installed_virtual_provider_silent() {
    // The installed `virtual/libc-1.0` has no tree candidate left (the repo
    // holds only 2.0), but Portage walks the vardb independently and keeps
    // it (`:8445-8458`), so neither the `U` nor the `N fakeglibc` row
    // prints (the kept dep itself is display-silent). Oracle: Portage
    // 3.0.82.2 `s0-b-toolchainroot` (`N plainleaf`, `N toolchainroot` as the
    // only rows).
    let entries = graph_update("dev-libs/toolchainroot");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/plainleaf".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "virtual/libc".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/toolchainroot".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn an_argument_upgrades_even_when_also_reached_as_a_dependency() {
    // A second argument (like an `@world` member: `reset_depth` defaults to
    // true) resets the update depth to 0, so the dependency visit wants the
    // update too. Oracle: Portage 3.0.82.2 `362-dtop-pvu-argdlib`
    // (`U pg34-dlib-2`, `N pg34-dtop`).
    let entries = graph_update_entries(
        &["dev-libs/pg34-dtop", "dev-libs/pg34-dlib"],
        &test_config(),
    );
    assert!(
        entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "pg34-dlib"
            && matches!(
                &e.outcome,
                PretendOutcome::Upgrade { from, to } if from == "1" && to == "2"
            )),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "pg34-dtop"
            && matches!(&e.outcome, PretendOutcome::New { .. })),
        "{entries:?}"
    );
}

#[test]
fn nodeps_resolves_the_top_level_atom_but_never_recurses() {
    // With --nodeps only the top-level atom itself is ever resolved; its own
    // DEPEND/RDEPEND is never even read.
    let entries = graph_nodeps("dev-libs/withdeps");
    assert_eq!(
        entries,
        vec![(
            "dev-libs/withdeps".to_string(),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        )]
    );
}

#[test]
fn nodeps_still_computes_use_flags_display_for_the_top_level_atom() {
    // Portage's own -v USE display is about a package's own metadata, so
    // --nodeps must not blank it out.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let entries = ResolveRequest {
        nodeps: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &["dev-libs/useflagpkg".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("resolve_pretend_graph must succeed")
    .entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].use_flags_display,
        vec![
            ("foo".to_string(), true),
            ("missingflag".to_string(), false),
        ]
    );
}

#[test]
fn use_expand_hidden_group_is_absent_from_the_pv_display() {
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let entry = ResolveRequest::new(
        &root,
        &root,
        &["dev-libs/hiddenexpandpkg".to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect("resolve_pretend_graph must succeed")
    .entries
    .remove(0);
    // cpu_flags_x86_sse2 is a real enabled flag...
    assert!(
        entry
            .use_flags_display
            .contains(&("cpu_flags_x86_sse2".to_string(), true))
    );
    // ...but CPU_FLAGS_X86 is USE_EXPAND_HIDDEN, so it never reaches the
    // `-pv` display.
    assert!(entry.use_expand_display.is_empty());
}

#[test]
fn recursion_walks_bdepend_pdepend_idepend_same_as_depend_rdepend() {
    for pkg in ["bdependpkg", "pdependpkg", "idependpkg"] {
        let entries = graph(&format!("dev-libs/{pkg}"));
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    format!("dev-libs/{pkg}"),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ],
            "{pkg}"
        );
    }
}

#[test]
fn recursion_dedupes_diamond_dependency() {
    let entries = graph("dev-libs/diamond");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "dev-libs/common",
            "dev-libs/shared-a",
            "dev-libs/shared-b",
            "dev-libs/diamond"
        ]
    );
    // "common" must appear exactly once despite being reachable via
    // both shared-a and shared-b.
    assert_eq!(names.iter().filter(|n| **n == "dev-libs/common").count(), 1);
}

#[test]
fn entries_are_returned_in_dependency_first_merge_order() {
    // `topological_merge_order`: the BFS builds entries parent-first, so the
    // graph re-sorts them into dependency-first order before returning.
    let names: Vec<String> = graph("dev-libs/withdeps")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let withdeps = names.iter().position(|n| n == "dev-libs/withdeps").unwrap();
    let newpkg = names.iter().position(|n| n == "dev-libs/newpkg").unwrap();
    let upgradepkg = names
        .iter()
        .position(|n| n == "dev-libs/upgradepkg")
        .unwrap();
    assert!(newpkg < withdeps, "{names:?}");
    assert!(upgradepkg < withdeps, "{names:?}");

    // Diamond: the shared leaf precedes both consumers, which both precede
    // the root.
    let d: Vec<String> = graph("dev-libs/diamond")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let pos = |p: &str| d.iter().position(|n| n == p).unwrap();
    assert!(pos("dev-libs/common") < pos("dev-libs/shared-a"));
    assert!(pos("dev-libs/common") < pos("dev-libs/shared-b"));
    assert!(pos("dev-libs/shared-a") < pos("dev-libs/diamond"));
    assert!(pos("dev-libs/shared-b") < pos("dev-libs/diamond"));
}

#[test]
fn required_by_is_empty_for_a_directly_requested_top_level_atom() {
    let entries = full_graph("dev-libs/newpkg");
    assert_eq!(entries[0].category, "dev-libs");
    assert_eq!(entries[0].package, "newpkg");
    assert_eq!(entries[0].required_by, Vec::new());
}

#[test]
fn required_by_names_the_single_owner_of_a_plain_dependency() {
    // Both deps list withdeps as their only owner.
    let entries = full_graph("dev-libs/withdeps");
    for entry in &entries {
        if entry.category == "dev-libs"
            && (entry.package == "newpkg" || entry.package == "upgradepkg")
        {
            assert_eq!(
                entry.required_by,
                vec![("dev-libs".to_string(), "withdeps".to_string())],
                "{}/{}",
                entry.category,
                entry.package
            );
        }
    }
}

#[test]
fn required_by_lists_every_owner_of_a_diamond_dependency() {
    // Both owners must be recorded (sorted), not just whichever the BFS
    // resolved first.
    let entries = full_graph("dev-libs/diamond");
    let common = entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "common")
        .expect("dev-libs/common must be in the graph");
    assert_eq!(
        common.required_by,
        vec![
            ("dev-libs".to_string(), "shared-a".to_string()),
            ("dev-libs".to_string(), "shared-b".to_string()),
        ]
    );
}

#[test]
fn required_by_is_set_on_every_slot_of_a_multi_slot_dependency() {
    // Two separate entries sharing one (category, package); both must carry
    // the owner (the old destructive map handed it to only the first).
    let entries = full_graph("dev-libs/multislotparent");
    let slots: Vec<&GraphEntry> = entries
        .iter()
        .filter(|e| e.category == "dev-libs" && e.package == "multislotpkg")
        .collect();
    assert_eq!(slots.len(), 2, "both slots must be present");
    for e in slots {
        assert_eq!(
            e.required_by,
            vec![("dev-libs".to_string(), "multislotparent".to_string())],
            "slot {:?} lost its required_by owner",
            e.slot
        );
    }
}

#[test]
fn a_slot_qualified_dep_does_not_edge_to_a_sibling_slot() {
    // `build_digraph` resolves each dep atom to the entries it actually
    // matches before adding an edge (a slot-blind lookup gave phantom edges).
    let names: Vec<String> = graph("dev-libs/slotorderroot")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let pos = |p: &str| names.iter().position(|n| n == p).unwrap();
    assert!(
        pos("dev-libs/slotordera") < pos("dev-libs/slotorderdual"),
        "{names:?}"
    );
    assert!(
        pos("dev-libs/slotorderdual") < pos("dev-libs/slotorderb"),
        "{names:?}"
    );
    assert!(
        pos("dev-libs/slotorderb") < pos("dev-libs/slotorderroot"),
        "{names:?}"
    );
    // both slots still merge -- the fix removes phantom edges, not nodes
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "dev-libs/slotorderdual")
            .count(),
        2,
        "{names:?}"
    );
}

#[test]
fn recursion_terminates_on_a_dependency_cycle() {
    let entries = graph("dev-libs/cycle-a");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["dev-libs/cycle-a", "dev-libs/cycle-b"]);
}

#[test]
fn virtual_cycle_aborts_like_portage_on_the_upstream_fixtures() {
    // Upstream bug 965570: both cells fail in Portage, while portuale merged
    // both as ordinary rings.
    for (top, expected) in [
        ("app-misc/bar", vec!["virtual/gzip-1::testrepo".to_string()]),
        (
            "app-misc/foo",
            vec![
                "virtual/A-1::testrepo".to_string(),
                "virtual/B-1::testrepo".to_string(),
                "virtual/C-1::testrepo".to_string(),
            ],
        ),
    ] {
        let result = graph_result_real(top);
        assert_eq!(result.virtual_cycle, expected, "{top}");
        match &result.outcome {
            ResolveOutcome::Aborted {
                reason: AbortReason::VirtualCycle { members },
                partial,
            } => {
                assert_eq!(members, &expected, "{top}");
                assert!(partial.is_empty(), "{top}: real shows no list at all");
            }
            other => panic!("{top}: expected VirtualCycle, got {other:?}"),
        }
    }
}

#[test]
fn conditional_grandparent_with_undeclared_flag_is_masked_invalid_before_any_cycle_analysis() {
    // The USE-gated cycle is still in the tree, but the target constrains the
    // flag conditionally with `x` in no IUSE: Portage masks the candidate
    // `invalid` at construction, so the resolve fails before any cycle walk.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let err = ResolveRequest::new(
        &root,
        &root,
        &["dev-libs/fucyclec".to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err("fucyclec must fail: its only candidate is masked invalid");
    let Error::Detail(report) = err else {
        panic!("expected the all-masked report, got: {err:?}");
    };
    assert!(
        report.contains(
            "- dev-libs/fucyclec-1.0::testrepo (masked by: invalid: DEPEND: \
             USE flag 'x' referenced in conditional 'x?' in atom \
             'dev-libs/fucyclea[x?]' is not in IUSE)"
        ),
        "{report}"
    );
}

#[test]
fn large_cycle_count_and_display_fire_on_a_four_ring() {
    // A four-ring of build-time deps: Portage records one ring per node, so
    // four records trip `large_cycle_count`.
    let result = graph_result_real("dev-libs/cyc4a");
    assert_eq!(result.circular_deps.len(), 1);
    assert!(
        result.large_cycle_count,
        "four rotations must trip the trailer"
    );
    let mut display = result.cycle_display.clone();
    display.sort();
    assert_eq!(
        display,
        vec![
            "dev-libs/cyc4a-1.0".to_string(),
            "dev-libs/cyc4b-1.0".to_string(),
            "dev-libs/cyc4c-1.0".to_string(),
            "dev-libs/cyc4d-1.0".to_string(),
        ]
    );
}

#[test]
fn small_cycles_report_without_large_count_or_display() {
    // Control: the two-ring still re-displays its two members, but
    // `large_cycle_count` stays false.
    let result = graph_result_real("dev-libs/hardcyclea");
    assert_eq!(result.circular_deps.len(), 1);
    assert!(
        !result.large_cycle_count,
        "two rotations must not trip the trailer"
    );
    assert_eq!(result.cycle_display.len(), 2);
}

#[test]
fn masked_dep_report_carries_the_chain_to_the_argument() {
    // The masked_deps report chain walks the merge parent up to the top-level
    // argument (Portage's dependency-required-by lines).
    let result = graph_result_real("dev-libs/maskneedpkg");
    assert_eq!(result.masked_deps.len(), 1);
    let rep = &result.masked_deps[0];
    assert_eq!(
        (
            rep.category.as_str(),
            rep.package.as_str(),
            rep.atom.as_str()
        ),
        ("dev-libs", "maskeddep", "dev-libs/maskeddep")
    );
    assert_eq!(rep.masked.len(), 1);
    assert_eq!(rep.masked[0].0, "dev-libs/maskeddep-1.0::testrepo");
    assert_eq!(
        rep.chain,
        vec![
            (
                "dev-libs/maskneedpkg-1.0::testrepo".to_string(),
                "ebuild".to_string()
            ),
            ("dev-libs/maskneedpkg".to_string(), "argument".to_string()),
        ]
    );
}

#[test]
fn masked_dep_pubgrub_matches_default_abort() {
    // S9 #364: `--solver=pubgrub` must abort exactly like the default
    // solver when a dependency is masked-only (Portage's "All ebuilds
    // that could satisfy … have been masked" block).
    // Portage: depgraph.py, _show_unsatisfied_dep.
    let default = graph_result_real("dev-libs/maskneedpkg");
    assert!(
        matches!(&default.outcome, ResolveOutcome::Aborted { .. }),
        "the gate exists on the default path: {:?}",
        default.outcome
    );
    let pubgrub = graph_result_pubgrub("dev-libs/maskneedpkg");
    assert_eq!(pubgrub.outcome, default.outcome);
    assert_eq!(pubgrub.masked_deps, default.masked_deps);
    assert!(pubgrub.use_unsat_deps.is_empty());
    assert!(pubgrub.plain_miss_deps.is_empty());
    // Pinned shape (same assertions as
    // `masked_dep_report_carries_the_chain_to_the_argument`).
    assert_eq!(pubgrub.masked_deps.len(), 1);
    let rep = &pubgrub.masked_deps[0];
    assert_eq!(
        (
            rep.category.as_str(),
            rep.package.as_str(),
            rep.atom.as_str()
        ),
        ("dev-libs", "maskeddep", "dev-libs/maskeddep")
    );
    assert_eq!(rep.masked.len(), 1);
    assert_eq!(rep.masked[0].0, "dev-libs/maskeddep-1.0::testrepo");
    assert_eq!(
        rep.chain,
        vec![
            (
                "dev-libs/maskneedpkg-1.0::testrepo".to_string(),
                "ebuild".to_string()
            ),
            ("dev-libs/maskneedpkg".to_string(), "argument".to_string()),
        ]
    );
}

#[test]
fn pg34_gapp_pubgrub_pvud_keeps_installed_only_dep_satisfied() {
    // S9 regression: `--solver=pubgrub -pvuD dev-libs/pg34-gapp` must stay
    // `Complete` with no abort disclosures. `pg34-gone` is installed-only
    // (vdb, no ebuild; its RDEPEND is `pg34-gextra`) and `pg34-gapp` depends
    // on it, so the edge is satisfied by the installed instance exactly as
    // the default walk's `AlreadyInstalled` outcome does before any
    // masked/use-unsat/plain-miss classification.
    // Portage: resolve/pretend.rs:787-824 (installed-only `AlreadyInstalled`
    // fallback); oracle Portage 3.0.82.2 `360-gapp-pvuD` (`N gextra + N gapp`).
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let result = active_resolver_for(SolverKind::PubGrub)
        .resolve(&ResolveRequest {
            update: true,
            deep: Deep::Unlimited,
            ..ResolveRequest::new(
                &root,
                &root,
                &["dev-libs/pg34-gapp".to_string()],
                &config,
                &fixtures_root().join("distfiles"),
            )
        })
        .unwrap_or_else(|e| panic!("pubgrub resolving dev-libs/pg34-gapp failed: {e}"));
    assert!(
        matches!(&result.outcome, ResolveOutcome::Complete),
        "installed-only pg34-gone satisfies the edge, no abort: {:?}",
        result.outcome
    );
    assert!(
        result.masked_deps.is_empty(),
        "no masked disclosure: {:?}",
        result.masked_deps
    );
    assert!(
        result.use_unsat_deps.is_empty(),
        "no use-unsat disclosure: {:?}",
        result.use_unsat_deps
    );
    assert!(
        result.plain_miss_deps.is_empty(),
        "no plain-miss disclosure: {:?}",
        result.plain_miss_deps
    );
}

#[test]
fn mia0a_cells_report_missing_iuse_for_the_conditional_dep() {
    // Both `mia0a` cells record one `Missing IUSE` disclosure with a chain to
    // the argument (Portage's lines).
    for (arg, atom) in [
        ("=dev-libs/mia0a-1", "dev-libs/mia0b[foo?]"),
        ("=dev-libs/mia0a-2", "dev-libs/mia0b[foo?,bar]"),
    ] {
        let result = graph_result_real(arg);
        assert_eq!(result.use_unsat_deps.len(), 1, "{arg}");
        let rep = &result.use_unsat_deps[0];
        assert_eq!(
            (
                rep.category.as_str(),
                rep.package.as_str(),
                rep.atom.as_str()
            ),
            ("dev-libs", "mia0b", atom),
            "{arg}"
        );
        assert_eq!(
            rep.rows,
            vec![(
                "dev-libs/mia0b-1::testrepo".to_string(),
                vec!["Missing IUSE: foo".to_string()]
            )],
            "{arg}"
        );
        assert!(
            result.plain_miss_deps.is_empty(),
            "{arg}: no bare miss once the USE block claims the dep: {:?}",
            result.plain_miss_deps
        );
    }
}

#[test]
fn use_unsat_dep_report_carries_the_chain_to_the_argument() {
    // The Missing IUSE disclosure chain walks the merge parent up to the
    // top-level argument (Portage's lines).
    let result = graph_result_real("dev-libs/unsatuseiuse");
    assert_eq!(result.use_unsat_deps.len(), 1);
    let rep = &result.use_unsat_deps[0];
    assert_eq!(
        (
            rep.category.as_str(),
            rep.package.as_str(),
            rep.atom.as_str()
        ),
        (
            "dev-libs",
            "unsatuseiusetarget",
            "dev-libs/unsatuseiusetarget[noiuse]"
        )
    );
    assert_eq!(
        rep.rows,
        vec![(
            "dev-libs/unsatuseiusetarget-1.0::testrepo".to_string(),
            vec!["Missing IUSE: noiuse".to_string()]
        )]
    );
    assert_eq!(
        rep.chain,
        vec![
            (
                "dev-libs/unsatuseiuse-1.0::testrepo".to_string(),
                "ebuild".to_string()
            ),
            ("dev-libs/unsatuseiuse".to_string(), "argument".to_string()),
        ]
    );
}

#[test]
fn unreachable_installed_pin_does_not_block_an_upgrade() {
    // keeper-1.0 is installed but outside every closure, so its pin never
    // applies and the explicit `=paired-2.0` request merges cleanly.
    let result = graph_result_real("=dev-libs/paired-2.0");
    let paired = result
        .entries
        .iter()
        .find(|e| e.package == "paired")
        .expect("paired resolves");
    assert!(
        matches!(
            &paired.outcome,
            PretendOutcome::Upgrade { to, .. } if to == "2.0"
        ),
        "the upgrade merges, unblocked by keeper's unreachable pin: {:?}",
        paired.outcome
    );
    assert!(
        result.slot_conflicts.is_empty(),
        "keeper is unreachable, so there is no residual conflict: {:?}",
        result.slot_conflicts
    );
}

#[test]
fn build_time_cycle_between_two_installed_packages_is_not_a_hard_cycle() {
    // Both already installed: Portage demotes the build-time edge an installed
    // package provides, so the cycle breaks (contrast the genuine hard cycle,
    // neither installed, which still reports).
    let result = graph_result_empty("dev-libs/instcyclea");
    assert!(
        result.circular_deps.is_empty(),
        "expected no hard cycle, got {:?}",
        result.circular_deps
    );
    let names: Vec<&str> = result.entries.iter().map(|e| e.package.as_str()).collect();
    assert!(names.contains(&"instcyclea") && names.contains(&"instcycleb"));
    assert!(
        result
            .entries
            .iter()
            .all(|e| matches!(e.outcome, PretendOutcome::Reinstall { .. }))
    );
    // sanity: the genuine hard cycle is still detected.
    assert!(
        !graph_result_real("dev-libs/hardcyclea")
            .circular_deps
            .is_empty()
    );
}

#[test]
fn or_dep_prefers_a_branch_already_merge_bound_in_the_graph() {
    // ingraphwallet is already a graph node when ingraphsvc's `||` resolves:
    // Portage's `all_in_graph` bin picks it over the first-listed branch.
    let names: Vec<String> = graph("dev-libs/ingraphany")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(names.contains(&"dev-libs/ingraphwallet".to_string()));
    assert!(!names.contains(&"dev-libs/ingraphkeyring".to_string()));
    assert!(!names.contains(&"dev-libs/ingraphkeyringdep".to_string()));

    // Control: with nothing pulling ingraphwallet in independently,
    // the same `||` falls back to the first-listed branch.
    let ctl: Vec<String> = graph("dev-libs/ingraphnoany")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(ctl.contains(&"dev-libs/ingraphkeyring".to_string()));
    assert!(!ctl.contains(&"dev-libs/ingraphwallet".to_string()));
}

#[test]
fn recursion_prefers_the_installed_any_of_alternative_over_an_earlier_uninstalled_one() {
    // The installed alternative wins even though it's listed second (Portage's
    // `preferred_installed` bin).
    let entries = graph("dev-libs/anyof");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["dev-libs/samepkg", "dev-libs/anyof"]);
}

#[test]
fn recursion_survives_an_unresolvable_dependency() {
    // The unresolvable dependency still shows up (as NoVisibleCandidate, not
    // silently dropped); it just isn't recursed into further.
    let entries = graph("dev-libs/missingdep");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/doesnotexist-anywhere".to_string(),
                PretendOutcome::NoVisibleCandidate
            ),
            (
                "dev-libs/missingdep".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn recursion_dedupes_across_depend_and_rdepend() {
    let entries = graph("dev-libs/dualdep");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["dev-libs/newpkg", "dev-libs/dualdep"]);
}

/// End-to-end wiring check: the fully resolved config proves genuine USE flags
/// reach use_reduce and change dependency resolution.
#[test]
fn real_resolved_use_flags_drive_dependency_recursion() {
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let entries = ResolveRequest::new(
        &root,
        &root,
        &["dev-libs/useflagpkg".to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect("resolve_pretend_graph must succeed")
    .entries;
    let full_names: Vec<String> = entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(full_names, vec!["dev-libs/newpkg", "dev-libs/useflagpkg"]);
}

#[test]
fn use_expand_flag_drives_dependency_recursion() {
    // VIDEO_CARDS="nvidia" expands into "video_cards_nvidia", which the RDEPEND
    // is gated on.
    let full_names: Vec<String> = graph_real("dev-libs/useexpandpkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/newpkg", "dev-libs/useexpandpkg"]);
}

#[test]
fn package_use_expand_shorthand_drives_dependency_recursion() {
    // package.use shorthand expands into the qualified flag the RDEPEND is
    // gated on.
    let full_names: Vec<String> = graph_real("dev-libs/packageuseexpandpkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec!["dev-libs/newpkg", "dev-libs/packageuseexpandpkg"]
    );
}

#[test]
fn use_expand_star_wildcard_enables_every_matching_iuse_flag() {
    // The `_*` wildcard enables every matching flag in the package's OWN IUSE
    // (Portage's setcpv); the masked one stays off, so only its gate fires.
    let full_names: Vec<String> = graph_real("dev-libs/wildexpandpkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec!["dev-libs/wildexpanddep", "dev-libs/wildexpandpkg"]
    );
}

#[test]
fn use_expand_implicit_flag_is_valid_iuse_even_when_unlisted() {
    // elibc_glibc is valid implicit IUSE for every package AND enabled, so the
    // dep resolves (before this slice the declared-only `.required` check
    // left it invisible).
    let entries = graph_real("dev-libs/implicitiusepkg");
    let prov = entries
        .iter()
        .find(|(name, _)| name == "dev-libs/implicitiuseprov")
        .expect("the dep is in the graph");
    // Resolved as a genuine New entry, NOT NoVisibleCandidate.
    assert_eq!(
        prov.1,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn use_expand_implicit_flag_valid_but_not_enabled_still_fails_the_use_dep() {
    // elibc_musl is valid implicit IUSE but not enabled, so the dep is
    // genuinely unsatisfiable (the slice widened the *valid* domain, not the
    // *enabled* one).
    let entries = graph_real("dev-libs/implicitiusepkgmusl");
    let prov = entries
        .iter()
        .find(|(name, _)| name == "dev-libs/implicitiuseprov")
        .expect("the unsatisfiable dep is still recorded");
    assert_eq!(prov.1, PretendOutcome::NoVisibleCandidate);
}

#[test]
fn fixture_stable_use_force_and_package_use_stable_mask_apply_when_stable() {
    // Genuinely stable KEYWORDS: use.stable.force pulls in its own RDEPEND.
    let full_names: Vec<String> = graph_real("dev-libs/stableusepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/newpkg", "dev-libs/stableusepkg"]);
}

#[test]
fn fixture_stable_use_force_skips_an_unstable_candidate() {
    // Genuinely NOT stable: the same use.stable.force never applies.
    let full_names: Vec<String> = graph_real("dev-libs/unstableusepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/unstableusepkg"]);
}

#[test]
fn with_test_deps_pulls_in_a_top_level_atoms_own_test_gated_dependency() {
    let full_names: Vec<String> = graph_real_with_test_deps("dev-libs/withtestdeppkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec![
            "dev-libs/newpkg",
            "dev-libs/testonlydep",
            "dev-libs/withtestdeppkg"
        ]
    );
}

#[test]
fn without_with_test_deps_the_test_gated_dependency_is_absent() {
    let full_names: Vec<String> = graph_real("dev-libs/withtestdeppkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec!["dev-libs/newpkg", "dev-libs/withtestdeppkg"]
    );
}

#[test]
fn with_test_deps_does_not_apply_beyond_a_top_level_atom() {
    // Reached at depth 1, not depth 0: testonlydep stays absent even with
    // --with-test-deps.
    let full_names: Vec<String> = graph_real_with_test_deps("dev-libs/withtestdepconsumer")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec![
            "dev-libs/newpkg",
            "dev-libs/withtestdeppkg",
            "dev-libs/withtestdepconsumer"
        ]
    );
}

#[test]
fn with_test_deps_is_a_no_op_for_a_package_with_no_test_iuse_flag_at_all() {
    // No "test" IUSE flag declared at all: Portage's validity guard means
    // --with-test-deps changes nothing for it.
    assert_eq!(
        graph_real_with_test_deps("dev-libs/newpkg"),
        graph_real("dev-libs/newpkg")
    );
}

#[test]
fn newuse_reinstall_still_recurses_into_its_own_dependencies() {
    // A Reinstall entry is walked for dependencies exactly like New/Upgrade,
    // not treated like the AlreadyInstalled dead-end.
    assert_eq!(
        graph_real_newuse("dev-libs/reinstallpkg"),
        vec![
            (
                "dev-libs/newpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/reinstallpkg".to_string(),
                PretendOutcome::Reinstall {
                    version: "1.0".to_string(),
                    changed_flags: vec!["foo".to_string()],
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: false,
                }
            )
        ]
    );
}

#[test]
fn fixture_package_use_enables_a_flag_not_on_globally() {
    // A package.use wildcard entry enables the gating flag for this package.
    let full_names: Vec<String> = graph_real("dev-libs/packageuseenablepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        full_names,
        vec!["dev-libs/newpkg", "dev-libs/packageuseenablepkg"]
    );
}

#[test]
fn nodeps_skips_recursion_even_when_package_use_would_otherwise_pull_something_in() {
    // Same fixture with --nodeps: only the *recursion* into the now-enabled
    // gate is skipped; visibility/USE still resolve normally.
    let full_names: Vec<String> = graph_real_nodeps("dev-libs/packageuseenablepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/packageuseenablepkg"]);
}

#[test]
fn fixture_package_use_disables_a_flag_that_is_on_globally() {
    // The fixture profile chain enables "foo" globally, but a package.use
    // "-foo" entry scoped to just this package keeps its gate closed.
    let full_names: Vec<String> = graph_real("dev-libs/packageusedisablepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/packageusedisablepkg"]);
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

#[test]
fn use_dep_enforcement_rejects_a_dependency_atom_but_does_not_fail_the_graph() {
    // Genuinely unsatisfiable: the parent still resolves, and the dependency
    // gets its own NoVisibleCandidate entry (reported, not failing the graph).
    let entries = graph_real("dev-libs/usedeprejectedpkg");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/useflagpkg".to_string(),
                PretendOutcome::NoVisibleCandidate
            ),
            (
                "dev-libs/usedeprejectedpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn required_use_satisfied_resolves_normally() {
    // REQUIRED_USE="foo? ( bar )", genuinely satisfied via the package.use
    // entry forcing bar on.
    assert_eq!(
        graph_real("dev-libs/requireduseokpkg"),
        vec![(
            "dev-libs/requireduseokpkg".to_string(),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        )]
    );
}

#[test]
fn required_use_violated_top_level_fails_the_whole_call() {
    // Genuinely violated: Portage aborts the whole run right after package
    // selection (a different severity than a merely unresolvable dependency).
    // The whole one-clause expression is the minimal unsatisfied one, with no
    // dep chain for a top-level atom.
    let err = graph_real_err("dev-libs/requiredusebadpkg");
    assert_eq!(
        err,
        "\n!!! The ebuild selected to satisfy \"dev-libs/requiredusebadpkg\" \
         has unmet requirements.\n\
         - dev-libs/requiredusebadpkg-1.0::testrepo USE=\"foo -bar\" ELIBC=\"glibc\"\n\
         \n  The following REQUIRED_USE flag constraints are unsatisfied:\n\
         \x20   foo? ( bar )\n\n"
    );
}

#[test]
fn required_use_violated_dependency_still_fails_the_whole_call() {
    // Same fatal severity whether reached as a top-level atom or a dependency.
    // As a dependency the block gains the chain plus a trailing `[argument]`
    // line since the parent is itself the CLI argument.
    let err = graph_real_err("dev-libs/requiredusebadparentpkg");
    assert_eq!(
        err,
        "\n!!! The ebuild selected to satisfy \"dev-libs/requiredusebadpkg\" \
         has unmet requirements.\n\
         - dev-libs/requiredusebadpkg-1.0::testrepo USE=\"foo -bar\" ELIBC=\"glibc\"\n\
         \n  The following REQUIRED_USE flag constraints are unsatisfied:\n\
         \x20   foo? ( bar )\n\n\
         (dependency required by \"dev-libs/requiredusebadparentpkg-1.0::testrepo\" [ebuild])\n\
         (dependency required by \"dev-libs/requiredusebadparentpkg\" [argument])\n"
    );
}

#[test]
fn required_use_violations_are_collected_across_the_whole_walk_not_just_the_first() {
    // Portage does NOT abort the whole walk on a violation, so a SECOND,
    // independent top-level atom's own violation still gets reported too.
    // Report order follows the walk (last-declared-first), so badpkg2's block
    // leads. (Portage itself stops at the first violation; collecting both
    // stays portuale's own separately-pinned behavior.)
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let err = ResolveRequest::new(
        &root,
        &root,
        &[
            "dev-libs/requiredusebadpkg".to_string(),
            "dev-libs/requiredusebadpkg2".to_string(),
        ],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err("both atoms should fail their own REQUIRED_USE");
    assert_eq!(
        err.to_string(),
        "\n!!! The ebuild selected to satisfy \"dev-libs/requiredusebadpkg2\" \
         has unmet requirements.\n\
         - dev-libs/requiredusebadpkg2-1.0::testrepo USE=\"baz -qux\" ELIBC=\"glibc\"\n\
         \n  The following REQUIRED_USE flag constraints are unsatisfied:\n\
         \x20   baz? ( qux )\n\n\
         \n!!! The ebuild selected to satisfy \"dev-libs/requiredusebadpkg\" \
         has unmet requirements.\n\
         - dev-libs/requiredusebadpkg-1.0::testrepo USE=\"foo -bar\" ELIBC=\"glibc\"\n\
         \n  The following REQUIRED_USE flag constraints are unsatisfied:\n\
         \x20   foo? ( bar )\n\n"
    );
}

#[test]
fn required_use_pubgrub_top_level_matches_default_refusal() {
    // S9 #365: `--solver=pubgrub` must refuse exactly like the default
    // solver when a selected ebuild violates REQUIRED_USE (Portage's
    // post-selection check in `depgraph.py _add_pkg`).
    let default = graph_real_err("dev-libs/requiredusebadpkg");
    let pubgrub = graph_pubgrub_err("dev-libs/requiredusebadpkg");
    assert_eq!(pubgrub, default);
}

#[test]
fn required_use_pubgrub_dependency_matches_default_refusal() {
    // Same fatal severity reached only as a dependency: the block gains
    // the chain plus the trailing `[argument]` line.
    let default = graph_real_err("dev-libs/requiredusebadparentpkg");
    let pubgrub = graph_pubgrub_err("dev-libs/requiredusebadparentpkg");
    assert_eq!(pubgrub, default);
}

#[test]
fn required_use_pubgrub_two_violations_match_default_order() {
    // Portage collects every violation before failing; the walk order is
    // last-declared-first, so badpkg2's block leads.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atoms = vec![
        "dev-libs/requiredusebadpkg".to_string(),
        "dev-libs/requiredusebadpkg2".to_string(),
    ];
    let req = ResolveRequest::new(
        &root,
        &root,
        &atoms,
        &config,
        &fixtures_root().join("distfiles"),
    );
    let default = req
        .resolve()
        .expect_err("both atoms should fail their own REQUIRED_USE")
        .to_string();
    let pubgrub = active_resolver_for(SolverKind::PubGrub)
        .resolve(&req)
        .expect_err("pubgrub should fail both atoms' REQUIRED_USE too")
        .to_string();
    assert_eq!(pubgrub, default);
}

#[test]
fn fixture_dependency_no_visible_candidate_gets_no_keyword_suggestion_by_default() {
    // A dependency's own NoVisibleCandidate carries a suggestion only once
    // autounmask_suggest_keywords is on.
    let entries = graph_entries_real("dev-libs/autounmaskdepconsumer");
    let dep = entries
        .iter()
        .find(|e| e.package == "autounmaskkeywordpkg")
        .expect("dependency entry present");
    assert_eq!(dep.outcome, PretendOutcome::NoVisibleCandidate);
    assert_eq!(dep.keyword_suggestion, None);
}

#[test]
fn fixture_dependency_no_visible_candidate_gets_a_keyword_suggestion_once_enabled() {
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atoms = vec!["dev-libs/autounmaskdepconsumer".to_string()];
    let result = ResolveRequest {
        autounmask_suggest_keywords: true,
        autounmask_suggest_use: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &atoms,
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("dependency's own NoVisibleCandidate is never fatal");
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "autounmaskkeywordpkg")
        .expect("dependency entry present");
    // With `--autounmask` explicit the keyword-masked dependency now RESOLVES
    // (was `NoVisibleCandidate` + a suggestion before keyword resolution).
    assert!(matches!(dep.outcome, PretendOutcome::New { .. }));
    assert_eq!(dep.keyword_mask, Some('~'));
    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec![
            "required by dev-libs/autounmaskdepconsumer-1.0::testrepo".to_string(),
            "required by dev-libs/autounmaskdepconsumer (argument)".to_string(),
        ]
    );
}

#[test]
fn top_level_use_miss_reports_the_use_block_without_chain() {
    // An *argument* atom's own USE miss aborts with the "no ebuilds built with
    // USE flags" block: the `(Change USE: …)` row and no dep-chain lines
    // (Portage skips the chain for an argument parent).
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atoms = vec!["dev-libs/useflagpkg[-foo]".to_string()];
    let err = ResolveRequest::new(
        &root,
        &root,
        &atoms,
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err("no visible candidate at all");
    assert_eq!(
        err.to_string(),
        "there are no ebuilds built with USE flags to satisfy \
         \"dev-libs/useflagpkg[-foo]\".\n\
         !!! One of the following packages is required to complete your request:\n\
         - dev-libs/useflagpkg-1.0::testrepo (Change USE: -foo)"
    );
}

#[test]
fn fixture_dependency_no_visible_candidate_gets_no_use_suggestion_by_default() {
    // A dependency's own NoVisibleCandidate carries a use_suggestion only once
    // autounmask_suggest_use is on.
    let entries = graph_entries_real("dev-libs/usedeprejectedpkg");
    let dep = entries
        .iter()
        .find(|e| e.package == "useflagpkg")
        .expect("dependency entry present");
    assert_eq!(dep.outcome, PretendOutcome::NoVisibleCandidate);
    assert_eq!(dep.use_suggestion, None);
}

#[test]
fn required_use_referencing_an_implicit_arch_list_flag_is_valid() {
    // "!x86" with "x86" never declared in IUSE is still valid via
    // PORTAGE_ARCHLIST (like Portage's media-libs/mesa); x86 stays disabled
    // here, so "!x86" is satisfied.
    assert_eq!(
        graph_real("dev-libs/archiuseimplicitpkg"),
        vec![(
            "dev-libs/archiuseimplicitpkg".to_string(),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        )]
    );
}

#[test]
fn aub0_backtrack_y_fails_on_a_with_the_order_dependent_slot_prefix() {
    for (order, prefix) in [
        ("c b a", vec![]),
        ("c a b", vec![]),
        ("b c a", vec![]),
        ("b a c", vec![]),
        ("a c b", vec![vec!["0".to_string(), "1".to_string()]]),
        ("a b c", vec![vec!["1".to_string(), "0".to_string()]]),
    ] {
        let result = aub0_result(order, true);
        assert_aub0_fails_on_a(&result, order);
        assert_eq!(aub0_conflict_versions(&result), prefix, "{order}");
    }
}

#[test]
fn aub0_backtrack_n_fails_or_stops_early_by_walk_order() {
    // A walked before B: B contradicts A's needed `-foo`, so the clean pass
    // fails on A with no conflict reached.
    for order in ["c b a", "b c a", "b a c"] {
        let result = aub0_result(order, false);
        assert_aub0_fails_on_a(&result, order);
        assert!(result.slot_conflicts.is_empty(), "{order}");
    }
    // B walked before A: A's `-foo` is the first needed change; the loop stops
    // on it and the pass is the report.
    for (order, instances) in [
        ("c a b", ["0", "1"]),
        ("a c b", ["0", "1"]),
        ("a b c", ["1", "0"]),
    ] {
        let result = aub0_result(order, false);
        assert!(
            matches!(result.outcome, ResolveOutcome::Complete),
            "{order}: {:?}",
            result.outcome
        );
        assert!(result.settled_with_backtracking, "{order}");
        assert_eq!(
            result
                .autounmask_use_changes
                .iter()
                .map(|c| (c.atom.as_str(), c.token.as_str()))
                .collect::<Vec<_>>(),
            vec![("=dev-libs/aub0d-0", "-foo")],
            "{order}"
        );
        assert_eq!(
            result.autounmask_use_changes[0].dep_chain,
            vec![
                "required by dev-libs/aub0a-0::testrepo".to_string(),
                "required by dev-libs/aub0a (argument)".to_string(),
            ],
            "{order}"
        );
        assert_eq!(
            aub0_conflict_versions(&result),
            vec![instances.iter().map(|v| v.to_string()).collect::<Vec<_>>()],
            "{order}"
        );
        let d0 = result.slot_conflicts[0]
            .instances
            .iter()
            .find(|i| i.version == "0")
            .unwrap();
        assert_eq!(
            d0.parents
                .iter()
                .map(|p| p.atom.as_str())
                .collect::<Vec<_>>(),
            vec!["dev-libs/aub0d[foo]", "dev-libs/aub0d[-foo]"],
            "{order}: B first (walked first), then A, whose change broke it"
        );
        assert_eq!(
            d0.use_display,
            vec![
                ("USE".to_string(), "-foo".to_string()),
                ("ELIBC".to_string(), "glibc".to_string()),
            ],
            "{order}"
        );
    }
}

#[test]
fn autounmask_use_n_leaves_a_masked_child_flag_dep_unresolved() {
    // With `--autounmask-use=n` neither the child nor the parent flip is
    // attempted: the dependency stays NoVisibleCandidate with no change.
    let result = graph_result_autounmask_use_n("dev-libs/parentflipeqpkg");
    let child = result
        .entries
        .iter()
        .find(|e| e.package == "parentflipchildpkg")
        .expect("child entry");
    assert!(matches!(child.outcome, PretendOutcome::NoVisibleCandidate));
    assert!(result.autounmask_use_changes.is_empty());
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

/// The USE-change chain starts at the recorded forcing parent (Portage picks
/// the parent whose atom the package still does not satisfy).
#[test]
fn use_change_chain_starts_at_the_recorded_forcing_parent() {
    let result = graph_result_abk0_worldb(2);
    assert_eq!(
        result.autounmask_use_changes.len(),
        1,
        "one USE change: {:?}",
        result.autounmask_use_changes
    );
    let change = &result.autounmask_use_changes[0];
    assert_eq!(change.atom, ">=dev-libs/abk0c-1");
    assert_eq!(change.token, "x y");
    assert_eq!(
        change.trigger,
        Some(("dev-libs".to_string(), "abk0d".to_string())),
        "the flip site records its forcing owner"
    );
    assert_eq!(
        change.dep_chain,
        vec![
            "required by dev-libs/abk0d-1::testrepo".to_string(),
            "required by dev-libs/abk0d (argument)".to_string(),
        ]
    );
}

#[test]
fn installed_instance_then_merge_records_an_unsolvable_slot_conflict() {
    // Direction 2, argv `[othermod, needer]`: last-declared-first graphs 2.0
    // first, then collides with the installed 1.0 -- the merge instance files
    // first (Portage's notice order for this argv order).
    let result = graph_result_real_atoms(
        &[
            "dev-libs/othermod".to_string(),
            "dev-libs/needer".to_string(),
        ],
        10,
    );
    assert_triangle_conflict(&result, false);
    // The upgrade still merges, like Portage's merge list.
    let paired = result
        .entries
        .iter()
        .find(|e| e.package == "paired")
        .expect("paired entry");
    assert_eq!(
        paired.outcome,
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string()
        }
    );
}

#[test]
fn merge_then_installed_instance_records_the_same_slot_conflict() {
    // Direction 1, the mirror argv order: the installed instance files first
    // (Portage's notice order for this argv order).
    let result = graph_result_real_atoms(
        &[
            "dev-libs/needer".to_string(),
            "dev-libs/othermod".to_string(),
        ],
        10,
    );
    assert_triangle_conflict(&result, true);
}

#[test]
fn solvable_installed_instance_collision_records_nothing() {
    // One graphed instance satisfies every hard parent atom, so the conflict
    // reconciles silently through the existing `slot_want` check.
    for argv in [
        [
            "dev-libs/plainuser".to_string(),
            "dev-libs/needer".to_string(),
        ],
        [
            "dev-libs/needer".to_string(),
            "dev-libs/plainuser".to_string(),
        ],
    ] {
        let result = graph_result_real_atoms(&argv, 10);
        assert_eq!(result.slot_conflicts, vec![], "argv {argv:?}");
        let paired = result
            .entries
            .iter()
            .find(|e| e.package == "paired")
            .expect("paired entry");
        assert_eq!(
            paired.outcome,
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string()
            },
            "argv {argv:?}"
        );
    }
}

#[test]
fn ignore_built_slot_operator_deps_skips_the_rebuild_scan() {
    // A test-local vdb: the consumer is normally rebuilt on the target upgrade
    // unless `ignore_built_slot_operator_deps` is set.
    let cfg_root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &cfg_root,
        main_repo_location: &cfg_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), cfg_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &cfg_root,
    })
    .expect("fixture config resolves");
    // Complete mode: `slotbindconsumer` is in `@world`, so the
    // slot-op scan considers it (see `required_set_reachable_cps`).
    config.complete_seed_atoms = vec!["dev-libs/slotbindconsumer".to_string()];
    let dir = TempDir::new("portage-repo-ignoreslotop").keep();
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("slotbindtarget-1.0", "2", "");
    mk("slotbindconsumer-1.0", "0", "dev-libs/slotbindtarget:2/2=");

    let resolve = |ignore: bool| {
        ResolveRequest {
            // selective = false: a directly-named installed atom with a higher
            // version still upgrades (matching Portage without --noreplace).
            selective: false,
            ignore_built_slot_operator_deps: ignore,
            ..ResolveRequest::new(
                &cfg_root,
                &dir,
                &["dev-libs/slotbindtarget".to_string()],
                &config,
                &fixtures_root().join("distfiles"),
            )
        }
        .resolve()
        .expect("resolves")
    };

    let normal = resolve(false);
    let consumer = normal
        .entries
        .iter()
        .find(|e| e.package == "slotbindconsumer")
        .expect("consumer is rebuilt by default");
    assert!(!normal.abi_rebuilds.is_empty());
    // The rebuilt consumer is a *walked node*, not a post-pass synthetic entry:
    // it carries the tree ebuild's own dependency edges, which is what puts
    // the provider ahead of it in merge order.
    assert!(
        matches!(
            consumer.outcome,
            PretendOutcome::Reinstall {
                slot_operator_rebuild: true,
                ..
            }
        ),
        "outcome flipped by the replace set, not synthesised: {:?}",
        consumer.outcome
    );
    assert!(
        consumer
            .deps
            .iter()
            .any(|d| d.atom.starts_with("dev-libs/slotbindtarget")),
        "the walked rebuild owns its tree deps: {:?}",
        consumer.deps
    );
    let pos = |pkg: &str| {
        normal
            .entries
            .iter()
            .position(|e| e.package == pkg)
            .expect("entry")
    };
    assert!(
        pos("slotbindtarget") < pos("slotbindconsumer"),
        "the provider merges before the consumer it forces a rebuild of"
    );
    // Exactly one entry per cp: the flip happens where the outcome is first
    // decided, so no `AlreadyInstalled` row is left behind.
    assert_eq!(
        normal
            .entries
            .iter()
            .filter(|e| e.package == "slotbindconsumer")
            .count(),
        1
    );

    let ignored = resolve(true);
    assert!(
        !ignored
            .entries
            .iter()
            .any(|e| e.package == "slotbindconsumer"),
        "--ignore-built-slot-operator-deps drops the consumer rebuild"
    );
    assert!(ignored.abi_rebuilds.is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dependency_avoid_update_candidate_is_slot_aware() {
    // Installed at SLOT="2" while the repo ebuild says SLOT="1": the `:1`
    // dependency must NOT short-circuit to "already installed".
    let cfg_root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &cfg_root,
        main_repo_location: &cfg_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), cfg_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &cfg_root,
    })
    .expect("fixture config resolves");
    let dir = TempDir::new("portage-repo-avoidslot").keep();
    let d = dir.join("var/db/pkg/dev-libs/avoidslotpkg-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "2\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();

    let result = ResolveRequest {
        selective: false,
        ..ResolveRequest::new(
            &cfg_root,
            &dir,
            &["dev-libs/avoidslotconsumer".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("resolves");

    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "avoidslotpkg")
        .expect("the :1 dependency is a real graph entry, not silently AlreadyInstalled");
    assert!(
        matches!(dep.outcome, PretendOutcome::New { .. }),
        "slot 1 is new: {:?}",
        dep.outcome
    );
    assert_eq!(dep.slot.as_deref(), Some("1"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn graph_entry_carries_sub_slot_repo_and_oldbest_for_slot_repo_decoration() {
    // A New from testrepo: sub_slot/repo populated, oldbest empty.
    let n = &graph_entries_real("dev-libs/newpkg")[0];
    assert_eq!(n.sub_slot.as_deref(), Some("0"));
    assert_eq!(n.repo_name.as_deref(), Some("testrepo"));
    assert!(n.oldbest.is_empty());

    // A sub-slotted dep: sub_slot != slot is carried through.
    let entries = graph_entries_real("dev-libs/subslotconsumer");
    let sub = entries.iter().find(|e| e.package == "subslotpkg").unwrap();
    assert_eq!(sub.slot.as_deref(), Some("0"));
    assert_eq!(sub.sub_slot.as_deref(), Some("2"));

    // A new-slot New: oldbest lists every installed version (all slots).
    let ns = &graph_entries_real("dev-libs/newslotpkg:1")[0];
    assert!(ns.new_slot);
    assert_eq!(
        ns.oldbest
            .iter()
            .map(|r| r.version.as_str())
            .collect::<Vec<_>>(),
        vec!["1.0"]
    );
    assert_eq!(ns.oldbest[0].slot, "0");
}

#[test]
fn package_provided_drops_a_matching_dependency_from_the_walk() {
    // The provided dep is silently dropped; pprovided_atoms stays empty (it's a
    // dependency, not a top-level target).
    let result = graph_result_real("dev-libs/needsprovided");
    let names: Vec<&str> = result.entries.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["newpkg", "needsprovided"]);
    assert!(result.pprovided_atoms.is_empty());
}

#[test]
fn package_provided_records_a_matching_top_level_target_for_the_warning() {
    // A direct `emerge providedpkg` resolves nothing (no entry) and records the
    // atom in pprovided_atoms (Portage's own `_pprovided_args`).
    let result = graph_result_real("dev-libs/providedpkg");
    assert!(result.entries.is_empty());
    assert_eq!(
        result.pprovided_atoms,
        vec!["dev-libs/providedpkg".to_string()]
    );
}

#[test]
fn buildpkgonly_flags_a_new_package_depending_on_another_new_package() {
    // Both DEPEND and RDEPEND on another New package: --buildpkgonly can't
    // resolve this, since the dep itself would also need building.
    let result = graph_result_buildpkgonly("dev-libs/dualdep");
    assert!(result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn buildpkgonly_does_not_fire_when_the_dependency_is_already_installed() {
    // RDEPENDs on an already-installed package: nothing else needs building.
    let result = graph_result_buildpkgonly("dev-libs/buildpkgonlysatisfied");
    assert!(!result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn buildpkgonly_is_always_false_when_not_requested() {
    // `graph_result_real` always passes buildpkgonly=false -- even for the same
    // dualdep fixture that trips the check above, the field must stay false
    // when the flag isn't given at all.
    let result = graph_result_real("dev-libs/dualdep");
    assert!(!result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn fixture_strong_blocker_matches_an_installed_package() {
    // A plain `emerge -p` prints `b` + `[uninstall samepkg-1.0]` (rc 0); the
    // other installed fixtures that RDEPEND on samepkg are not walked, so
    // they do not block the unmerge.
    let entries = graph_entries_real("dev-libs/blockerpkg");
    // The satisfied block also produces the removal entry Portage models as an
    // uninstall task.
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].blockers,
        vec![BlockerConflict {
            atom_str: "!!dev-libs/samepkg".to_string(),
            strong: true,
            matched_category: "dev-libs".to_string(),
            matched_package: "samepkg".to_string(),
            matched_version: "1.0".to_string(),
            unsolvable: false,
            // Resolved by removing the installed match.
            satisfied_by: Some(BlockerSatisfiedBy::Uninstall {
                cpv: "dev-libs/samepkg-1.0".to_string(),
                anchor: ("dev-libs".to_string(), "blockerpkg".to_string()),
            }),
            tree_scheduled_uninstall: false,
        }]
    );
}

#[test]
fn blocker_retry_seeds_make_an_uninstall_resolved_block_unsolvable() {
    // Complete mode re-runs the serialize in the retry walk covering
    // `@selected ∪ @profile ∪ @system`, so an installed blocked instance
    // reachable from a world member is a digraph node with parents and the
    // block is `B` (rc 1).
    //
    // With no seeds the block is uninstall-resolved (`b`, the pin above);
    // with installed `changeddepspkg` as the world seed, the retry closure
    // reaches `samepkg` -> unresolved.
    let entries = graph_entries_with_retry_seeds("dev-libs/blockerpkg", &[]);
    assert!(!entries[0].blockers[0].unsolvable);
    let entries =
        graph_entries_with_retry_seeds("dev-libs/blockerpkg", &["dev-libs/changeddepspkg"]);
    assert!(
        entries[0].blockers[0].unsolvable,
        "the world member's installed closure is a parent"
    );
}

#[test]
fn fixture_weak_blocker_matches_another_new_package_in_the_same_graph() {
    // The weak blocker is reported against the graph-resolved version, not
    // just against the (empty) vdb.
    let entries = graph_entries_real("dev-libs/graphblockerparent");
    let full_names: Vec<String> = entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert_eq!(
        full_names,
        vec![
            "dev-libs/blockerpartnerpkg",
            "dev-libs/weakblockerpkg",
            "dev-libs/graphblockerparent"
        ]
    );
    let by_pkg = |p: &str| entries.iter().find(|e| e.package == p).unwrap();
    assert!(by_pkg("blockerpartnerpkg").blockers.is_empty());
    assert!(by_pkg("graphblockerparent").blockers.is_empty());
    // weakblockerpkg's own RDEPEND carries the `!dev-libs/blockerpartnerpkg`
    // weak blocker.
    assert_eq!(
        by_pkg("weakblockerpkg").blockers,
        vec![BlockerConflict {
            atom_str: "!dev-libs/blockerpartnerpkg".to_string(),
            strong: false,
            matched_category: "dev-libs".to_string(),
            matched_package: "blockerpartnerpkg".to_string(),
            matched_version: "1.0".to_string(),
            // A merge-bound match with a merging parent is unresolved (Portage
            // prints `[blocks B]` for this merge-vs-merge shape).
            unsolvable: true,
            satisfied_by: None,
            tree_scheduled_uninstall: false,
        }]
    );
}

#[test]
fn fixture_unrelated_packages_report_no_blockers() {
    // Regression guard: the diamond fixture has no blockers at all, so
    // none of its entries should gain a spurious one.
    let entries = graph_entries_real("dev-libs/diamond");
    assert!(entries.iter().all(|e| e.blockers.is_empty()));
}

/// An exact-version top-level request keeps its named installed instance
/// under `--update` (the argument pins it; the rebuilt consumer reuses it
/// instead of upgrading).
#[test]
fn exact_version_update_keeps_arg_pinned_provider() {
    let base = TempDir::new("portage-repo-285-exact-update").keep();
    for (name, slot, rdepend) in [
        ("mmprov-1", "0/1", ""),
        ("mmprov-2", "1/1", ""),
        ("mmcons-1", "0", "app-misc/mmprov:0/1="),
    ] {
        let d = base.join("var/db/pkg/app-misc").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-misc\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        fs::write(d.join("EAPI"), "8\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    fs::create_dir_all(base.join("var/lib/portage")).unwrap();
    fs::write(base.join("var/lib/portage/world"), "").unwrap();
    fs::create_dir_all(base.join("distfiles")).unwrap();
    let root = fixtures_root();
    let resolve = |atoms: &[&str]| {
        let req = ResolveRequest {
            config_root: root.clone(),
            root: base.clone(),
            atoms: atoms.iter().map(|a| a.to_string()).collect(),
            argument_sets: HashMap::new(),
            config: test_config(),
            newuse: false,
            changed_use: false,
            nodeps: false,
            onlydeps: false,
            onlydeps_with_rdeps: true,
            onlydeps_with_ideps: false,
            update: true,
            deep: Deep::Unlimited,
            excluded: Vec::new(),
            with_bdeps: true,
            changed_deps: false,
            changed_slot: false,
            with_test_deps: false,
            changed_deps_report: false,
            selective: true,
            autounmask_suggest_keywords: false,
            autounmask_suggest_use: false,
            autounmask_suggest_license: false,
            autounmask_suggest_masks: false,
            usepkg: false,
            usepkgonly: false,
            binpkg_respect_use: false,
            usepkg_exclude: Vec::new(),
            usepkg_include: Vec::new(),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            newrepo: false,
            buildpkgonly: false,
            root_deps_running_root: None,
            distdir: base.join("distfiles"),
            empty: false,
            getbinpkg: false,
            ignore_built_slot_operator_deps: false,
            backtrack_max: 3,
            reinstall_atoms: Vec::new(),
            rebuild_if_new_slot: true,
            rebuild_if_unbuilt: false,
            rebuild_if_new_rev: false,
            rebuild_if_new_ver: false,
            rebuild_exclude: Vec::new(),
            rebuild_ignore: Vec::new(),
            dynamic_deps: true,
            implicit_system_deps: true,
            complete: false,
            set_args: std::collections::HashSet::new(),
            solver: SolverKind::Portage,
        };
        active_resolver()
            .resolve(&req)
            .unwrap_or_else(|e| panic!("resolve({atoms:?}) failed: {e}"))
    };
    // Merge-bound `(cat/pkg, version)` pairs (`AlreadyInstalled` merges nothing
    // and renders no row).
    let merges = |result: &GraphResult| {
        result
            .entries
            .iter()
            .filter_map(|e| match &e.outcome {
                PretendOutcome::New { version } => {
                    Some((format!("{}/{}", e.category, e.package), version.clone()))
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some((format!("{}/{}", e.category, e.package), to.clone()))
                }
                PretendOutcome::Reinstall { version, .. } => {
                    Some((format!("{}/{}", e.category, e.package), version.clone()))
                }
                PretendOutcome::AlreadyInstalled { .. }
                | PretendOutcome::NoVisibleCandidate
                | PretendOutcome::Uninstall { .. } => None,
            })
            .collect::<Vec<_>>()
    };
    for atoms in [
        ["=app-misc/mmprov-2", "app-misc/mmcons"],
        ["~app-misc/mmprov-2", "app-misc/mmcons"],
    ] {
        let result = resolve(&atoms);
        assert_eq!(
            merges(&result),
            vec![("app-misc/mmcons".to_string(), "1".to_string())],
            "exact-version pin keeps mmprov-2, only the consumer rebuilds: {atoms:?}"
        );
        let consumer = result
            .entries
            .iter()
            .find(|e| e.package == "mmcons")
            .expect("mmcons entry");
        assert!(
            matches!(
                consumer.outcome,
                PretendOutcome::Reinstall {
                    slot_operator_rebuild: true,
                    ..
                }
            ),
            "the consumer rebuilds through the slot-operator path: {:?}",
            consumer.outcome
        );
    }
    // Control: the slot atom upgrades the provider itself, so the rebuilt
    // consumer binds the upgrade.
    let result = resolve(&["app-misc/mmprov:1/2", "app-misc/mmcons"]);
    assert_eq!(
        merges(&result),
        vec![
            ("app-misc/mmprov".to_string(), "3".to_string()),
            ("app-misc/mmcons".to_string(), "1".to_string()),
        ],
        "slot-atom control still upgrades the provider"
    );
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn newuse_without_deep_discards_a_use_drift_reinstall_of_an_unwanted_dep() {
    // `pg34-ulib-1` is installed without `x` while the config enables it, so
    // `--newuse` hides the installed instance — but Portage then drops the
    // edge (`reinstall=True` plus `_ignore_dependency`): display prints
    // `N pg34-uapp` alone (the discarded edge leaves a silent
    // `AlreadyInstalled` entry). Oracle: Portage 3.0.82.2 `dbg-uapp-pvN`
    // and `362-uapp-pvN`.
    for update in [false, true] {
        let entries = graph_real_with("dev-libs/pg34-uapp", |r| {
            r.newuse = true;
            r.update = update;
        });
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/pg34-ulib".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1".to_string()
                    }
                ),
                (
                    "dev-libs/pg34-uapp".to_string(),
                    PretendOutcome::New {
                        version: "1".to_string()
                    }
                ),
            ],
            "update={update}"
        );
    }
}

#[test]
fn newuse_with_deep_keeps_a_use_drift_reinstall_of_an_unwanted_dep() {
    // Same shape with `--deep`: the walk recurses into the installed
    // subtree (`recurse_satisfied`), so the edge survives and the `R` plus
    // the `N pg34-uextra` rows print. Oracle: Portage 3.0.82.2
    // `362-uapp-pvDN` (the `-pvuD` and `-pvuDN` controls share these rows).
    let entries = graph_real_with("dev-libs/pg34-uapp", |r| {
        r.newuse = true;
        r.deep = Deep::Unlimited;
    });
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/pg34-uextra".to_string(),
                PretendOutcome::New {
                    version: "1".to_string()
                }
            ),
            (
                "dev-libs/pg34-ulib".to_string(),
                PretendOutcome::Reinstall {
                    version: "1".to_string(),
                    changed_flags: vec!["x".to_string()],
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: false,
                }
            ),
            (
                "dev-libs/pg34-uapp".to_string(),
                PretendOutcome::New {
                    version: "1".to_string()
                }
            ),
        ]
    );
}

/// An installed-only dependency under `--update --deep` is kept, and the
/// deep walk reads its vdb-recorded deps though no ebuild exists: `N gextra
/// + N gapp`. Oracle: Portage 3.0.82.2 `360-gapp-pvuD` (and
/// `360-gapp-pvuD-bdn` with `--with-bdeps=n`, same rows).
#[test]
fn deep_update_walks_the_vdb_deps_of_an_installed_only_dependency() {
    for with_bdeps in [true, false] {
        let entries = graph_with("dev-libs/pg34-gapp", |r| {
            r.update = true;
            r.deep = Deep::Unlimited;
            r.with_bdeps = with_bdeps;
        });
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/pg34-gextra".to_string(),
                    PretendOutcome::New {
                        version: "1".to_string()
                    }
                ),
                (
                    "dev-libs/pg34-gone".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1".to_string()
                    }
                ),
                (
                    "dev-libs/pg34-gapp".to_string(),
                    PretendOutcome::New {
                        version: "1".to_string()
                    }
                ),
            ],
            "with_bdeps={with_bdeps}"
        );
    }
}

/// An installed-only `BDEPEND` edge satisfies a package being merged, with
/// or without `--with-bdeps`: Portage consults the installed db for every
/// dep kind. Oracle: Portage 3.0.82.2 S0 `s0-e-gbapp-pvuD` (and
/// `s0-e-gbapp-pvuD-bdy` with `--with-bdeps=y`, same rows).
#[test]
fn deep_update_keeps_an_installed_only_build_time_dependency() {
    for with_bdeps in [true, false] {
        let entries = graph_with("dev-libs/pg359-gbapp", |r| {
            r.update = true;
            r.deep = Deep::Unlimited;
            r.with_bdeps = with_bdeps;
        });
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/pg34-gextra".to_string(),
                    PretendOutcome::New {
                        version: "1".to_string()
                    }
                ),
                (
                    "dev-libs/pg34-gone".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1".to_string()
                    }
                ),
                (
                    "dev-libs/pg359-gbapp".to_string(),
                    PretendOutcome::New {
                        version: "1".to_string()
                    }
                ),
            ],
            "with_bdeps={with_bdeps}"
        );
    }
}

/// An installed-only top-level argument under `--update --selective` is
/// already installed (`Total: 0`); under `--deep` its vdb deps still walk.
/// Oracle: Portage 3.0.82.2 `360-gone-pvu` (`[]`) and `360-gone-pvuD`
/// (`N gextra`).
#[test]
fn update_selective_keeps_an_installed_only_argument() {
    let entries = graph_with("dev-libs/pg34-gone", |r| {
        r.update = true;
    });
    assert_eq!(
        entries,
        vec![(
            "dev-libs/pg34-gone".to_string(),
            PretendOutcome::AlreadyInstalled {
                version: "1".to_string()
            }
        ),]
    );
    let entries = graph_with("dev-libs/pg34-gone", |r| {
        r.update = true;
        r.deep = Deep::Unlimited;
    });
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/pg34-gextra".to_string(),
                PretendOutcome::New {
                    version: "1".to_string()
                }
            ),
            (
                "dev-libs/pg34-gone".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1".to_string()
                }
            ),
        ]
    );
}
