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

/// A run that stops after the boot row merged leaves the reduced build
/// installed: one line names the package, the removed flag and the command
/// that restores it. Once the definitive row has merged, nothing is said.
#[test]
fn an_interrupted_bootstrap_names_the_reduced_install_and_its_restore_command() {
    let root = tempdir();
    let distdir = tempdir();
    let portage_tmpdir = tempdir();
    let (entries, repos) = bootstrap_plan(&root);
    assert!(
        interrupted_bootstrap_notes(root.path(), &entries).is_empty(),
        "nothing merged yet"
    );
    do_merge(&root, &distdir, &portage_tmpdir, &entries[0..1], &repos, 1);
    let notes = interrupted_bootstrap_notes(root.path(), &entries);
    assert_eq!(notes.len(), 1, "{notes:?}");
    for part in [
        "dev-libs/w753otherpkg-1",
        "useboh",
        "emerge --oneshot =dev-libs/w753otherpkg-1",
    ] {
        assert!(notes[0].contains(part), "{part} in {:?}", notes[0]);
    }
    do_merge(&root, &distdir, &portage_tmpdir, &entries[2..3], &repos, 1);
    assert!(
        interrupted_bootstrap_notes(root.path(), &entries).is_empty(),
        "the definitive build replaced the reduced one"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The plan's boot row is saved as one nested mark, and the resume list
/// rebuilt from the saved text marks the first row of that cpv only: the
/// resumed boot row executes with the reduced USE, the definitive row does
/// not carry the marker.
#[test]
fn a_resumed_boot_row_keeps_its_marker_and_runs_with_the_reduced_use() {
    let root = tempdir();
    let distdir = tempdir();
    let portage_tmpdir = tempdir();
    let (entries, repos) = bootstrap_plan(&root);
    let list: Vec<_> = entries.iter().filter_map(resume_cpv).collect();
    let marks = boot_marks(&entries);
    assert_eq!(
        marks.iter().collect::<Vec<_>>(),
        vec![(
            &"dev-libs/w753otherpkg-1".to_string(),
            &"useboh".to_string()
        )]
    );
    crate::mtimedb::write_resume_list_marked(
        root.path(),
        &[],
        &list,
        &crate::mtimedb::ResumeOpts::default(),
        &marks,
    )
    .unwrap();
    let (_, saved, _) = crate::mtimedb::read_resume_list(root.path()).unwrap();
    let mut resumed: Vec<GraphEntry> = saved
        .iter()
        .map(|(_, c, p, v)| {
            resume_entry(ResumeEntryArgs {
                category: c,
                package: p,
                version: v,
                source: CandidateSource::Ebuild,
                repos: &repos,
                binary_repo: None,
            })
        })
        .collect();
    apply_boot_marks(
        &mut resumed,
        &crate::mtimedb::read_resume_bootstrap(root.path()),
    );
    assert_eq!(
        resumed
            .iter()
            .map(|e| e.bootstrap.as_ref().map(|b| b.reduced_flag.as_str()))
            .collect::<Vec<_>>(),
        vec![Some("useboh"), None, None]
    );
    do_merge(&root, &distdir, &portage_tmpdir, &resumed[0..1], &repos, 1);
    let boot_use = read_use(&root, "usr/share/w753/w753otherpkg-boot-use.txt");
    assert!(
        boot_use.contains("USE=") && !use_flags(&boot_use).contains("useboh"),
        "the resumed boot row ran with the reduced USE, got {boot_use:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// After the boot row merged, the definitive row of the same cpv is still
/// unmerged (the installed USE lacks the reduced flag); the boot row is not.
/// Once the definitive build lands nothing is left.
#[test]
fn the_definitive_row_is_not_merged_just_because_the_boot_row_was() {
    let root = tempdir();
    let distdir = tempdir();
    let portage_tmpdir = tempdir();
    let (entries, repos) = bootstrap_plan(&root);
    let names = |root: &TempDir| -> Vec<String> {
        crate::pretend::entries_not_merged(root.path(), &entries)
            .into_iter()
            .map(|(_, _, p, _)| p)
            .collect()
    };
    let save = |root: &TempDir| {
        crate::pretend::unmerged_for_resume_save(
            root.path(),
            &entries,
            &boot_marks(&entries),
            &MergePassReport::default(),
        )
    };
    assert_eq!(names(&root).len(), 3, "nothing merged yet");
    assert_eq!(
        save(&root).boot.len(),
        1,
        "the failure re-save keeps the mark"
    );
    do_merge(&root, &distdir, &portage_tmpdir, &entries[0..1], &repos, 1);
    assert_eq!(names(&root), vec!["w753pkgnew", "w753otherpkg"]);
    let after_boot = save(&root);
    assert_eq!(after_boot.mergelist.len(), 2);
    assert_eq!(
        after_boot.boot.len(),
        1,
        "the definitive row still owes its mark"
    );
    do_merge(&root, &distdir, &portage_tmpdir, &entries[1..], &repos, 1);
    assert!(names(&root).is_empty(), "{:?}", names(&root));
    assert!(save(&root).boot.is_empty(), "definitive landed: no mark");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// A resumed list `[middle, P]` whose boot row already merged: the single P
/// row is the definitive build still pending. Applying the saved mark puts
/// no boot marker on it, a failure at the middle row re-saves P together
/// with the mark, and once the definitive build lands P is merged and the
/// mark is not carried on.
#[test]
fn a_resume_after_the_boot_row_still_owes_the_definitive_build() {
    let root = tempdir();
    let distdir = tempdir();
    let portage_tmpdir = tempdir();
    let (entries, repos) = bootstrap_plan(&root);
    do_merge(&root, &distdir, &portage_tmpdir, &entries[0..1], &repos, 1);
    let marks = boot_marks(&entries);
    let mut resumed: Vec<GraphEntry> = entries[1..].to_vec();
    for e in &mut resumed {
        e.bootstrap = None;
    }
    apply_boot_marks(&mut resumed, &marks);
    assert!(resumed.iter().all(|e| e.bootstrap.is_none()));
    let save = crate::pretend::unmerged_for_resume_save(
        root.path(),
        &resumed,
        &marks,
        &MergePassReport::default(),
    );
    assert_eq!(
        save.mergelist.len(),
        2,
        "P still unmerged: {:?}",
        save.mergelist
    );
    assert_eq!(save.boot, marks, "the mark stays with the pending P");
    do_merge(&root, &distdir, &portage_tmpdir, &entries[1..], &repos, 1);
    let save = crate::pretend::unmerged_for_resume_save(
        root.path(),
        &resumed,
        &marks,
        &MergePassReport::default(),
    );
    assert!(
        save.mergelist.is_empty() && save.boot.is_empty(),
        "{:?}",
        save.mergelist
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}
