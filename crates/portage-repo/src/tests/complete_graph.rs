use super::*;

#[test]
fn complete_graph_never_merges_a_missing_deep_dep_of_an_installed_pkg() {
    // deeppkg (installed) -> deeppkg2 (installed) -> newpkg (not installed).
    // `-D` merges newpkg; `complete=true` never does (graph-or-installed only).
    let root = fixtures_root();
    let resolve_cfg =
        |atom: &str, update: bool, deep: Deep, complete: bool, locked: &[&str]| -> GraphResult {
            let mut cfg = test_config();
            cfg.complete_locked_merges = locked.iter().map(|s| s.to_string()).collect();
            ResolveRequest {
                update,
                deep,
                complete,
                ..ResolveRequest::new(
                    &root,
                    &root,
                    &[atom.to_string()],
                    &cfg,
                    &fixtures_root().join("distfiles"),
                )
            }
            .resolve()
            .unwrap_or_else(|e| panic!("resolving {atom} failed: {e}"))
        };
    let resolve = |atom: &str, update: bool, deep: Deep, complete: bool| -> GraphResult {
        resolve_cfg(atom, update, deep, complete, &[])
    };
    let names = |r: &GraphResult| -> Vec<String> {
        r.entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect()
    };
    let has_newpkg = |r: &GraphResult| names(r).iter().any(|n| n == "dev-libs/newpkg");

    // complete=false, no --deep: deps not walked.
    assert!(!has_newpkg(&resolve(
        "dev-libs/deeppkg",
        false,
        Deep::NotRequested,
        false
    )));
    // `-D` merges the missing dep.
    assert!(has_newpkg(&resolve(
        "dev-libs/deeppkg",
        false,
        Deep::Unlimited,
        false
    )));
    // `complete=true` never merges the missing deep dep, and unlike `-D` it
    // doesn't force the deep walk unless `--changed-deps-report` needs it.
    assert!(!has_newpkg(&resolve(
        "dev-libs/deeppkg",
        false,
        Deep::NotRequested,
        true
    )));
    assert!(!has_newpkg(&resolve_cfg(
        "dev-libs/deeppkg",
        false,
        Deep::NotRequested,
        true,
        &["dev-libs/deeppkg"],
    )));

    // Auto-enable: an Upgrade re-walks when `complete_if_new_ver` is on.
    let up = resolve("dev-libs/completegraphpkg", true, Deep::NotRequested, false);
    assert!(matches!(
        up.entries
            .iter()
            .find(|e| e.package == "completegraphpkg")
            .map(|e| &e.outcome),
        Some(PretendOutcome::Upgrade { .. })
    ));
    assert!(!has_newpkg(&up));
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &up.entries,
        if_new_use: true,
        if_new_ver: true,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &up.entries,
        if_new_use: false,
        if_new_ver: true,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &up.entries,
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &up.entries,
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    // The re-walk still merges nothing new (only `completegraphpkg` locked).
    assert!(!has_newpkg(&resolve_cfg(
        "dev-libs/completegraphpkg",
        true,
        Deep::NotRequested,
        true,
        &["dev-libs/completegraphpkg"],
    )));

    // An AlreadyInstalled-only graph never auto-enables.
    let noop = resolve("dev-libs/deeppkg2", false, Deep::NotRequested, false);
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &noop.entries,
        if_new_use: true,
        if_new_ver: true,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
}

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
