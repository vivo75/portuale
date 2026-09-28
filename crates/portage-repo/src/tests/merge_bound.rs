use super::*;

/// The same g216 shape with `--backtrack=0` reports the pass-1 self-cycle block.
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
    // The cycle node is the merge-bound cpv (Portage prints the
    // scheduled-for-merge form).
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

/// Missed-update notices come from the settled backtrack trial state.
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
    // Portage's `test_blocker` shape: X-1 merges and installed Y-1 uninstalls
    // in all six argv orders.
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
