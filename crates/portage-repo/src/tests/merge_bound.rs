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
