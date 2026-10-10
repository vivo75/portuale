use super::*;

/// The fixture tree is not in this repository: `fixtures/` is a
/// symlink to `../pmtest/fixtures`, the single copy, which the
/// contract suite reads too (`LLM/agent-context.md`, "Where the
/// tests live"). Every `#[cfg(test)]` reader in the workspace
/// resolves it through a compile-time `CARGO_MANIFEST_DIR/../..`
/// path, so a missing or half-checked-out sibling would otherwise
/// surface as a pile of unrelated file-not-found failures. This
/// test is the one that says why.
#[test]
fn fixtures_tree_is_the_pmtest_checkout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    assert!(
        root.is_dir(),
        "{} does not resolve -- `fixtures/` is a symlink to ../pmtest/fixtures: \
         check out the sibling pmtest repo next to this one",
        root.display()
    );
    // Markers, not a full inventory: enough to catch a tree that is
    // there but is not the fixture tree (a stale mount, an empty
    // clone, a symlink retargeted by hand).
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

/// #109 S2: the shared field set is real's own 23 names, listed
/// literally so a drift fails loudly, and the format version stays 1
/// (real's `_METADATA_FILE_FORMAT_VERSION`; changing either the set
/// or the version breaks interoperability with real's reader/writer
/// on a live vdb in both directions).
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

/// #109 S2: `in_metadata_file` is real's `_in_metadata_file()` --
/// membership in the 23-set, so the line-oriented `CONTENTS` and
/// `NEEDED.*` fields are rejected (they must never be answered from
/// the snapshot).
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
    // dev-libs/samepkg is installed at 1.0 and is the highest visible
    // version. Without --emptytree a *selective* resolve leaves it
    // AlreadyInstalled; --emptytree (real create_depgraph_params.py:
    // 176-179 pops "selective" and forces the merge) turns it into a
    // bare Reinstall -- every field false, exactly real portage's own
    // reasonless `[ebuild R]`.
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
    // dev-libs/upgradepkg is installed at 1.0, 2.0 is visible --
    // without `update`, real avoid_update's own shortcut never gets
    // a chance to fire for a not-selective top-level atom (the
    // installed version is never even a matched candidate to begin
    // with), so the ordinary "best across everything visible"
    // search proceeds and finds 2.0, exactly as if `update` were
    // true.
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
    // Same fixture as the reinstall test above, but selective=true
    // (via the pre-existing `resolve` helper) -- must still be
    // AlreadyInstalled, matching every test written before this
    // slice.
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
    // dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible
    // in the tree too. Real depgraph.py's own `avoid_update`
    // (lines 7814/8448) means plain `emerge dev-libs/upgradepkg`,
    // with no --update, never even looks for a better version --
    // see resolve_pretend's own doc comment. This was, before this
    // slice, portuale's own (inaccurate) default behavior for
    // `upgrade` below.
    assert_eq!(
        resolve("dev-libs", "upgradepkg"),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn upgrade() {
    // Same fixture as above, but with --update: now a real Upgrade,
    // matching real depgraph.py's own `dont_miss_updates` branch.
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
    // dev-libs/downgradepkg is installed at 2.0, but only 1.0 is
    // visible in the tree (2.0's own ebuild is gone) -- real
    // output.py's own in-slot `best()` check (see PretendOutcome::
    // Downgrade's own doc comment) flags this as a genuine downgrade,
    // not an "upgrade" to an older version. The installed version
    // (2.0) has no visible candidate of its own, so avoid_update's
    // shortcut doesn't apply here even without --update -- see
    // resolve_pretend's own doc comment on requiring a *visible*
    // candidate.
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
    // dev-libs/keywordmaskedpkg is installed at 2.0, whose own
    // KEYWORDS ("~amd64") are no longer accepted under the fixture
    // profile's own default ACCEPT_KEYWORDS ("amd64") -- only 1.0
    // (KEYWORDS="amd64") is currently visible. Reached as a
    // DEPENDENCY atom (is_top_level=false, via `resolve_as_
    // dependency`), real depgraph.py's own early `avoid_update`
    // return (`_select_pkg_highest_available_imp`, ~8440: `if
    // inst_pkg is not None and parent is not None and not self.
    // _want_update_pkg(...)`) requires NO visibility check at all --
    // confirmed live against a real system (`sys-libs/liburing`,
    // installed ~amd64-only, real emerge --pretend never even
    // considers downgrading it). Before this slice's own fix, this
    // portuale incorrectly required the installed version to also be
    // in the visible-filtered candidate set, so this fixture would
    // have (wrongly) resolved to `Downgrade { from: "2.0", to:
    // "1.0" }` instead.
    assert_eq!(
        resolve_as_dependency("dev-libs", "keywordmaskedpkg"),
        PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string()
        }
    );
}

