use super::*;

#[test]
fn complete_graph_auto_enable_reasonless_reinstall_with_use_drift() {
    // A reasonless `[ebuild R]` with USE drift still enables on `if_new_use`.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let reasonless = GraphEntry {
        category: "dev-libs".to_string(),
        package: "cgp0x".to_string(),
        outcome: PretendOutcome::Reinstall {
            version: "2.8.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("2".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        ..graph_entry("dev-libs", "cgp0x", "2.8.0")
    };
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: std::slice::from_ref(&reasonless),
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: false,
        root: &root,
        repos: &repos,
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: std::slice::from_ref(&reasonless),
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: &root,
        repos: &repos,
        config: &portage_profile::Config::default()
    }));
}
