use super::*;

#[test]
fn candidate_iuse_and_use_reads_a_binarys_own_baked_use() {
    // A binary candidate carries its built-with flags over its own
    // `iuse` (Portage `pkg.use.enabled` for a built package).
    let c = Candidate {
        version: "1.0".to_string(),
        keywords: vec!["amd64".to_string()],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: i32::MIN,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: "feat other".to_string(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Binary,
        binary_use: Some(HashSet::from(["feat".to_string()])),
        remote: true,
        build_id: Some("1".to_string()),
        build_time: Some(1),
        binary_deps: HashMap::new(),
    };
    let (iuse, use_flags) = candidate_iuse_and_use(&c, "dev-libs", "x", &test_config()).unwrap();
    assert_eq!(
        iuse,
        HashSet::from(["feat".to_string(), "other".to_string()])
    );
    assert_eq!(*use_flags, HashSet::from(["feat".to_string()]));
}
