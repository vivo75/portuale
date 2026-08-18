//! [`MergeEngine`] adapters executing the actual source/binary merges: a
//! source package through the `EbuildBuild` chain, a `"binary"` one through
//! binpkg unpacking. Each [`MergeUnit`] derives from its resolved
//! [`GraphEntry`] via [`merge_unit_for_entry`]; the engine seam owns
//! per-unit execution, not DAG scheduling.
// Portage: MergeListItem.py, _start

use mrg_director::{MergeKind, MergeUnit};
use portage_repo::{CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_entry() -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-libs".to_string(),
            package: "example".to_string(),
            outcome: PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: Some("main".to_string()),
            oldbest: Vec::new(),
            use_flags_display: vec![("flag".to_string(), true)],
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
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }
}
