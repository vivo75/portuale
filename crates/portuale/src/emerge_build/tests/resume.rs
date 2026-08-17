use super::*;

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
