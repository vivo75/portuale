/// Direct unit tests for the eight pure selection predicates: each
/// test observes the predicate's own return value.
use crate::*;
use portage_util::TempDir;

/// Minimal merge-bound `GraphEntry` for the predicate legs (slot
/// and `new_slot` as the leg needs; everything display-side left
/// at its empty default).
fn entry_162(
    cat: &str,
    pkg: &str,
    outcome: PretendOutcome,
    slot: Option<&str>,
    new_slot: bool,
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: slot.map(str::to_string),
        sub_slot: slot.map(str::to_string),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

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
