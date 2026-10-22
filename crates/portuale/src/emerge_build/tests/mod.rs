use super::*;

use portage_repo::find_repos;

use portage_util::TempDir;

use std::fs;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn tempdir() -> std::path::PathBuf {
    let dir = TempDir::new("emerge_build_test").keep();
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn live_test_entry() -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "propertiespkg".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        blockers: vec![],
        slot: None,
        sub_slot: None,
        repo_name: None,
        oldbest: vec![],
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        build_time: None,
        deps: Vec::new(),
    }
}

fn source_entry(package: &str, outcome: PretendOutcome) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: package.into(),
        outcome,
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        oldbest: vec![],
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        build_time: None,
        deps: Vec::new(),
    }
}

fn buildpkgonly_entry(category: &str, package: &str, version: &str) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: category.into(),
        package: package.into(),
        outcome: PretendOutcome::New {
            version: version.into(),
        },
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        oldbest: vec![],
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        build_time: None,
        deps: Vec::new(),
    }
}

mod buildpkgonly;
mod entries;
mod entry_env;
mod logs;
mod resume;
mod scheduler;
mod source_merge;
mod status;
