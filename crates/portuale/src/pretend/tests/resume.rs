use super::*;

#[test]
fn resume_pretend_end_notice_prints_once_like_portage() {
    // Portage: actions.py, post_emerge.py. `--resume --pretend` prints the end notice exactly once.
    let portuale_bin = built_portuale_bin();
    let (root, _) = stale_resume_root("news_pretend");
    let tmp = TempDir::new("resume_news_pretend").keep();
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--resume", "--pretend"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(news_resolve_env(&root, &tmp.join("pt-resume-news")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dev-libs/stale-a-1"), "{stdout}");
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resume_skipfirst_empty_list_prints_only_the_pre_notice_like_portage() {
    // Portage: actions.py. The pre-resolution notice precedes resume handling;
    // the post notice needs a changed vdb.
    let portuale_bin = built_portuale_bin();
    let root = single_resume_root("news_empty");
    let tmp = TempDir::new("resume_news_empty").keep();
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--resume", "--skipfirst"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(news_resolve_env(&root, &tmp.join("pt-resume-empty")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("*** Resuming merge..."), "{stdout}");
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pretend_writes_no_resume_list_even_with_a_stale_list_present() {
    // Portage: actions.py. `--pretend` returns before the scheduler, so it
    // never touches the resume list -- but the updates run still records
    // its mtimes (the S0 oracle: host Portage commits `mtimedb["updates"]`
    // on a run whose resolve never happened).
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
        !crate::mtimedb::read_updates_mtimes(&bare).is_empty(),
        "--pretend records the updates mtimes"
    );
    assert!(
        crate::mtimedb::read_resume_list(&bare).is_none(),
        "--pretend must not write a resume list"
    );

    let (root, _) = stale_resume_root("pretend");
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
        crate::mtimedb::read_resume_list(&root)
            .map(|(favorites, mergelist, _)| (favorites, mergelist)),
        Some((
            vec!["dev-libs/stale-a".to_string()],
            vec![
                (
                    crate::mtimedb::ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "stale-a".to_string(),
                    "1".to_string(),
                ),
                (
                    crate::mtimedb::ResumeEntryKind::Ebuild,
                    "dev-libs".to_string(),
                    "stale-b".to_string(),
                    "1".to_string(),
                ),
            ],
        )),
        "--pretend must leave a stale resume list alone"
    );
    assert!(
        !crate::mtimedb::read_updates_mtimes(&root).is_empty(),
        "--pretend still records the updates mtimes"
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

#[test]
fn resume_display_marks_an_installed_cpv_as_reinstall() {
    // Portage re-resolves the saved mergelist (`resume_depgraph`), so
    // `--resume --pretend` renders an already-installed entry `[ebuild R]`
    // (a failed `pkg_postinst` lands the vdb entry) and an uninstalled one
    // `[ebuild N]`. The saved list pins `=cpv`, so one `CONTENTS` check is
    // the whole re-resolution.
    let root = TempDir::new("pretend-test-resume_display_reinstall").keep();
    let _ = std::fs::remove_dir_all(&root);
    let landed = root.join("var/db/pkg/dev-libs/landed-1.0");
    std::fs::create_dir_all(&landed).unwrap();
    std::fs::write(landed.join("CONTENTS"), "").unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let entries = vec![
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "landed",
            version: "1.0",
            source: portage_repo::CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "missing",
            version: "1.0",
            source: portage_repo::CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
    ];
    let display = resume_display_entries(&root, &entries);
    assert!(
        matches!(
            &display[0].outcome,
            portage_repo::PretendOutcome::Reinstall { version, .. } if version == "1.0"
        ),
        "{:?}",
        display[0].outcome
    );
    assert_eq!(display[1].outcome, entries[1].outcome);
    // The merge entries themselves are untouched: the retry rebuilds.
    assert!(matches!(
        &entries[0].outcome,
        portage_repo::PretendOutcome::New { .. }
    ));
    let _ = std::fs::remove_dir_all(&root);
}
