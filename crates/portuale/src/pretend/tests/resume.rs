use super::*;

#[test]
fn pretend_writes_no_resume_list_even_with_a_stale_list_present() {
    // Portage: actions.py. `--pretend` returns before the scheduler, so it
    // never touches the resume list.
    let portuale_bin = built_portuale_bin();
    let tmp = TempDir::new("resume_no_write_pretend").keep();

    let bare = tmp.join("bare");
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--pretend", "--oneshot", "dev-libs/schedok"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(fixture_resolve_env(&bare, &tmp.join("pt-bare")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("dev-libs/schedok-1.0"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !crate::mtimedb::mtimedb_path(&bare).exists(),
        "--pretend must not create an mtimedb"
    );

    let (root, before) = stale_resume_root("pretend");
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--pretend", "--oneshot", "dev-libs/schedok"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(fixture_resolve_env(&root, &tmp.join("pt-stale")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(crate::mtimedb::mtimedb_path(&root)).unwrap(),
        before,
        "--pretend must leave a stale resume list byte-identical"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn entries_not_merged_tags_and_includes_both_source_and_binary_entries() {
    // The resume list can hold source and binary entries together; both are tagged.
    let root = TempDir::new("pretend-test-entries_not_merged_binary").keep();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let source_entry = emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
        category: "dev-libs",
        package: "src-pkg",
        version: "1.0",
        source: portage_repo::CandidateSource::Ebuild,
        repos: &[],
        binary_repo: None,
    });
    let binary_entry = emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
        category: "dev-libs",
        package: "bin-pkg",
        version: "2.0",
        source: portage_repo::CandidateSource::Binary,
        repos: &[],
        binary_repo: None,
    });
    let entries = vec![source_entry, binary_entry];

    let unmerged = entries_not_merged(&root, &entries);
    assert_eq!(unmerged.len(), 2);
    assert_eq!(
        unmerged[0],
        (
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "src-pkg".to_string(),
            "1.0".to_string(),
        )
    );
    assert_eq!(
        unmerged[1],
        (
            crate::mtimedb::ResumeEntryKind::Binary,
            "dev-libs".to_string(),
            "bin-pkg".to_string(),
            "2.0".to_string(),
        )
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn up_front_save_covers_exactly_the_failure_path_tail_when_nothing_merged() {
    // The up-front save and the failure-path tail agree when nothing merged yet.
    let root = TempDir::new("pretend-test-up_front_matches_failure_tail").keep();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let entries = vec![
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "src-pkg",
            version: "1.0",
            source: portage_repo::CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "bin-pkg",
            version: "2.0",
            source: portage_repo::CandidateSource::Binary,
            repos: &[],
            binary_repo: None,
        }),
    ];
    let up_front: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    assert_eq!(up_front.len(), 2);
    assert_eq!(entries_not_merged(&root, &entries), up_front);
    let _ = std::fs::remove_dir_all(&root);
}
