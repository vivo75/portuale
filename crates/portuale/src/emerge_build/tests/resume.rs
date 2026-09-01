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
