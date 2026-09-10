use super::*;

#[test]
fn schedule_graph_prunes_rootless_synthetic_nodes_and_gates_complete_mode() {
    let root = mo_tmp_vdb(
        "schedule",
        &[
            ("dev-libs", "sysseed-1.0", &[("SLOT", "0")]),
            (
                "dev-libs",
                "dep-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/grand")],
            ),
            ("dev-libs", "grand-1.0", &[("SLOT", "0")]),
        ],
    );
    let repos: &[RepoConfig] = &[];
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sysseed".to_string()],
        ..portage_profile::Config::default()
    };
    // An all-New resolve is virtuals_only: no installed closure.
    let entries = vec![new_entry("dev-libs", "top", "1.0", Vec::new())];
    let (ext, g, real_n, _) = schedule_graph(ScheduleGraphArgs {
        entries: &entries,
        top_level_atoms: &["dev-libs/top".to_string()],
        config: &config,
        root: &root,
        implicit_system_deps: true,
        repos,
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    assert_eq!(
        ext.len(),
        1,
        "no complete-mode closure for an all-New resolve"
    );
    assert_eq!(real_n, 1);
    assert!(g.order.iter().all(|&i| i < real_n));
    // A Reinstall entry turns complete mode on: the @system seed is
    // added, walked, and then pruned as a rootless nomerge node.
    let mut top = new_entry("dev-libs", "top", "1.0", Vec::new());
    top.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: vec!["flip".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let entries = vec![top];
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sysseed".to_string()],
        ..portage_profile::Config::default()
    };
    let (ext, g, real_n, _) = schedule_graph(ScheduleGraphArgs {
        entries: &entries,
        top_level_atoms: &["dev-libs/top".to_string()],
        config: &config,
        root: &root,
        implicit_system_deps: true,
        repos,
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    assert_eq!(real_n, 1);
    let seed = ext
        .iter()
        .position(|e| e.package == "sysseed")
        .expect("the @system seed is added");
    assert!(seed >= real_n);
    assert!(!g.alive[seed], "a rootless synthetic node is pruned");
    assert!(!g.order.contains(&seed));
}
