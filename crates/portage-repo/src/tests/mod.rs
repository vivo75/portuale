use super::*;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("fixtures/ does not resolve: it is a symlink to ../pmtest/fixtures, so the sibling pmtest repo has to be checked out next to this one")
}

fn candidate(version: &str, keywords: &[&str]) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "test".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Ebuild,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: None,
        binary_deps: HashMap::new(),
    }
}