#[test]
fn keyword_masked_but_installed_dependency_still_requires_visibility_as_a_top_level_atom() {
    // Same fixture, but reached as a TOP-LEVEL atom instead (real
    // depgraph.py's own SEPARATE, later `if avoid_update: ... self.
    // _pkg_visibility_check(...)` block, which DOES require
    // visibility) -- `resolve`'s own `is_top_level=true` exercises
    // this, unaffected by this slice's own fix, matching real
    // portage's real distinction between the two code paths.
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
    // #157: dev-libs/needskeywordmasked RDEPENDs on dev-libs/
    // keywordmaskedpkg, installed at 2.0 (`KEYWORDS="~amd64"`, not
    // accepted under the fixture profile's own default
    // `ACCEPT_KEYWORDS="amd64"`) while 1.0 (`KEYWORDS="amd64"`) is the
    // only visible version -- the installed `~arch`-masked version
    // beside a visible stable the filing names. Under `--emptytree`
    // real drops every installed candidate wholesale
    // (`depgraph.py:7888-7899`: `if empty and pkg.installed ...:
    // continue`), so its dependency `inst_pkg` return
    // (`depgraph.py:8453-8458`) never fires and the best visible
    // stable wins -> downgrade 2.0 -> 1.0. Before #157 the dependency
    // avoid_update shortcut kept the masked 2.0 and reinstalled it,
    // exactly the L3 `sys-apps/portage-3.0.82.2` reinstall / real
    // `3.0.81.3` downgrade shape.
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
    // Same fixture as above, but without --newuse: the USE mismatch
    // is real but the flag that would detect it is off, so this must
    // stay the pre-existing AlreadyInstalled outcome.
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
    // dev-libs/overlayonlypkg exists only in the fixture's overlay
    // repo (see fixtures/etc/portage/repos.conf), not the
    // main repo -- proving the overlay is actually searched, not
    // just present in repos.conf.
    assert_eq!(
        resolve_real("dev-libs", "overlayonlypkg"),
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn fixture_best_version_wins_regardless_of_which_repo_has_it() {
    // dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in
    // the overlay -- the higher version wins even though it isn't
    // in the main (lower-priority) repo.
    assert_eq!(
        resolve_real("dev-libs", "overlaynewerpkg"),
        PretendOutcome::New {
            version: "2.0".to_string()
        }
    );
}

#[test]
fn fixture_same_version_tie_across_repos_is_broken_toward_higher_priority() {
    // dev-libs/overlaytiepkg-1.0 exists identically-versioned in both
    // the main repo (priority -1000, no deps) and the overlay
    // (priority 10, RDEPENDs on dev-libs/newpkg): resolving it must
    // pull in newpkg, proving the overlay's own copy -- not the main
    // repo's -- is the one whose metadata got read.
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
    // dev-libs/slotconflictparent pulls in slotconflictnewconsumer
    // (bare RDEPEND on slotconflicttarget) and slotconflictoldconsumer
    // (RDEPEND "<dev-libs/slotconflicttarget-2.0"). Backlog #90 (S1):
    // the walk resolves last-declared-first (real's stack), so the
    // old consumer's `<2.0` graphs 1.0 before the bare dep is even
    // visited -- and the bare dep reuses the already-graphed 1.0
    // (real `_select_package`'s existing-node return), so no slot
    // conflict ever forms, at any backtrack budget. The backtracking
    // retry path this test used to exercise (2.0 graphed first, then
    // reconciled to 1.0) no longer triggers here; the unsolvable twin
    // below still covers the recorded-conflict shape. Real
    // `--debug` never prints `Slot conflict handler started.` for
    // this fixture (S0 matrix oracle).
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
    // Backlog #209: `--backtrack=0` must not skip the
    // reverse-dependency feed loop. Real `_resolve_conflicts`
    // (`_emerge/depgraph.py:9444`) calls `_complete_graph()`
    // (`:8562`) with no `_allow_backtracking` gate, so a satisfiable
    // installed-consumer pin enforces in-pass even with backtracking
    // off. Hermetic r25 shape (#25 S0): installed world consumer
    // `r25consumer` pins `<r25lib-2.0:=` (both bound forms), and
    // `-uD r25target` must withhold `r25lib-2.0` silently --
    // `backtrack_restarts` 0 (real `backtrack: 0/20`) and no
    // skipped-update rows (real merges `r25up-2.0 + r25target-1.0`
    // with no warning).
    //
    // The fixture world file does not carry the consumer (it would
    // perturb every other fixture-ROOT test), so the seed is set
    // directly -- the `FX_WORLD_EXTRA=dev-libs/r25consumer`
    // equivalent the bed cell stages.
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
    // Real's silence: no slot-operator withhold rows at bt0 (the
    // minimizer/probe path writes no missed-update state).
    assert_eq!(result.skipped_updates, vec![]);
    assert_eq!(result.backtrack_restarts, 0);
    assert_eq!(result.slot_conflicts, vec![]);
    // The rest of real's merge set is intact.
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
    // Walk-level S0 matrix cells (old,new): at `--backtrack=0` and
    // at the default budget alike, no `SlotConflict` survives (the
    // direct solve removes 2.0 in-pass, so the retry has nothing to
    // do), the merge list settles on 1.0, and exactly one skip row
    // names oldconsumer's pin -- real's rows + `WARNING`, rc 0
    // (staged oracle both budgets).
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
    // dev-libs/btparent -> btconsumer (resolves -2.0, RDEPEND
    // >=bttarget-2.0) + btpin (RDEPEND <bttarget-2.0). No bttarget
    // version satisfies both, so slice 1's solvability check fails.
    // C3's ranked choices mask bttarget-2.0 on one node; the retry
    // finds btconsumer-2.0's dep unsatisfiable (mask-aware
    // missing-dep probe) and masks its parent, falling back to
    // btconsumer-1.0 + bttarget-1.0 with every constraint met --
    // the same downgrade the old puller pre-masking produced, via
    // real's feedback paths instead.
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
    // dev-libs/slotconflictunsolvable: both pullers of
    // slotconflicttarget:0 have a single version, so masking
    // slotconflicttarget-2.0 only turns the >=2.0 dependency into a
    // NoVisibleCandidate -- strictly worse. Slice 3 must revert the
    // trial and report the original slot conflict unchanged
    // (Backlog #90 S1: instances now file oldpin's 1.0 first --
    // real's own notice order on the staged oracle).
    let result = graph_result_real("dev-libs/slotconflictunsolvable");
    assert_one_conflict(
        &result,
        "dev-libs",
        "slotconflicttarget",
        "0",
        "1.0",
        ">=dev-libs/slotconflicttarget-2.0",
    );
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
    // Slice 4: the SlotConflict names every conflicting version of
    // slotconflicttarget:0 and, per version, the (parent cpv, atom)
    // that pulled it -- real slot_collision_handler's per-slot_atom
    // (pkg, parent_atoms).
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
    // slotconflicttarget and both pins declare no IUSE -> every
    // instance/parent renders `USE="" ELIBC="glibc"` (real
    // `pkg_use_display`: the bare USE group plus the implicit
    // USE_EXPAND group -- backlog #264, grounded in real
    // `/usr/sbin/emerge` 3.0.82.2 `--pretend --ignore-default-opts
    // dev-libs/slotconflictunsolvable` on the staged fixture).
    assert!(c.instances.iter().all(|i| i.use_display
        == vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ]));
}

#[test]
fn fixture_slot_conflict_carries_pkg_use_display() {
    // dev-libs/scuseparent -> scusenewpin (>=scusetarget-2.0) +
    // scuseoldpin (<scusetarget-2.0): unsolvable slot conflict on
    // scusetarget:0. scusetarget IUSE="+scuon scuoff" -> both
    // instances render `USE="scuon -scuoff" ELIBC="glibc"` (real
    // pkg_use_display, enabled-first, plus the implicit USE_EXPAND
    // group -- backlog #264, grounded in real `/usr/sbin/emerge`
    // 3.0.82.2 `--pretend --ignore-default-opts dev-libs/scuseparent`
    // on the staged fixture). scusenewpin IUSE="+scupin" -> that
    // parent line renders `USE="scupin" ELIBC="glibc"`; scuseoldpin
    // has no IUSE -> `USE="" ELIBC="glibc"`.
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
    // Backlog #90 (S1): instance order follows the walk -- the
    // oldpin's `<2.0` resolves first (reversed declaration order,
    // real's stack discipline), so 1.0 sorts first, exactly real's
    // own notice order (staged oracle: 1.0 pulled by oldpin, then
    // 2.0 by newpin).
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
    // dev-libs/slotconflictunsolvable pulls in slotconflictnewpin
    // (RDEPEND ">=dev-libs/slotconflicttarget-2.0") and
    // slotconflictoldpin (RDEPEND "<dev-libs/slotconflicttarget-
    // 2.0"). No single version of slotconflicttarget satisfies both,
    // so the backtracking solvability pre-check fails, no retry is
    // attempted, and the SlotConflict is reported exactly as before
    // (Backlog #90 S1: oldpin's instance files first -- real's
    // notice order -- since the walk resolves it first).
    let result = graph_result_real("dev-libs/slotconflictunsolvable");
    assert_one_conflict(
        &result,
        "dev-libs",
        "slotconflicttarget",
        "0",
        "1.0",
        ">=dev-libs/slotconflicttarget-2.0",
    );
}

#[test]
fn fixture_different_slots_of_the_same_package_coexist_without_conflict() {
    // dev-libs/multislotparent RDEPENDs on both
    // dev-libs/multislotpkg:0 and dev-libs/multislotpkg:1 -- real,
    // different slots of the same package are normal coexistence
    // (like dev-lang/python:3.11 and :3.12), not a conflict: both
    // must appear as independent entries, and neither is silently
    // dropped by the visited-set the way v1 always did before slot
    // tracking existed.
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
    // dev-libs/newslotpkg-1.0 (SLOT 0) is installed; -2.0 (SLOT 1) is
    // not. Requesting `:1` matches only -2.0, whose own slot has
    // nothing installed -- real portage's `_get_installed_best`
    // (`not myinslotlist` while `vardb.match(cp)` is non-empty) makes
    // this a `New` with `new_slot=True` (`[ebuild NS]`), NOT an
    // `Upgrade` from 1.0. Before installed matching was slot-aware
    // portuale mis-reported it as `Upgrade { from: "1.0", to:
    // "2.0" }` (both this crate and the Python oracle agreed, and
    // both were wrong).
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
    // Requesting the slot that IS installed (`:0`, holding
    // newslotpkg-1.0) stays an in-slot outcome -- selective (this
    // helper's mode) keeps the installed version untouched, and
    // `new_slot` is never set for anything but a `New`.
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
    // dev-libs/interactivemergepkg has an unconditional
    // PROPERTIES=interactive and is merge-bound -> real
    // output.py:833's `attr_display.interactive`.
    let entries = graph_entries_real("dev-libs/interactivemergepkg");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].interactive);
    assert!(matches!(entries[0].outcome, PretendOutcome::New { .. }));

    // dev-libs/interactivecondpkg's `interactive` token sits behind a
    // `gtk? ( ... )` conditional with `gtk` disabled -> the real
    // USE-conditional evaluation (`_PackageMetadataWrapper`, `"?" in
    // v`) gates it out.
    let entries = graph_entries_real("dev-libs/interactivecondpkg");
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].interactive);
    // (The merge-bound `Reinstall` case -- an already-installed
    // interactive package under non-selective resolution rendering
    // `[ebuild Ir]` -- is covered end to end by the contract suite;
    // `graph_entries_real` is selective, so a bare installed atom
    // resolves to `AlreadyInstalled` here, which carries no bracket.)
}

#[test]
fn fixture_fetch_restrict_column_tracks_distdir_state() {
    // Both fixtures are `RESTRICT="fetch"`. The test helpers pass
    // `fixtures_root().join("distfiles")` as `distdir`, which holds
    // `frs-1.0.tar.gz` at the size its Manifest records but not
    // `frm-1.0.tar.gz` -- real `output.py:633-641`'s `fetch_restrict`
    // / `not getfetchsizes(only_restricted=True)`.
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
    // virtual/texteditor is shaped exactly like a real virtual (e.g.
    // virtual/pager in the real Gentoo tree, confirmed by
    // inspection): an ordinary ebuild whose RDEPEND is a
    // "|| ( dev-libs/newpkg dev-libs/samepkg )" any-of group of real
    // providers -- no PROVIDE mechanism, no dedicated virtuals
    // resolution code anywhere in portuale. Real `dep_zapdeps`
    // preference (see `use_reduce_flat_disjunctive`'s `AltPreference`,
    // portage-use-reduce): dev-libs/samepkg is already installed
    // (choice bin 0, `preferred_installed`) so it wins over the
    // first-listed dev-libs/newpkg (bin 1, needs a merge) -- exactly
    // how real portage resolves `virtual/wine` -> the installed
    // `wine-staging` instead of the first-listed `wine-vanilla`.
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
    // keyword -> '*' (real get_keyword_mask "missing").
    assert_eq!(
        graph_entries_real("dev-libs/tildestarkeywordpkg")[0].keyword_mask,
        Some('*')
    );
    // package.mask'd then package.unmask'd -> '#' (real isHardMasked
    // ignores package.unmask), and it wins over any keyword state.
    assert_eq!(
        graph_entries_real("dev-libs/maskedandunmaskedpkg")[0].keyword_mask,
        Some('#')
    );
}

