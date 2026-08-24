use super::*;

use portage_util::TempDir;

fn binary_entry(package: &str, binary: bool) -> portage_repo::GraphEntry {
    use portage_repo::{CandidateSource, PretendOutcome, VisibilityProvenance};
    portage_repo::GraphEntry {
        discovery: 0,
        category: "dev-libs".to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: "1.0".to_string(),
        },
        blockers: Vec::new(),
        slot: Some("0".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: if binary {
            CandidateSource::Binary
        } else {
            CandidateSource::Ebuild
        },
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

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}
