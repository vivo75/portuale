/// Direct unit tests for the eight pure selection predicates: each
/// test observes the predicate's own return value.
use crate::*;
use portage_util::TempDir;

/// Fixed "amd64"-only, no-overrides config, mirroring `tests`'
/// own `test_config` (visibility is pinned the same way so a
/// `KEYWORDS="amd64"` scratch ebuild is always visible).
fn cfg_162() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

/// Direct `Candidate` literal for the visibility-free legs (mirrors
/// the existing `candidate()` helper's full field set).
fn cand_162(version: &str, keywords: &[&str], slot: &str) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        slot: slot.to_string(),
        sub_slot: slot.to_string(),
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
