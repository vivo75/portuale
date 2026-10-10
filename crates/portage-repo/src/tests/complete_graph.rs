use super::*;

#[test]
fn complete_graph_never_merges_a_missing_deep_dep_of_an_installed_pkg() {
    // deeppkg (installed) -> RDEPEND deeppkg2 (installed) -> RDEPEND
    // newpkg (not installed). `-D` deep-walks and merges newpkg.
    // Real `complete` mode (`_complete_graph` 8662) also deep-walks
    // but swaps to `_select_pkg_from_graph` -- graph-or-installed,
    // never a new merge -- so the CLI's 2-pass (phase 1 non-complete,
    // phase 2 complete with phase 1's merge set locked) leaves newpkg
    // accounted-for but unmerged. The low-level `complete=true` with
    // no locked set is the pre-2-pass fallback: a plain deep walk.
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

    // complete=false, no --deep: deeppkg's deps are not walked.
    assert!(!has_newpkg(&resolve(
        "dev-libs/deeppkg",
        false,
        Deep::NotRequested,
        false
    )));
    // `-D` deep-walks and merges the missing dep.
    assert!(has_newpkg(&resolve(
        "dev-libs/deeppkg",
        false,
        Deep::Unlimited,
        false
    )));
    // `complete=true` never merges the missing deep dep -- neither
    // the low-level call (no locked set) nor the CLI's real 2-pass
    // (phase 1's merge set locked). Real `_select_pkg_from_graph`:
    // graph-or-installed, never a new merge. And -- unlike `-D` --
    // complete mode doesn't force the deep walk at all unless
    // `--changed-deps-report` needs the wider scan (portuale's deep
    // walk is full resolution per node, far heavier than real's).
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

    // Auto-enable: `emerge -u completegraphpkg` is an Upgrade (a
    // version change), so `complete_graph_auto_enable` says to re-walk
    // when `complete_if_new_ver` is on -- but not when every trigger
    // is off.
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
    // Even after the auto-enable re-walk, the missing deep dep is not
    // merged (2-pass, `completegraphpkg` is the only phase-1 merge).
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
    // Backlog #222: a bare reasonless `[ebuild R]` for an
    // explicitly-requested installed package carries no
    // `changed_flags` (they are only computed under `--newuse`/
    // `--changed-use`), yet real still enables complete mode when
    // the merge node's own USE differs from the vdb's
    // (`depgraph.py:8620-8637`). On the committed fixtures
    // `dev-libs/cgp0x-2.8.0` (IUSE `+icu`, installed `USE=""`)
    // such a reinstall enables on `if_new_use` even with empty
    // `changed_flags`.
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
