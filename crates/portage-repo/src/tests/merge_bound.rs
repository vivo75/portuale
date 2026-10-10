use super::*;

/// Track X Slice B (backlog #242, ex-#207): with a non-`/` target
/// ROOT, EAPI >= 7 `DEPEND` resolves against the running root (real
/// `_add_pkg_deps`'s `depend_root`), so graph nodes carry their root
/// and candidates select against that root's tree/installed db.
/// `=dev-libs/cyc0z-1`'s `DEPEND` pulls `cyc0y` into the (here empty)
/// running root, whose unversioned `DEPEND` then selects `cyc0z-3` --
/// the target-root argument is not reused across the root boundary --
/// forming the running-root `cyc0z-3 <-> cyc0y-1` cycle live real
/// reports under default staging (backtrack `1/20`, blame `cyc0z-3`;
/// `LLM/evidence/2026-09-29-242-inventory/probes/default-cyc0z-1.txt`).
#[test]
fn cross_root_depend_forms_the_running_root_cycle() {
    let root = fixtures_root();
    // Hermetic running root: nothing installed there, so every build
    // dep selects from the tree -- the empty-`/` half of the S0 repro
    // (a parent in <ROOT> whose DEPEND resolves against the running
    // root, where the running-root choice forms a cycle).
    let running = TempDir::new("portuale-242-xb-empty-running-root").keep();
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
            &["=dev-libs/cyc0z-1".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving =dev-libs/cyc0z-1 failed: {e}"));
    // Real starts the printed cycle at `cyc0z-3`, reached from
    // `cyc0y-1` -- the argument version is not a cycle member.
    // (`cat/pkg-version` CPVs: `cyc0z-3` is version 3 of `cyc0z`.)
    assert_eq!(
        result.circular_deps,
        vec![vec![
            "dev-libs/cyc0z-3".to_string(),
            "dev-libs/cyc0y-1".to_string()
        ]]
    );
    // The blame node is a running-root merge: it carries its root.
    let blamed = result
        .entries
        .iter()
        .find(|e| {
            e.category == "dev-libs"
                && e.package == "cyc0z"
                && merge_bound_version(&e.outcome).is_some_and(|v| v == "3")
        })
        .expect("cyc0z-3 is a graph node");
    assert!(blamed.targets_running_root);
    // Real spends its single backtrack retry on the serialize abort.
    assert!(result.backtrack_restarts >= 1);
    assert!(matches!(result.outcome, ResolveOutcome::Aborted { .. }));
}

/// Track X Slice C (#242): the same g216 shape with `--backtrack=0`
/// reports real's pass-1 self-cycle block (rc 1), like real's
/// `default-g216top-b0.txt` (`Total: 4` over the dual rows).
#[test]
fn cross_root_bdepend_self_branch_backtrack0_reports_the_self_cycle() {
    let root = fixtures_root();
    let running = TempDir::new("portuale-242-xc-empty-running-root-b0").keep();
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
        backtrack_max: 0,
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
    assert!(matches!(result.outcome, ResolveOutcome::Aborted { .. }));
    // The pass-1 graph strands on the running-root self loop: four
    // merge rows (dual `g216comp`) behind the self block. The cycle
    // node is the merge-bound cpv (the `circular_deps` convention
    // everywhere -- real prints
    // `(dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled for
    // merge)`).
    assert_eq!(
        result.circular_deps,
        vec![vec!["dev-lang/g216comp-1.0".to_string()]]
    );
    let merge_rows = result
        .entries
        .iter()
        .filter(|e| merge_bound_version(&e.outcome).is_some())
        .count();
    assert_eq!(merge_rows, 4);
}

/// Backlog #198: with keyword suggestions off (real
/// `--autounmask-keep-keywords=y`, i.e. no `_autounmask_levels`
/// step unmasks `~amd64`), the newest `akk0a-2`'s `akk0b` dep is
/// unsatisfiable even USE-free, so the missing-dependency
/// backtrack masks the top-level-selected parent
/// (`depgraph.py:3493-3503`, no top-level exemption) and the retry
/// settles `akk0a-1` + `akk0c-1` (`foo` is globally on, so no USE
/// change is recorded). The masked `akk0a-2` rides out as a
/// full-form skipped-missing-dep row (real
/// `_show_missed_update_unsatisfied_dep`, `depgraph.py:1592`):
/// selected `akk0a-1`, skipped `akk0a-2`, masked-only `akk0b`
/// disclosure chained to the single skipped parent.
#[test]
fn missing_dep_backtrack_masks_the_top_level_selected_parent() {
    let result = graph_result_real_backtrack("dev-libs/akk0a", 10);
    let versions: Vec<(&str, &str)> = result
        .entries
        .iter()
        .filter_map(|e| merge_bound_version(&e.outcome).map(|v| (e.package.as_str(), v.as_str())))
        .collect();
    assert_eq!(
        versions,
        vec![("akk0c", "1"), ("akk0a", "1")],
        "falls back to akk0a-1, saw {versions:?}"
    );
    assert_eq!(
        result.backtrack_restarts, 1,
        "one retry masks akk0a-2, restarts={}",
        result.backtrack_restarts
    );
    assert_eq!(result.skipped_missing_dep_full.len(), 1);
    let row = &result.skipped_missing_dep_full[0];
    assert_eq!(
        (
            row.category.as_str(),
            row.package.as_str(),
            row.slot.as_str()
        ),
        ("dev-libs", "akk0a", "0")
    );
    assert_eq!(
        row.selected_display,
        "(dev-libs/akk0a-1:0/0::testrepo, ebuild scheduled for merge)"
    );
    assert_eq!(
        row.skipped_display,
        "(dev-libs/akk0a-2:0/0::testrepo, ebuild scheduled for merge)"
    );
    match &row.dep {
        SkippedDepDisclosure::Masked(report) => {
            assert_eq!(report.atom, "dev-libs/akk0b");
            assert_eq!(
                report.masked,
                vec![(
                    "dev-libs/akk0b-1::testrepo".to_string(),
                    vec!["~amd64 keyword".to_string()]
                )]
            );
            assert_eq!(
                report.chain,
                vec![(
                    "dev-libs/akk0a-2::testrepo".to_string(),
                    "ebuild".to_string()
                )]
            );
        }
        SkippedDepDisclosure::Plain(report) => {
            panic!("akk0b is masked-only, not a plain miss: {report:?}")
        }
    }
}

/// Backlog #129 (S1): real's two notices on the mg2top backtrack
/// shape (`l0-fx-20260927T080515Z` oracle) come from the settled
/// backtrack trial state, not from a second pass. The search
/// settles every `-1`, and the accumulated masks derive the
/// slot-conflict `WARNING` rows (`mgxc-2` vs `=mgxc-1`/`mgxa-1`,
/// `mgfc-3.0` vs `=mgfc-1`/`mgfa-1`, in first-mask order) plus the
/// abbreviated missing-dependency slots (`mgxb:0`, `mgfb:0`).
#[test]
fn backtrack_missed_updates_reports_the_mg2top_shape() {
    let result = graph_result_real_backtrack("dev-libs/mg2top", 20);
    assert!(
        result.slot_conflicts.is_empty(),
        "the search settles with no surviving conflict"
    );
    let versions: Vec<(&str, &str)> = result
        .entries
        .iter()
        .filter_map(|e| merge_bound_version(&e.outcome).map(|v| (e.package.as_str(), v.as_str())))
        .collect();
    for pkg in ["mg2top", "mgfa", "mgfb", "mgfc", "mgxa", "mgxb", "mgxc"] {
        assert!(
            versions.iter().any(|(p, v)| p == &pkg && v == &"1"),
            "settles {pkg}-1, saw {versions:?}"
        );
    }
    assert_eq!(
        result
            .skipped_updates
            .iter()
            .map(|s| (
                s.package.as_str(),
                s.skipped_version.as_str(),
                s.atom.as_str(),
                s.consumer_cpv.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "mgxc",
                "2",
                "=dev-libs/mgxc-1",
                "dev-libs/mgxa-1:0/0::testrepo"
            ),
            (
                "mgfc",
                "3.0",
                "=dev-libs/mgfc-1",
                "dev-libs/mgfa-1:0/0::testrepo"
            ),
        ]
    );
    assert_eq!(
        result
            .skipped_missing_deps
            .iter()
            .map(|s| (s.category.as_str(), s.package.as_str(), s.slot.as_str()))
            .collect::<Vec<_>>(),
        vec![("dev-libs", "mgxb", "0"), ("dev-libs", "mgfb", "0"),]
    );
}

#[test]
fn blk_pg0_all_orders_merge_x1_and_uninstall_y1() {
    // #142 S2 (upstream `test_blocker.py::testBlocker`, oracle
    // `differential-test-bed/logs/fx-b1-blocker.json` cases 0-5 and
    // the `l0-fx-20260922T101821Z` blk0 cells): real merges X-1 and
    // uninstall-orders installed Y-1 in all six argv orders
    // (`--backtrack=0`, rc 0). Joint assertion over the whole
    // defect: the joint slot solve (F1) must settle the row at the
    // uniquely jointly-satisfying X-1 from any walk order, the
    // stale-owner gate (F2) must drop every non-merged candidate's
    // blocker, and the keeper walk (F3) must file X-1's own
    // `!=Y-1` so the uninstall pairs Y-1.
    let orders = [
        ["dev-libs/blk0a", "dev-libs/blk0b", "dev-libs/blk0c"],
        ["dev-libs/blk0a", "dev-libs/blk0c", "dev-libs/blk0b"],
        ["dev-libs/blk0b", "dev-libs/blk0a", "dev-libs/blk0c"],
        ["dev-libs/blk0b", "dev-libs/blk0c", "dev-libs/blk0a"],
        ["dev-libs/blk0c", "dev-libs/blk0a", "dev-libs/blk0b"],
        ["dev-libs/blk0c", "dev-libs/blk0b", "dev-libs/blk0a"],
    ];
    for order in orders {
        let atoms: Vec<String> = order.iter().map(|s| s.to_string()).collect();
        let result = graph_result_real_atoms(&atoms, 0);
        let x = result
            .entries
            .iter()
            .find(|e| e.package == "blk0x")
            .unwrap_or_else(|| panic!("{}: no blk0x entry", atoms.join(" ")));
        assert_eq!(
            merge_bound_version(&x.outcome).map(String::as_str),
            Some("1"),
            "{}: blk0x must settle at X-1",
            atoms.join(" ")
        );
        let uninstalls: Vec<String> = result
            .entries
            .iter()
            .filter_map(|e| match &e.outcome {
                PretendOutcome::Uninstall { version } => {
                    Some(format!("{}/{}-{version}", e.category, e.package))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            uninstalls,
            vec!["dev-libs/blk0y-1".to_string()],
            "{}: exactly Y-1 uninstalls",
            atoms.join(" ")
        );
    }
}
