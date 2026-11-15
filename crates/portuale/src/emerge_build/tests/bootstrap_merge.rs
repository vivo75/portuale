//! Charter 2a bootstrap, real merges (backlog #753 C1/C2): the 3-row plan
//! `[boot otherpkg(-useboh) -> pkgnew -> otherpkg (definitive)]` merges to
//! completion under `-j1` (serial engine) and `-j2` (the parallel
//! scheduler), and the boot row executes with its own reduced USE.
//!
//! The boot build's marker cannot be asserted after the full plan: the
//! definitive reinstall is the same PF, so its unmerge deletes the boot
//! row's files first (Portage reinstall semantics). C2 is therefore pinned
//! by merging the boot row alone in its own ROOT (its marker and VDB USE
//! lack the reduced flag), while the full-plan test pins C1 (all 3 rows
//! land, rc 0) and the survivor's declared USE.
//!
//! Fixtures are the `dev-libs/w753*` ebuilds, whose `src_install` records
//! `USE` into per-build marker files: `USE="-useboh"` in the boot build,
//! full USE in the definitive one.

use super::*;

use std::collections::HashSet;
use std::sync::Arc;

fn bootstrap_atoms() -> Vec<String> {
    ["dev-libs/w753pkgnew", "dev-libs/w753otherpkg"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn fixture_config(root: &TempDir) -> portage_profile::Config {
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &fixtures_root(),
        main_repo_location: &fixtures_root().join("repo"),
        overlay_repos: &[("overlay".to_string(), fixtures_root().join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &std::collections::HashMap::new(),
        eroot: root.path(),
    })
    .expect("fixture config resolves")
}

/// Resolve the owner pair's 3-row bootstrap plan over the fixture tree
/// into a scratch ROOT: the same rows `--pretend` prints.
fn bootstrap_plan(root: &TempDir) -> (Vec<GraphEntry>, Vec<RepoConfig>) {
    let repos = find_repos(&fixtures_root()).unwrap();
    let config = fixture_config(root);
    let result = portage_repo::ResolveRequest::new(
        &fixtures_root(),
        root.path(),
        &bootstrap_atoms(),
        &config,
        &root.path().join("distfiles"),
    )
    .resolve()
    .expect("the w753 pair resolves to the bootstrap plan");
    assert_eq!(result.entries.len(), 3, "{:#?}", result.entries);
    assert!(
        result.entries[0].bootstrap.is_some(),
        "row 0 is the boot row: {:#?}",
        result.entries[0]
    );
    (result.entries, repos)
}

fn merge_options(distdir: &TempDir, config: portage_profile::Config) -> ebuild_merge::MergeOptions {
    let mut options = ebuild_merge::MergeOptions {
        distdir: distdir.path().to_path_buf(),
        config_root: fixtures_root(),
        ..ebuild_merge::MergeOptions::default()
    };
    options.resolved_config = Some(Arc::new(config));
    options
}

/// Merges `entries` into a scratch ROOT; panics with the jobs count on failure.
#[allow(clippy::too_many_arguments)]
fn do_merge(
    root: &TempDir,
    distdir: &TempDir,
    portage_tmpdir: &TempDir,
    entries: &[GraphEntry],
    repos: &[RepoConfig],
    jobs: usize,
) {
    let config = fixture_config(root);
    let options = merge_options(distdir, config);
    run_source_merge(RunSourceMergeArgs {
        entries,
        repos,
        root: root.path(),
        portage_tmpdir: portage_tmpdir.path(),
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .unwrap_or_else(|e| panic!("jobs={jobs}: the merge lands: {e}"));
}

fn read_use(root: &TempDir, path: &str) -> String {
    std::fs::read_to_string(root.path().join(path)).unwrap_or_default()
}

fn use_flags(text: &str) -> HashSet<String> {
    text.split_whitespace().map(|s| s.to_string()).collect()
}

/// C2: the boot row alone executes with the reduced USE, not the declared
/// one. Its marker and its VDB `USE` lack the reduced flag; the definitive
/// build never runs here, so no definitive marker exists.
#[test]
fn bootstrap_boot_row_alone_merges_with_the_reduced_use() {
    let root = tempdir();
    let distdir = tempdir();
    let portage_tmpdir = tempdir();
    let (entries, repos) = bootstrap_plan(&root);
    do_merge(&root, &distdir, &portage_tmpdir, &entries[0..1], &repos, 1);
    let boot_use = read_use(&root, "usr/share/w753/w753otherpkg-boot-use.txt");
    assert!(
        boot_use.contains("USE=") && !use_flags(&boot_use).contains("useboh"),
        "boot build ran without the reduced flag, got {boot_use:?}"
    );
    assert!(
        use_flags(&boot_use).contains("useblah"),
        "boot build keeps the other flags, got {boot_use:?}"
    );
    assert!(
        read_use(&root, "usr/share/w753/w753otherpkg-definitive-use.txt").is_empty(),
        "the definitive build never ran in the boot-only ROOT"
    );
    let vdb_use = read_use(&root, "var/db/pkg/dev-libs/w753otherpkg-1/USE");
    assert!(
        !use_flags(&vdb_use).contains("useboh"),
        "boot-only VDB USE lacks the reduced flag, got {vdb_use:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// C1: the 3-row plan merges to completion and the survivor carries the
/// declared USE, under both the serial engine (`-j1`) and the parallel
/// scheduler (`-j2`). The boot marker is gone by design (same-PF
/// reinstall unmerge), so only the definitive marker is asserted.
#[test]
fn bootstrap_three_row_plan_merges_fully_with_the_declared_survivor() {
    for jobs in [1, 2] {
        let root = tempdir();
        let distdir = tempdir();
        let portage_tmpdir = tempdir();
        let (entries, repos) = bootstrap_plan(&root);
        do_merge(&root, &distdir, &portage_tmpdir, &entries, &repos, jobs);
        let definitive_use = read_use(&root, "usr/share/w753/w753otherpkg-definitive-use.txt");
        assert!(
            use_flags(&definitive_use).contains("useboh"),
            "jobs={jobs}: definitive build ran with the declared USE, got {definitive_use:?}"
        );
        let pkgnew_use = read_use(&root, "usr/share/w753/w753pkgnew-definitive-use.txt");
        assert!(
            use_flags(&pkgnew_use).contains("usefoo"),
            "jobs={jobs}: middle row merged, got {pkgnew_use:?}"
        );
        let vdb_use = read_use(&root, "var/db/pkg/dev-libs/w753otherpkg-1/USE");
        let vdb_flags = use_flags(&vdb_use);
        assert!(
            vdb_flags.contains("useboh") && vdb_flags.contains("useblah"),
            "jobs={jobs}: survivor VDB USE equals the declared build, got {vdb_use:?}"
        );
        let vdb_new = read_use(&root, "var/db/pkg/dev-libs/w753pkgnew-1/USE");
        assert!(
            use_flags(&vdb_new).contains("use3"),
            "jobs={jobs}: middle row VDB landed, got {vdb_new:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}