#[test]
fn fixture_package_mask_minus_atom_removal_is_unaffected() {
    // dev-libs/samepkg is masked then immediately un-masked via
    // "-dev-libs/samepkg" within package.mask itself -- it must
    // resolve completely normally (already installed), not as if it
    // were ever masked.
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
    // Regression guard: the "*/wildcardkeywordpkg" package.accept_keywords
    // entry is scoped to that package name only (not "dev-libs/*"),
    // specifically so it can't accidentally make dev-libs/maskedpkg
    // visible too.
    assert_eq!(
        resolve_real("dev-libs", "maskedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_eula_style_license_is_masked_by_the_portage_default_accept_license() {
    // Neither the fixture profile chain nor make.conf sets
    // ACCEPT_LICENSE at all -- real portage's own "* -@EULA"
    // default applies, and profiles/license_groups defines
    // EULA="SomeEula", so dev-libs/eulapkg's own LICENSE="SomeEula"
    // is masked.
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
    // fixtures/etc/portage/package.license accepts SomeEula for this
    // one package specifically, despite the same global "* -@EULA"
    // default that masks dev-libs/eulapkg above.
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
    // fixtures/etc/portage/package.use forces "nonfreeflag" on for
    // this specific package, activating its own LICENSE's
    // "nonfreeflag? ( SomeEula )" conditional -- masked by the same
    // real "* -@EULA" default as dev-libs/eulapkg.
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
    // fixtures/etc/portage/package.properties revokes "interactive"
    // for dev-libs/interactivepkg specifically, despite the real
    // default ACCEPT_PROPERTIES=* that leaves dev-libs/propertiespkg
    // (above) visible.
    assert_eq!(
        resolve_real("dev-libs", "interactivepkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

#[test]
fn fixture_package_accept_restrict_narrows_acceptance_for_one_package() {
    // fixtures/etc/portage/package.accept_restrict revokes "bindist"
    // for dev-libs/restrictedpkg specifically, same "-token narrows
    // despite a permissive global default" mechanism as
    // package.properties above.
    assert_eq!(
        resolve_real("dev-libs", "restrictedpkg"),
        PretendOutcome::NoVisibleCandidate
    );
}

/// Regression test: a sibling package sharing a name prefix
/// ("foo-bar" installed) must not be misread as an installed version
/// of "foo" when scanning the vdb category directory. Without the
/// digit-start guard in strip_version_prefix, this would wrongly
/// report an Upgrade from a bogus "bar-2.0" pseudo-version instead of
/// a clean New.
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

/// Backlog #194: `--onlydeps-with-rdeps` / `--onlydeps-with-ideps`
/// blank an onlydeps root's dep classes exactly like real
/// `depgraph.py:4186-4193` (sets asserted against the
/// `/tmp/opencode/n194/oracle_odw.log` playground oracle; the root
/// itself is always resolved -- only the display layer suppresses
/// it -- so it stays in the entries here).
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

/// Backlog #194: an EAPI-7 root's `IDEPEND` never reaches the walk
/// (real `bin/ebuild.sh` unsets it in the depend phase), so even the
/// default rdeps-on shape merges no `E`.
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

/// #74 S1: an installed `||` alternative's `[use]` deps are matched
/// against its recorded vdb USE, not the tree candidate's
/// profile-derived USE (real `dep_check.dep_zapdeps`'s
/// `_dep_check_composite_db` covers the vardb, and `dbapi._match_use`
/// has a separate built-package branch that reads `metadata["USE"]`).
/// `fixtures/repo/dev-libs/altconsumer-1.0` BDEPENDs
/// `|| ( dev-libs/altprov:1.1[flip] dev-libs/altprov:1.0 )`;
/// `fixtures/var/db/pkg/dev-libs/altprov-1.1` is installed with
/// `USE=flip` while the fixture profile/environment never enables
/// `flip`. Before the fix the installed alternative ranked
/// `UnsatUseInstalled` and the uninstalled slot's cp-level
/// `Installed` won, so portuale merged `dev-libs/altprov-1.0`; real's
/// own container capture (findings "#74 S0") keeps the installed
/// `:1.1` (bin 0 plus the `all_installed_slots` promotion).
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
    // dev-libs/upgradepkg is reached only as a dependency of
    // dev-libs/withdeps here, never a top-level atom -- --exclude
    // must still leave it alone despite --update, proving the flag
    // threads uniformly through the whole BFS (see
    // resolve_pretend_graph's own doc comment), not just a
    // top-level atom.
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
fn rebuild_if_unbuilt_reinstalls_a_consumer_of_a_merged_build_dep() {
    // dev-libs/rebuildconsumer (installed) has a vdb DEPEND on
    // dev-libs/rebuildtrigger; `emerge -u rebuildtrigger` upgrades it
    // 1.0 -> 2.0. --rebuild-if-unbuilt then schedules rebuildconsumer
    // for a rebuild (it isn't otherwise in the graph at all).
    let g = graph_rebuild_if("dev-libs/rebuildtrigger", true, false, false, &[], &[]);
    assert!(
        g.iter().any(|(n, o)| n == "dev-libs/rebuildconsumer"
            && matches!(
                o,
                PretendOutcome::Reinstall {
                    slot_operator_rebuild: false,
                    ..
                }
            )),
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
    let ex = graph_rebuild_if(
        "dev-libs/rebuildtrigger",
        true,
        false,
        false,
        &["dev-libs/rebuildconsumer"],
        &[],
    );
    assert!(!ex.iter().any(|(n, _)| n == "dev-libs/rebuildconsumer"));
    let ig = graph_rebuild_if(
        "dev-libs/rebuildtrigger",
        true,
        false,
        false,
        &[],
        &["dev-libs/rebuildtrigger"],
    );
    assert!(!ig.iter().any(|(n, _)| n == "dev-libs/rebuildconsumer"));
}

#[test]
fn dynamic_deps_picks_ebuild_vs_vdb_deps_for_an_already_installed_deep_walk() {
    // changeddepspkg (installed) has vdb RDEPEND=dev-libs/samepkg but
    // its current ebuild RDEPEND=dev-libs/newpkg. Kept from being a
    // Reinstall via --reinstall-atoms's opposite... just drive it as
    // AlreadyInstalled by NOT requesting a reinstall: resolve_pretend
    // in test_config keeps a directly-named installed atom as
    // AlreadyInstalled (selective=true here), so --deep recurses via
    // enqueue_dependencies.
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
    // dev-libs/builtbindpkg is installed with vdb
    // RDEPEND="dev-libs/builtbindtarget:0/1=" while its current ebuild
    // RDEPEND is "dev-libs/newpkg" (the binding was dropped from the
    // tree). Its vdb carries no `EAPI` file, so real reads the
    // installed EAPI as `"0"` and the overlay never applies here
    // (backlog #25 S1b): the default walks the live ebuild alone;
    // `=n` walks the vdb snapshot alone;
    // `--ignore-built-slot-operator-deps` changes nothing on this
    // shape. (The EAPI-bearing r25 shape walks both; the ungated
    // append broke both #24 pins, Phase 5 S1 S0.)
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
    // dev-libs/deepvdbusepkg is installed with vdb USE="" (no
    // `wantdep`); its IUSE is `wantdep` and fixtures/repo/profiles/
    // base/package.use enables it in the *effective* set. Its current
    // ebuild RDEPENDs `wantdep? ( dev-libs/deepvdbusetarget )`.
    // Reached only via the --deep recursion under
    // dev-libs/deepvdbuseconsumer (New), that conditional must be
    // evaluated against the recorded vdb USE (real `_pkg_use_enabled`
    // for a built package) -> deepvdbusetarget NOT pulled.
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
    // #139 (#132's missing Rust-side test): the `deepusedep*` arms
    // pin `enqueue_dependencies`' evaluation rule at the graph level.
    // Each installed parent carries `~child-1.0[flip=]`; the token is
    // evaluated against the parent's recorded vdb USE (real
    // `_pkg_use_enabled` for a built package) before candidate
    // selection, so the conditional form never arrives unevaluated
    // (where it imposes no state constraint at all).
    //
    // Arm A (parent built -flip, child installed +flip and
    // profile-pinned +flip): `[-flip]` is unsatisfiable -- the child
    // is present as an entry but never as a satisfied install, so the
    // abort path (pinned at the CLI by the #132 contract test) has
    // something to report.
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
    // Arm B (same parent shape, child not profile-pinned): `[-flip]`
    // reaches candidate *selection* -- the child rebuilds with flip
    // off rather than aborting. A fix that only hardened the failure
    // display would pass arm A and fail here.
    let arm_b = resolve_deep_consumer("dev-libs/deepusedepbconsumer", false);
    assert!(
        arm_b
            .iter()
            .any(|(n, o, _)| n == "dev-libs/deepusedepbchild" && o == "Reinstall"),
        "evaluated [-flip] must rebuild the installed +flip child with \
         flip off: {arm_b:?}"
    );
    //
    // Arm C (control -- parent built +flip): the same atom evaluates
    // to `[flip]`, which the installed +flip child satisfies, so
    // nothing merges. (The graph still records the walked installed
    // nodes as `AlreadyInstalled` entries -- unlike the CLI merge
    // list -- so the assertion is "no merging entry", not
    // single-entry.) Guards against an over-broad fix.
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
    // #133 at the graph level (the contract pin is
    // `test_root_deps_evaluates_conditional_use_deps_before_the_satisfied_check`):
    // `dev-libs/deeprootdepparent` is installed with vdb USE=""
    // (built -flip) and BDEPENDs `~dev-libs/deeprootdepchild-1.0[flip=]`;
    // the running root has the child installed +flip. With `--root-deps`
    // the producers must evaluate the atom to `[-flip]` *and* enforce it
    // against the running-root instance -- otherwise the raw form is
    // vacuously satisfied and the child vanishes (bed
    // `l0-fx-20260922T082944Z`: real rebuilds it with flip off).
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
    // Control without `--root-deps`: the producers are a strict no-op
    // (`root_deps_running_root` None) and the ordinary S5-evaluated
    // walk rebuilds the child against the target root instead.
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

/// #141 (Phase 5b S1): a `--root-deps` running-root build entry's
/// `use_flags_display` must carry the rebuild's flipped flag, so the
/// renderer prints real's `USE="-flip*"` column (bed
/// `l0-fx-20260922T192350Z`) instead of skipping the display. The
/// contract pin is
/// `test_root_deps_evaluates_conditional_use_deps_before_the_satisfied_check`;
/// this is the `cargo test` gate over the same constructor.
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
    // Installed +flip (vdb USE="flip"), rebuilt with flip off -- the
    // `use_flags_display` pair the renderer's `-flip*` comes from.
    assert_eq!(
        child.use_flags_display,
        vec![("flip".to_string(), false)],
        "the rebuild's flipped flag must reach the display (#141)"
    );
    assert_eq!(child.slot.as_deref(), Some("0"));
}

#[test]
fn emptytree_reinstalls_the_whole_deep_dependency_tree() {
    // Without --deep/--emptytree, `graph("dev-libs/deeppkg")` on an
    // installed deeppkg shows only itself (AlreadyInstalled, or a
    // top-level bare Reinstall) and never walks deeppkg2/newpkg.
    // --emptytree forces `deep` on AND turns each installed dep into
    // a Reinstall, so the whole chain shows: deeppkg + deeppkg2 as
    // Reinstall (both installed), newpkg as New (not installed).
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
    // withbdepspkg is already installed, RDEPENDs on newpkg, DEPENDs
    // on builddeponlypkg, BDEPENDs on hostdeponlypkg -- with_bdeps
    // defaulting to true (real --with-bdeps=auto/y, portuale's own
    // --usepkg-less default) walks all three under --deep, same as
    // before --with-bdeps existed.
    // Merge order follows real's own discovery order (RDEPEND,
    // IDEPEND, PDEPEND, DEPEND, BDEPEND -- `_add_pkg_dep_string`'s
    // `deps` tuple, `depgraph.py:4253-4289`), via
    // `portage_discovery_order`: RDEPEND's newpkg is discovered before
    // DEPEND's builddeponlypkg, which is discovered before BDEPEND's
    // hostdeponlypkg.
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
    // Real depgraph.py's own "if pkg.built and not removal_action":
    // --with-bdeps=n only ever drops DEPEND/BDEPEND for an
    // already-built (here: AlreadyInstalled) package -- RDEPEND is
    // never affected, so newpkg (RDEPEND) still shows up while
    // builddeponlypkg (DEPEND) and hostdeponlypkg (BDEPEND) don't.
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

/// Real "recursively pull in and build a new package against the
/// running root" (see `resolve_root_deps_build_entries`'s own doc
/// comment): `rootdepsbuildpkg`'s own `BDEPEND` (`dev-libs/
/// rootdepsbuildtool`) has a real, tree-visible ebuild, but no vdb
/// entry anywhere -- so under `--root-deps`, it's neither dropped as
/// already-satisfied (unlike `rootdepsprovider` above) nor reported
/// as `NoVisibleCandidate` (unlike the same atom would be *without*
/// `--root-deps`, since it's genuinely visible in the tree): it gets
/// its own real `New` entry, `targets_running_root: true`.
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

/// Recursive successor to the test above (see
/// `resolve_root_deps_build_entries`'s own doc comment): `rdrapp`
/// BDEPENDs `rdrtool`, which itself BDEPENDs `rdrtooldep` and
/// RDEPENDs `rdrlib` -- so all four resolve, the three build entries
/// carry `targets_running_root: true`, each names its *immediate*
/// requester in `required_by`, and `rdrlib` proves an entry's own
/// RDEPEND is walked against the running root too.
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

/// `IDEPEND` of a running-root build entry is walked against the
/// running root too (real portage: `IDEPEND` *always* targets
/// `_running_root.root`, `depgraph.py:4247-4252`). `rdriapp`
/// BDEPENDs `rdritool`, whose own `IDEPEND` (`rdrilib`) must be
/// pulled in as its own `targets_running_root` entry.
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

/// A *top-level* package's own `IDEPEND` resolves against the running
/// root too, not just recursed running-root build entries (real
/// `depgraph.py:4247-4252`: `IDEPEND` always targets
/// `_running_root.root` for every package). `topidepapp` IDEPENDs
/// `topideplib`; under `--root-deps` `topideplib` becomes a
/// `targets_running_root` entry rather than an ordinary ROOT entry.
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

/// Track X Slice C (#242): the g216 shape under default staging --
/// `g216top` RDEPENDs `g216mid` RDEPENDs `g216comp` BDEPENDs
/// `|| ( >=dev-lang/g216comp-1.0 dev-lang/g216boot )`. Real's pass 1
/// keeps the in-graph self branch live and dead-ends on the
/// running-root self cycle (`_serialize_tasks`, `depgraph.py:10262`),
/// then re-resolves with the `circular_dependency` map to `g216boot`
/// (`backtrack: 1/20`, rc 0). The merge list spans both roots: the
/// running-root build dep merges before its target-root consumer, and
/// `g216comp` lands TWICE (one row per root, real `output.py`
/// `get_display_list`).
/// (`LLM/evidence/2026-09-29-242-inventory/probes/default-g216top.txt`).
#[test]
fn cross_root_bdepend_self_branch_merges_in_both_roots() {
    let root = fixtures_root();
    // Hermetic running root: nothing installed there, so every build
    // dep selects from the tree.
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
    // Real spends its single backtrack retry on the serialize abort.
    assert!(result.backtrack_restarts >= 1);
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    // Merge order runs across roots: the running-root build dep
    // merges before its target-root consumer, and `g216comp` lands
    // twice -- real's `default-g216top.txt` rows.
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

/// The cycle guard: `rdrcyca` BDEPENDs `rdrcycb` BDEPENDs `rdrcyca`.
/// The shared `root_deps_build_seen` set (inserted *before* a
/// package's own deps are walked) must cut the recursion so each
/// cycle node appears exactly once, not overflow the stack.
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

/// Per the confirmed judgment call for this slice: an unbuildable,
/// not-installed dep of a running-root build entry (`rdrmisstool`
/// BDEPENDs `rdrnothere`, which has no ebuild anywhere) is now
/// surfaced as its own `NoVisibleCandidate` entry -- previously
/// `--root-deps` silently swallowed it.
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
    // --nodeps trumps --deep -- real create_depgraph_params.py pops
    // "recurse" out of myparams outright, which the dependency walk
    // itself checks for before `deep` is ever consulted.
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
    // dev-libs/upgradepkg is installed at 1.0 with a newer 2.0
    // visible in the tree too -- without --update, it stays
    // AlreadyInstalled, same as resolve_pretend's own
    // without_update_an_installed_version_that_still_satisfies_the_atom_is_kept
    // test above, just reached here as a dependency instead of a
    // top-level atom.
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
    // dev-libs/needskeywordmasked (New) RDEPENDs on dev-libs/
    // keywordmaskedpkg, installed at 2.0 (KEYWORDS="~amd64", not
    // accepted under the fixture profile's own default
    // ACCEPT_KEYWORDS="amd64") -- reached only as a dependency,
    // never a top-level atom, so this exercises the exact real
    // resolve_pretend_graph BFS path a real `sys-fs/fuse`'s own
    // `sys-libs/liburing` dependency takes, confirmed live against
    // a real system (see this slice's own resolve_pretend-level
    // keyword_masked_but_installed_dependency_is_kept_not_downgraded
    // test for the full citation). Before this slice's own fix,
    // this would have (wrongly) shown a Downgrade entry to 1.0.
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
    // dev-libs/needskeywordmaskeduse (New) RDEPENDs on
    // dev-libs/keywordmaskedusepkg[flag] -- a real USE-dep on top of
    // the same keyword-masked-but-installed situation as the test
    // above. Installed at 2.0 with real vdb USE="flag" (see the
    // fixture's own IUSE/USE files) -- checked against that real
    // vdb-recorded USE, not the current tree's, via
    // `use_deps_satisfied` -- so this is kept exactly as installed,
    // matching real `sys-fs/fuse`'s own real
    // `sys-libs/liburing:=[abi_x86_64(-)?,...]` dependency (the
    // actual real-world case this slice's own fix was built for,
    // confirmed live: without checking USE-deps against vdb data,
    // an earlier version of this fix would have (wrongly) fallen
    // back to the old, visibility-gated lookup for ANY USE-decorated
    // atom, missing this exact case entirely).
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
    // dev-libs/needsbuiltusediverge (New) RDEPENDs on
    // dev-libs/builtusedivergedep[divergedflag]. The installed 1.0
    // has vdb USE="divergedflag" but vdb IUSE="" -- and the *current*
    // ebuild has dropped `divergedflag` from its IUSE entirely, so
    // the tree candidate can't satisfy the `[divergedflag]` USE-dep
    // at all (top-level `dev-libs/builtusedivergedep[divergedflag]`
    // resolves to NoVisibleCandidate). Real
    // `dbapi._iuse_implicit_built` (bug #640318): a built package's
    // own USE is authoritative for what counts as a valid IUSE flag,
    // so the installed version satisfies the atom and the dependency
    // is kept exactly as installed -- no spurious NoVisibleCandidate.
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
fn recursion_basic_chain_with_update_upgrades_the_dependency() {
    // Same fixture as recursion_basic_chain above, but with --update:
    // dev-libs/upgradepkg -- reached only as a *dependency* of
    // withdeps, never a top-level atom -- still upgrades, proving
    // `update` threads uniformly through the whole BFS (see
    // resolve_pretend_graph's own doc comment), not just top-level
    // atoms.
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
                PretendOutcome::Upgrade {
                    from: "1.0".to_string(),
                    to: "2.0".to_string(),
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
fn nodeps_resolves_the_top_level_atom_but_never_recurses() {
    // Same dev-libs/withdeps fixture as recursion_basic_chain above --
    // with --nodeps, only the top-level atom itself is ever resolved;
    // its own DEPEND/RDEPEND (which would otherwise pull in newpkg
    // and upgradepkg) is never even read.
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
    // Real portage's own -v USE display is about a package's own
    // metadata, unrelated to whether its dependencies get walked --
    // --nodeps must not blank it out. dev-libs/useflagpkg's own
    // foo?-gated RDEPEND on dev-libs/newpkg proves the *dependency*
    // walk really is skipped (no second entry), while its own
    // IUSE="foo missingflag" still shows up correctly.
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
    // ...but CPU_FLAGS_X86 is USE_EXPAND_HIDDEN, so it never reaches
    // the `-pv` display.
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
    // `topological_merge_order`: real portage's `mylist` is a genuine
    // topological merge schedule; portuale's BFS builds entries
    // parent-first, so `resolve_pretend_graph` re-sorts them into
    // dependency-first order before returning. `dev-libs/withdeps`
    // RDEPENDs `dev-libs/newpkg` + `dev-libs/upgradepkg`; both deps
    // must precede `withdeps`, and among the two unconstrained deps
    // their BFS-discovery (RDEPEND string) order is preserved.
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

    // Diamond: the shared leaf precedes both of its consumers, which
    // both precede the root -- a strict topological order.
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
    // dev-libs/withdeps RDEPENDs on newpkg and upgradepkg -- both
    // should list withdeps as their only owner.
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
    // dev-libs/common is reached via both shared-a and shared-b --
    // both owners must be recorded (sorted), not just whichever one
    // the BFS happened to resolve it through first.
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
    // dev-libs/multislotparent RDEPENDs on both dev-libs/multislotpkg:0
    // and :1 -- two separate entries sharing one (category, package).
    // Both were pulled in by multislotparent, so both must carry it in
    // `required_by`. The merge post-pass previously used a destructive
    // `required_by_map.remove(...)`, handing the owner to whichever
    // slot's entry came first and leaving the other with `[]` (which
    // `--tree` then dropped to its flush-left safety net).
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
    // `build_digraph` resolves each dep atom to the entries it
    // actually matches before adding an edge. `slotorderroot` RDEPENDs
    // `dev-libs/slotorderdual:2`; `:1` is pulled only through
    // `slotorderb`. Before per-atom edge matching, the slot-blind
    // `cat/pkg` lookup gave `slotorderroot` a phantom edge to `:1`
    // too (and the `required_by` fallback did the same in reverse),
    // so both `slotorderdual` slots reached parent-count 2 and
    // `_merge_order_bias` promoted them ahead of the plain leaf
    // `slotordera` (parent-count 1, declared first).
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
    // Backlog #193, upstream `test_virtual_cycle.py` (bug 965570) as
    // emitted by pmtest `89d17e0`: both cells fail in real
    // (`ResolverPlayground` success=False,
    // virtual_cycle={gzip-1} / {A-1,B-1,C-1}; g193 container probe
    // `emerge -p --color=n app-misc/{bar,foo}` on the staged tree:
    // rc 1 with the `!!! virtual cycle detected:` block). Portuale
    // merged both as ordinary rings (rc 0).
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
    // Backlog #153: the fucyclea/fucycleb USE-gated cycle is still in
    // the tree, but the top-level target dev-libs/fucyclec constrains
    // the solution flag conditionally (`dev-libs/fucyclea[x?]`) with
    // `x` in no IUSE of its own -- real masks the whole candidate
    // `invalid` at construction (`Package._validate_deps`), so the
    // resolve fails with the all-masked report before any cycle is
    // ever walked: there is no circular-dep suggestion to keep. (This
    // test previously pinned the followup suggestion; that output
    // described a merge real never makes.)
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
    // dev-libs/cyc4a through dev-libs/cyc4d form a four-ring of
    // build-time deps (the cyc4a edge gated behind USE=x, on by
    // default): real `get_cycles` records one ring per node, so four
    // records trip `large_cycle_count`, and the stuck remainder
    // re-displays all four members in leaf-drain order.
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
    // Control: the two-ring reports its error block but neither the
    // trailer (two records) nor ... -- well, the re-display still
    // shows (two members); only `large_cycle_count` stays false.
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
    // End to end through the driver: the maskneedpkg resolve records
    // one masked_deps report (atom + masked list) whose chain walks
    // the merge parent up to the top-level argument, like real's
    // `(dependency required by …)` lines.
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
fn mia0a_cells_report_missing_iuse_for_the_conditional_dep() {
    // End to end through the driver for #265(a): both `mia0a` cells
    // record one `Missing IUSE: foo` disclosure on `mia0b` whose chain
    // walks the merge parent up to the top-level argument, like
    // real's `(dependency required by …)` lines in
    // `LLM/evidence/2026-09-30-265/real/mia0a-{1,2}.txt`.
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
    // End to end through the driver: the unsatuseiuse resolve records
    // one Missing IUSE disclosure whose chain walks the merge parent
    // up to the top-level argument, like real's `(dependency required
    // by …)` lines.
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
    // #54: dev-libs/keeper-1.0 (installed, outside every target
    // closure, and outside @world/@selected/@system in the checked-in
    // fixture) records `=dev-libs/paired-1.0` -- but `graph_result_real`
    // resolves through a single non-complete-mode pass (no
    // `complete_seed_atoms`, matching real never entering complete
    // mode here at all: hermetic real 3.0.82.2 never walks to keeper),
    // so the explicit `=dev-libs/paired-2.0` request merges cleanly
    // with no residual conflict (verified live,
    // `TEST/findings/l0-fixture-oracle.md` "#54 S0" hermetic row).
    // Before the #54 fix this asserted the opposite: a residual
    // conflict pairing the merge against installed 1.0 with keeper as
    // parent -- portuale's unconditional (unreachable-consumers
    // included) vdb reverse scan applying a pin real never sees.
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
    // instcyclea BDEPENDs instcycleb BDEPENDs instcyclea, both
    // already installed. Real `DepPriority.satisfied` demotes a
    // build-time edge whose dep an installed package provides, so
    // `_serialize_tasks` can break the cycle. Under --emptytree both
    // are `[ebuild R]`; `circular_deps` must stay empty (contrast
    // `hardcyclea`/`hardcycleb`, neither installed -> reported).
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
    // ingraphsvc RDEPENDs `|| ( dev-libs/ingraphkeyring
    // dev-libs/ingraphwallet[keyring] )`. Neither branch is installed.
    // ingraphany pulls ingraphwallet[keyring] directly *and*
    // ingraphsvc -- so when ingraphsvc's `||` is resolved,
    // ingraphwallet is already a graph node. Real `dep_zapdeps`
    // `all_in_graph` (choice bin 0) picks it over the first-listed
    // ingraphkeyring, so ingraphkeyring + its own dep never enter the
    // graph.
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
    // dev-libs/anyof's own RDEPEND is
    // "|| ( dev-libs/newpkg dev-libs/samepkg )". dev-libs/newpkg
    // (listed first) would need a merge; dev-libs/samepkg (second) is
    // already installed. Real `dep_zapdeps` files samepkg in choice
    // bin 0 (`preferred_installed`, aliased to `preferred_in_graph`)
    // and newpkg in bin 1 (`preferred_non_installed`), and takes the
    // first entry of the best non-empty bin -- so `samepkg` wins even
    // though it's listed second, and `newpkg` is never merged. See
    // `use_reduce_flat_disjunctive`'s `AltPreference` (portage-use-
    // reduce) and `atom_cp_installed`.
    let entries = graph("dev-libs/anyof");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["dev-libs/samepkg", "dev-libs/anyof"]);
}

#[test]
fn recursion_survives_an_unresolvable_dependency() {
    // The top-level package still resolves, and the unresolvable
    // dependency still shows up in the graph (as NoVisibleCandidate,
    // not silently dropped) -- it just isn't recursed into further.
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

/// End-to-end wiring check: uses the *real* profile-resolved config
/// (via portage-profile, a dev-dependency only) instead of the fixed
/// test_accept_keywords()/empty-USE sets every other test in this file
/// uses, proving real USE flags -- not just a hardcoded empty set --
/// actually reach use_reduce and change dependency resolution.
/// fixtures/repo/profiles resolves "foo" enabled and
/// "missingflag" disabled (see portage-profile's own fixture test),
/// so useflagpkg's `foo? ( dev-libs/newpkg )` must be pulled in and
/// its `missingflag? ( dev-libs/hiddendep )` must not be.
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
    // profiles/base/make.defaults declares USE_EXPAND="VIDEO_CARDS"
    // and VIDEO_CARDS="nvidia" -- expands into "video_cards_nvidia",
    // which dev-libs/useexpandpkg's own RDEPEND is gated on.
    let full_names: Vec<String> = graph_real("dev-libs/useexpandpkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/newpkg", "dev-libs/useexpandpkg"]);
}

#[test]
fn package_use_expand_shorthand_drives_dependency_recursion() {
    // fixtures/etc/portage/package.use has "dev-libs/
    // packageuseexpandpkg PYTHON_TARGETS: python3_12" -- expands into
    // "python_targets_python3_12", which
    // dev-libs/packageuseexpandpkg's own RDEPEND is gated on.
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
    // fixtures/etc/portage/package.use has "dev-libs/wildexpandpkg
    // linguas_*" -- real config.py setcpv's own _* wildcard: enable
    // every linguas_<x> in this package's OWN IUSE
    // ("linguas_en linguas_de") that isn't masked. profiles/base/
    // package.use.mask keeps linguas_en off, so only linguas_de is
    // enabled -> RDEPEND's "linguas_de? ( wildexpanddep )" fires and
    // "linguas_en? ( wildexpandmasked )" does not.
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
    // profiles/base/make.defaults: USE_EXPAND_IMPLICIT="ELIBC",
    // USE_EXPAND_VALUES_ELIBC="glibc musl", ELIBC="glibc". So
    // elibc_glibc is valid implicit IUSE for every package AND
    // enabled -- dev-libs/implicitiusepkg RDEPENDs
    // implicitiuseprov[elibc_glibc], and implicitiuseprov never
    // lists elibc_glibc in its own IUSE. Before this slice that
    // dep's `.required` check failed against the (declared-only)
    // IUSE and implicitiuseprov was invisible.
    let entries = graph_real("dev-libs/implicitiusepkg");
    let prov = entries
        .iter()
        .find(|(name, _)| name == "dev-libs/implicitiuseprov")
        .expect("the dep is in the graph");
    // Resolved as a real New entry, NOT NoVisibleCandidate -- which
    // is exactly what it would be without this slice (elibc_glibc
    // absent from implicitiuseprov's declared IUSE -> `.required`
    // fails -> no matching candidate).
    assert_eq!(
        prov.1,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn use_expand_implicit_flag_valid_but_not_enabled_still_fails_the_use_dep() {
    // elibc_musl is valid implicit IUSE (in USE_EXPAND_VALUES_ELIBC)
    // but not enabled (ELIBC="glibc"), so
    // implicitiuseprov[elibc_musl] is genuinely unsatisfiable --
    // implicitiusepkgmusl's own dep resolves to NoVisibleCandidate,
    // exactly like any other unsatisfiable USE-dep (proves the slice
    // widened the *valid* domain, not the *enabled* one).
    let entries = graph_real("dev-libs/implicitiusepkgmusl");
    let prov = entries
        .iter()
        .find(|(name, _)| name == "dev-libs/implicitiuseprov")
        .expect("the unsatisfiable dep is still recorded");
    assert_eq!(prov.1, PretendOutcome::NoVisibleCandidate);
}

#[test]
fn fixture_stable_use_force_and_package_use_stable_mask_apply_when_stable() {
    // dev-libs/stableusepkg's own KEYWORDS="amd64" (no "~") is
    // genuinely stable -- use.stable.force (profiles/base) pulls in
    // its own RDEPEND.
    let full_names: Vec<String> = graph_real("dev-libs/stableusepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/newpkg", "dev-libs/stableusepkg"]);
}

#[test]
fn fixture_stable_use_force_skips_an_unstable_candidate() {
    // dev-libs/unstableusepkg's own KEYWORDS="~amd64" is genuinely
    // NOT stable -- the same use.stable.force never applies, so its
    // own stableforceflag?-gated RDEPEND is never pulled in.
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
    // dev-libs/withtestdepconsumer RDEPENDs on dev-libs/withtestdeppkg,
    // reached at depth 1, not depth 0 -- testonlydep must stay absent
    // even though --with-test-deps is given.
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
    // dev-libs/newpkg has no "test" IUSE flag declared at all -- real
    // portage's own "pkg.iuse.is_valid_flag(\"test\")" guard means
    // --with-test-deps changes nothing for it.
    assert_eq!(
        graph_real_with_test_deps("dev-libs/newpkg"),
        graph_real("dev-libs/newpkg")
    );
}

#[test]
fn newuse_reinstall_still_recurses_into_its_own_dependencies() {
    // dev-libs/reinstallpkg RDEPENDs on dev-libs/newpkg -- proving a
    // Reinstall entry is walked for dependencies exactly like New/
    // Upgrade, not treated like the AlreadyInstalled dead-end it used
    // to be before --newuse existed.
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
    // Neither the profile nor make.conf enables "pkguseflag", but
    // fixtures/etc/portage/package.use has a "*/packageuseenablepkg
    // pkguseflag" wildcard entry, so newpkg's foo?-unrelated,
    // pkguseflag?-gated dependency must be pulled in.
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
    // Same fixture as fixture_package_use_enables_a_flag_not_on_globally
    // above, but with --nodeps: the real profile-resolved config still
    // decides packageuseenablepkg's own visibility/USE normally, it's
    // only the *recursion* into its now-enabled pkguseflag?-gated
    // RDEPEND that --nodeps skips -- dev-libs/newpkg must NOT appear.
    let full_names: Vec<String> = graph_real_nodeps("dev-libs/packageuseenablepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/packageuseenablepkg"]);
}

#[test]
fn fixture_package_use_disables_a_flag_that_is_on_globally() {
    // The fixture profile chain enables "foo" globally (see
    // resolves_fixture_profile_chain_and_make_conf in portage-profile,
    // and dev-libs/useflagpkg's own test above, which -- unlike this
    // package -- DOES pull in its foo?-gated dependency), but
    // fixtures/etc/portage/package.use has a "dev-libs/packageusedisablepkg
    // -foo" entry scoped to just this package, so its own foo?-gated
    // dependency must NOT be pulled in.
    let full_names: Vec<String> = graph_real("dev-libs/packageusedisablepkg")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(full_names, vec!["dev-libs/packageusedisablepkg"]);
}

#[test]
fn use_dep_enforcement_matches_a_declared_and_enabled_flag() {
    // dev-libs/useflagpkg's own IUSE="foo missingflag", "foo"
    // enabled globally by the fixture profile chain -- see
    // resolve_pretend's own doc comment on use_deps_satisfied.
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
    // dev-libs/usedeprejectedpkg's own RDEPEND is
    // "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the
    // parent still resolves, and the dependency gets its own
    // NoVisibleCandidate entry (reported, not silently dropped or
    // failing the whole graph -- same "report, don't fail"
    // precedent an unresolvable dependency atom already gets; see
    // resolve_pretend_graph's own doc comment).
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
    // dev-libs/requireduseokpkg's own REQUIRED_USE is "foo? ( bar )"
    // -- foo enabled globally, bar forced on by this package's own
    // package.use entry, so genuinely satisfied.
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
    // dev-libs/requiredusebadpkg's own REQUIRED_USE is "foo? ( bar )"
    // -- foo enabled globally, bar never forced on, genuinely
    // violated. Real depgraph.py's own REQUIRED_USE check happens
    // right after package selection and aborts the whole run on
    // failure -- a materially different severity than a merely
    // unresolvable dependency (report, don't fail).
    // Real `_show_unsatisfied_dep`'s REQUIRED_USE block -- the whole
    // one-clause expression is the minimal unsatisfied one (no
    // "complete expression" line); top-level atom -> no dep chain.
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
    // dev-libs/requiredusebadparentpkg RDEPENDs on
    // dev-libs/requiredusebadpkg -- proving the same fatal severity
    // applies regardless of whether the violating package was
    // reached as a top-level atom or a dependency.
    // Reached as a dependency -> the block gains the `(dependency
    // required by "..." [ebuild])` chain, plus a trailing
    // `[argument]` line since the parent is itself the CLI argument.
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
    // Real depgraph.py's own _add_pkg sets
    // _dynamic_config._required_use_unsatisfied = True and returns 0
    // on a violation -- it does NOT abort the whole graph walk, so a
    // SECOND, independent top-level atom's own violation (here,
    // dev-libs/requiredusebadpkg2's own "baz? ( qux )", unrelated to
    // dev-libs/requiredusebadpkg's own "foo? ( bar )") still gets
    // reached, resolved, and reported too -- not silently skipped
    // because the first atom already failed. Confirmed live against
    // both portuale's own Rust and Python implementations
    // (byte-identical joined output) before this test was written.
    // Backlog #90 (S1): report order follows the walk, which
    // resolves last-declared-first (real's stack discipline) -- so
    // badpkg2's block now leads. (Real itself stops at the first
    // violation, argv order -- staged oracle shows only badpkg --
    // but collecting both stays portuale's own established,
    // separately-pinned behavior; only the order moves here.)
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
fn fixture_dependency_no_visible_candidate_gets_no_keyword_suggestion_by_default() {
    // dev-libs/autounmaskdepconsumer RDEPENDs on dev-libs/
    // autounmaskkeywordpkg (the same keyword-masked-only fixture the
    // top-level test above uses), a *dependency's* own
    // NoVisibleCandidate -- previously always silent, now able to
    // carry a keyword_suggestion, but only once
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
    // With `--autounmask` explicit the keyword-masked dependency now
    // RESOLVES (was `NoVisibleCandidate` + a `keyword_suggestion`
    // before autounmask keyword *resolution* shipped).
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
    // Backlog #265(b) (with #273): an *argument* atom's own USE miss
    // aborts with real `_show_unsatisfied_dep`'s "no ebuilds built
    // with USE flags" block -- the `(Change USE: …)` row and no
    // `(dependency required by …)` lines (real skips the chain for
    // an `AtomArg` parent, `depgraph.py:7080-7090`). Live real
    // 3.0.82.2 staged-fixture probe
    // (`LLM/evidence/2026-09-30-265/real/useflagpkg-autounmask-use-n.txt`):
    // `emerge --pretend --autounmask-use=n
    // 'dev-libs/useflagpkg[-foo]'` prints the block where portuale
    // printed the plain `no ebuilds to satisfy` miss. Real's staging
    // `for <root>.` suffix stays absent (the long-standing
    // `fixture-miss-message-unsuffixed` class).
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
    // dev-libs/usedeprejectedpkg RDEPENDs on
    // "dev-libs/useflagpkg[-foo]" (the same fixture the top-level
    // test above uses), a *dependency's* own NoVisibleCandidate --
    // able to carry a use_suggestion, but only once
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
    // dev-libs/archiuseimplicitpkg's own IUSE is empty and its own
    // REQUIRED_USE is "!x86" -- "x86" is never declared by this
    // package's own IUSE at all, but IS a real, valid flag via
    // PORTAGE_ARCHLIST (fixtures/repo/profiles/base/arch.list),
    // exactly like real media-libs/mesa's own REQUIRED_USE
    // referencing "x86" without declaring it in IUSE -- confirmed
    // live against the real, installed system. Before this fix,
    // portuale's own iuse_set never consulted PORTAGE_ARCHLIST at
    // all, so this would fail with "USE flag 'x86' is not in IUSE"
    // instead of resolving (x86 isn't the active profile's own arch,
    // so it stays disabled, and "!x86" is satisfied).
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
    // A walked before B: B contradicts A's needed `-foo`, real's
    // breakage clean pass fails on A, no conflict reached.
    for order in ["c b a", "b c a", "b a c"] {
        let result = aub0_result(order, false);
        assert_aub0_fails_on_a(&result, order);
        assert!(result.slot_conflicts.is_empty(), "{order}");
    }
    // B walked before A: A's `-foo` is the first needed change; the
    // loop stops on it (`need_config_change`), the pass is the report.
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
    // The same fixture with `--autounmask-use=n`: the shared
    // `autounmask_suggest_use` gate is off, so neither the child nor
    // the parent flip is attempted -- the dependency stays
    // `NoVisibleCandidate` and no change is recorded.
    let result = graph_result_autounmask_use_n("dev-libs/parentflipeqpkg");
    let child = result
        .entries
        .iter()
        .find(|e| e.package == "parentflipchildpkg")
        .expect("child entry");
    assert!(matches!(child.outcome, PretendOutcome::NoVisibleCandidate));
    assert!(result.autounmask_use_changes.is_empty());
}

/// Backlog #161 S2: `adopt_current` replaces the live node's params
/// in place (the C2 dead-end accumulator hand-off) without adding a
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

/// Backlog #161 S2: `get_best_run` returns the deepest terminal
/// node, earliest on ties -- the most config progress with no masks.
/// (The children differ in accumulator state, not just position:
/// `add_node` dedups position-only progress via `params_equal`.)
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
    // Both children are terminal at depth 3; the tie keeps the
    // first, not the last.
    let best = bt.get_best_run();
    assert_eq!(best.depth, 3);
    assert!(
        best.autounmask_use_config.contains_key(&cp_a)
            && !best.autounmask_use_config.contains_key(&cp_b),
        "depth ties keep the first deepest terminal"
    );
}

/// Backlog #161 S2: the slot-conflict arm masks the target in a new
/// node (costing one budget unit), replacing -- not duplicating -- a
/// mask for the same negative.
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

/// Backlog #161 S2: the missing-dep arm latches the parent and masks
/// it once -- an already-masked parent is not pushed again -- and a
/// mask step costs one budget unit.
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

/// Backlog #205 R4: the USE-change chain starts at the recorded
/// forcing parent. Real `_get_dep_chain(pkg,
/// unsatisfied_dependency=True)` (`lib/_emerge/depgraph.py:6257`)
/// picks the parent whose atom the package still does not satisfy;
/// here that is `abk0d-1` (whose `C[x]` the flip satisfies, not
/// `abk0a-2`'s plain `C`). The flip site records that owner in
/// `AutounmaskChange::trigger`, and the fill starts the chain
/// there -- asserting the recorded trigger itself, not just the
/// final text, is what distinguishes this from the legacy
/// first-requirer start.
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
    // #57 direction 2: argv `[othermod, needer]`. Backlog #90 (S1):
    // the walk resolves last-declared-first (real's stack), so
    // `needer`'s `>=paired-2.0` graphs 2.0 first and `othermod`'s
    // `<paired-2.0` then collides with the installed 1.0 -- the
    // merge instance files first, exactly real's notice order for
    // this argv order (staged oracle: 2.0 by needer, then 1.0).
    // Real's `_package_tracker` holds both nodes and reports the
    // collision; portuale used to merge all three silently. The two
    // atoms are jointly unsatisfiable, so the backtracker's mask
    // trials all fail and `get_best_run` reports the original
    // shape -- exactly real's `backtrack: 4/20` run, whose block is
    // byte-identical to its own `--backtrack=0` one
    // (`TEST/findings/l0-fixture-oracle.md` "#57 S0" cells b/c).
    let result = graph_result_real_atoms(
        &[
            "dev-libs/othermod".to_string(),
            "dev-libs/needer".to_string(),
        ],
        10,
    );
    assert_triangle_conflict(&result, false);
    // The upgrade still merges, like real's merge list.
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
    // #57 direction 1, the mirror argv order: argv `[needer,
    // othermod]`. Backlog #90 (S1): last-declared-first, so
    // `othermod`'s `<paired-2.0` meets the installed 1.0 (the
    // walker's `AlreadyInstalled` early branch) before `needer`'s
    // `>=paired-2.0` graphs 2.0 -- the installed instance files
    // first, exactly real's notice order for this argv order
    // (staged oracle: 1.0, then 2.0 by needer).
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
    // #57 K2: `plainuser`'s bare `dev-libs/paired` is satisfied by
    // the installed 1.0 *and* by the 2.0 `needer` forces, so one
    // graphed instance satisfies every hard parent atom -- real
    // `_solve_non_slot_operator_slot_conflicts` reconciles it
    // silently. Portuale reaches the same place through
    // `collect_feedback`'s existing `slot_want` solvability check
    // (the merge-vs-merge reconciliation reused, not a second
    // solver), so no record survives to the settled pass.
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
    // A test-local vdb (the shared fixtures keep only the slotbind
    // repo ebuilds/md5-cache): slotbindtarget-1.0 at SLOT="2" plus a
    // stale `:2/2=` consumer. Emerging the target picks
    // slotbindtarget-2.0 (SLOT="2/9") from the shared repo, so the
    // consumer is normally rebuilt -- unless
    // `ignore_built_slot_operator_deps` is set.
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
            // selective = false: a directly-named installed atom
            // with a higher version still upgrades (matching real
            // `emerge <atom>` without `--noreplace`).
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
    // #24 S3: the rebuilt consumer is a *walked node*, not a
    // post-pass synthetic entry -- it carries the tree ebuild's own
    // dependency edges (`slotbindtarget:2/2=` re-read from
    // `metadata/md5-cache`), which is what puts the provider ahead
    // of it in merge order. The pre-S3 synthesiser left `deps`
    // empty, so this assertion is the slice.
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
    // Exactly one entry per cp: the flip happens where the outcome is
    // first decided, so no `AlreadyInstalled` row is left behind.
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
    // avoidslotpkg-1.0 is installed at SLOT="2" (the repo ebuild says
    // SLOT="1"); avoidslotconsumer RDEPENDs `dev-libs/avoidslotpkg:1`.
    // The `:1` dependency must NOT short-circuit to "already
    // installed" off the slot-2 install -- slot 1 is genuinely new.
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

    // A new-slot New: oldbest lists every installed version (all
    // slots); the vdb entry for newslotpkg-1.0 has no `repository`
    // file -> `__unknown__`... but portuale's fixtures add one, so
    // it's testrepo. Its slot ("0") differs from the resolved slot.
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
    // dev-libs/needsprovided RDEPENDs dev-libs/providedpkg (listed in
    // profiles/default/package.provided) + dev-libs/newpkg. The
    // provided dep is silently dropped -- only needsprovided +
    // newpkg show, and pprovided_atoms stays empty (it's a
    // dependency, not a top-level target).
    let result = graph_result_real("dev-libs/needsprovided");
    let names: Vec<&str> = result.entries.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["newpkg", "needsprovided"]);
    assert!(result.pprovided_atoms.is_empty());
}

#[test]
fn package_provided_records_a_matching_top_level_target_for_the_warning() {
    // A direct `emerge providedpkg` resolves nothing (no entry) and
    // records the atom in pprovided_atoms -- real depgraph.py's own
    // _pprovided_args, which pretend.rs turns into the `WARNING: …
    // package.provided:` block.
    let result = graph_result_real("dev-libs/providedpkg");
    assert!(result.entries.is_empty());
    assert_eq!(
        result.pprovided_atoms,
        vec!["dev-libs/providedpkg".to_string()]
    );
}

#[test]
fn buildpkgonly_flags_a_new_package_depending_on_another_new_package() {
    // dev-libs/dualdep (New) has both DEPEND and RDEPEND on
    // dev-libs/newpkg (also New) -- real --buildpkgonly can't
    // resolve this, since newpkg itself would also need building.
    let result = graph_result_buildpkgonly("dev-libs/dualdep");
    assert!(result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn buildpkgonly_does_not_fire_when_the_dependency_is_already_installed() {
    // dev-libs/buildpkgonlysatisfied (New) RDEPENDs on dev-libs/
    // samepkg, which is already installed -- nothing else needs
    // building.
    let result = graph_result_buildpkgonly("dev-libs/buildpkgonlysatisfied");
    assert!(!result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn buildpkgonly_is_always_false_when_not_requested() {
    // graph_result_real's own helper always passes buildpkgonly=false
    // -- even for the same dualdep fixture that trips the check
    // above, the field must stay false when the flag isn't given at
    // all.
    let result = graph_result_real("dev-libs/dualdep");
    assert!(!result.buildpkgonly_deps_unsatisfied);
}

#[test]
fn fixture_strong_blocker_matches_an_installed_package() {
    // dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and
    // dev-libs/samepkg-1.0 is already installed per the fixture vdb.
    // #68 S3 (container oracle, 2026-09-16): a plain `emerge -p
    // dev-libs/blockerpkg` prints `b` + `[uninstall samepkg-1.0]`,
    // rc 0 -- the other installed fixtures that RDEPEND on samepkg
    // are *not* walked into the graph, so they do not block the
    // unmerge. (Pre-S3 this expected `unsolvable: true` from the vdb
    // reverse scan.)
    let entries = graph_entries_real("dev-libs/blockerpkg");
    // #72 B3: the satisfied block also produces the removal entry
    // real models as an uninstall task (display of it is B4).
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
            // #72 B3: resolved by removing the installed match.
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
    // #68/#72 B2c, B0 cell u4: real re-runs `_serialize_tasks` in
    // complete mode when the first pass selected an uninstall task
    // (`depgraph.py:10365-10383`), and that walk covers
    // `@selected ∪ @profile ∪ @system` -- so an installed blocked
    // instance reachable from a world member is a digraph node with
    // parents and the block is `B` (rc 1), even though the resolve
    // itself is not complete.
    //
    // `dev-libs/blockerpkg` (`!!dev-libs/samepkg`) with installed
    // `samepkg-1.0`: with no seeds the block is uninstall-resolved
    // (`b`, the `test_strong_blocker_matches_an_installed_package`
    // pin); with installed `dev-libs/changeddepspkg` (vdb
    // `RDEPEND=dev-libs/samepkg`) as the world seed, the retry
    // closure reaches `samepkg` -> unresolved.
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
    // dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg
    // and dev-libs/weakblockerpkg (whose RDEPEND is
    // "!dev-libs/blockerpartnerpkg") as New in the same run, so the
    // weak blocker must be reported against blockerpartnerpkg's
    // graph-resolved version, not just against the (empty) vdb.
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
            // #68 S2 (container oracle, 2026-09-16): real prints
            // `[blocks B]` for this merge-vs-merge shape -- a
            // merge-bound match with a merging parent is unresolved.
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

/// Backlog #285: an exact-version top-level request keeps its named
/// installed instance under `--update`. Same installed/tree shape as
/// the #268 pin (`mmprov-1` `0/1` + `mmprov-2` `1/1` + `mmcons-1`
/// bound `mmprov:0/1=`, tree `mmprov-1/-2/-3` with `mmprov-3` at
/// `1/2`), but the tree here is the shared fixture repo (resolved
/// through `config_root`) while the vdb is ad-hoc under `root` --
/// the `_b1_root` split. Full `--update --deep` walks, real 3.0.82.2
/// bytes from the R3 S0 capture (host staged-fixture probe
/// 2026-10-02, `--ignore-default-opts --pretend --color=n
/// --backtrack=3 --update --deep <atoms>`, rc 0 throughout):
/// - `=app-misc/mmprov-2 app-misc/mmcons` -> `[ebuild rR]
///   app-misc/mmcons-1` alone (`backtrack: 1/3`): the argument pins
///   installed `mmprov-2` in the graph and the rebuilt consumer's
///   `app-misc/mmprov:=` reuses it (real
///   `_wrapped_select_pkg_highest_available_imp`'s
///   `find_existing_node` pass) instead of upgrading to `mmprov-3`.
/// - `~app-misc/mmprov-2 app-misc/mmcons` -> the same single row.
/// - Control `app-misc/mmprov:1/2 app-misc/mmcons` -> `[ebuild U]
///   app-misc/mmprov-3 [2]` + `[ebuild rR] app-misc/mmcons-1`: the
///   slot atom's own pick is the upgrade, so the dep binds that.
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
    // Merge-bound `(cat/pkg, version)` pairs, for the row-level
    // assertion below (an `AlreadyInstalled` provider merges
    // nothing and renders no row).
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
    // Control: the slot atom upgrades the provider itself, so the
    // rebuilt consumer binds the upgrade.
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
