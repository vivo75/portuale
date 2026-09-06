use super::*;

use portage_util::TempDir;

fn prio(word: u64) -> DepPriority {
    DepPriority {
        buildtime: word & 1 != 0,
        runtime: word & 2 != 0,
        runtime_post: word & 4 != 0,
        buildtime_slot_op: word & 8 != 0,
        runtime_slot_op: word & 16 != 0,
        optional: word & 32 != 0,
        satisfied: word & 64 != 0,
    }
}

/// A minimal `New` `GraphEntry` for `build_digraph` tests --
/// mirrors `synthetic_installed_entry`'s full field list.
fn new_entry(category: &str, package: &str, version: &str, deps: Vec<DepEdge>) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: Some(version.to_string()),
        sub_slot: Some(version.to_string()),
        repo_name: None,
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
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps,
    }
}

// -----------------------------------------------------------------
// Dep-key / priority mapping
// -----------------------------------------------------------------

fn dep_metadata(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn tokens(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn plain_edge(atom: &str, category: &str, package: &str, priority: DepPriority) -> DepEdge {
    DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: category.to_string(),
        package: package.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: 3,
    }
}

fn fixtures_vdb_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}
mod debug;
