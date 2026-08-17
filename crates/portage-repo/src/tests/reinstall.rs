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

#[test]
fn best_binary_build_time_is_the_newest_binary_of_the_version() {
    let mk = |source, ver: &str, bt: Option<i64>| Candidate {
        version: ver.to_string(),
        keywords: vec![],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "gentoo".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: bt,
        binary_deps: HashMap::new(),
    };
    let cands = vec![
        mk(CandidateSource::Ebuild, "1.0", None),
        mk(CandidateSource::Binary, "1.0", Some(100)),
        mk(CandidateSource::Binary, "1.0", Some(300)),
        mk(CandidateSource::Binary, "2.0", Some(999)),
    ];
    assert_eq!(best_binary_build_time(&cands, "1.0"), Some(300));
    assert_eq!(best_binary_build_time(&cands, "3.0"), None);
}

#[test]
fn split_slot_defaults_sub_slot_to_the_slot_itself_when_no_slash_is_present() {
    assert_eq!(split_slot("0"), ("0".to_string(), "0".to_string()));
    assert_eq!(split_slot("0/2"), ("0".to_string(), "2".to_string()));
    assert_eq!(split_slot(""), ("0".to_string(), "0".to_string()));
}
