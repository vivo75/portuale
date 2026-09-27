use super::*;

#[test]
fn resume_cpv_maps_source_and_binary_and_skips_non_merges() {
    // The up-front resume save records every entry that will actually
    // merge -- source and binary alike -- and nothing else.
    let src = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "src-pkg",
        version: "1.0",
        source: CandidateSource::Ebuild,
        repos: &[],
        binary_repo: None,
    });
    assert_eq!(
        resume_cpv(&src),
        Some((
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "src-pkg".to_string(),
            "1.0".to_string(),
        ))
    );
    let bin = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "bin-pkg",
        version: "2.0",
        source: CandidateSource::Binary,
        repos: &[],
        binary_repo: None,
    });
    assert_eq!(
        resume_cpv(&bin),
        Some((
            crate::mtimedb::ResumeEntryKind::Binary,
            "dev-libs".to_string(),
            "bin-pkg".to_string(),
            "2.0".to_string(),
        ))
    );
    let mut installed = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "old-pkg",
        version: "3.0",
        source: CandidateSource::Ebuild,
        repos: &[],
        binary_repo: None,
    });
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "3.0".into(),
    };
    assert_eq!(resume_cpv(&installed), None);
    let mut uninstalled = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "gone-pkg",
        version: "4.0",
        source: CandidateSource::Ebuild,
        repos: &[],
        binary_repo: None,
    });
    uninstalled.outcome = PretendOutcome::NoVisibleCandidate;
    assert_eq!(resume_cpv(&uninstalled), None);
}

#[test]
fn resume_entry_uses_the_binary_repo_for_binary_entries_only() {
    // `binary_repo` wins for a `Binary` entry, is ignored for an `Ebuild`
    // entry, and `None` keeps the ebuild fallback.
    let bin = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "bin-pkg",
        version: "2.0",
        source: CandidateSource::Binary,
        repos: &[],
        binary_repo: Some("binhostrepo"),
    });
    assert_eq!(bin.repo_name.as_deref(), Some("binhostrepo"));
    let src = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "src-pkg",
        version: "1.0",
        source: CandidateSource::Ebuild,
        repos: &[],
        binary_repo: Some("binhostrepo"),
    });
    assert_eq!(src.repo_name, None);
    let fallback = resume_entry(ResumeEntryArgs {
        category: "dev-libs",
        package: "bin-pkg",
        version: "2.0",
        source: CandidateSource::Binary,
        repos: &[],
        binary_repo: None,
    });
    assert_eq!(fallback.repo_name, None);
}

#[test]
fn run_merge_loop_shrinks_the_saved_resume_list_per_successful_merge() {
    // With the full mergelist saved up front, each successful merge removes
    // exactly that entry, so a SIGKILL mid-run leaves the tail and the last
    // merge deletes the key.
    let root = tempdir();
    let entries = vec![
        resume_entry(ResumeEntryArgs {
            category: "dev-libs",
            package: "loop-a",
            version: "1.0",
            source: CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
        resume_entry(ResumeEntryArgs {
            category: "dev-libs",
            package: "loop-b",
            version: "1.0",
            source: CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
        resume_entry(ResumeEntryArgs {
            category: "dev-libs",
            package: "loop-c",
            version: "1.0",
            source: CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
    ];
    let full: Vec<crate::mtimedb::ResumeCpv> =
        entries.iter().map(|e| resume_cpv(e).unwrap()).collect();
    crate::mtimedb::write_resume_list(
        &root,
        &["dev-libs/loop-a"],
        &full,
        &crate::mtimedb::ResumeOpts::default(),
    )
    .unwrap();

    run_merge_loop(&entries, false, &root, |_, entry| {
        // By the time this entry's own merge runs, every earlier entry
        // has already left the on-disk list.
        let pos = full
            .iter()
            .position(|(_, _, p, _)| p == &entry.package)
            .expect("loop entry is in the saved list");
        let (_, remaining, _) =
            crate::mtimedb::read_resume_list(&root).expect("resume list mid-run");
        assert_eq!(remaining, full[pos..]);
        Ok(())
    })
    .unwrap();

    assert!(crate::mtimedb::read_resume_list(&root).is_none());
    let _ = fs::remove_dir_all(&root);
}
