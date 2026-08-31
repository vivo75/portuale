use super::*;

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
