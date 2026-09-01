//! [`MergeEngine`] adapters executing the actual source/binary merges: a
//! source package through the `EbuildBuild` chain, a `"binary"` one through
//! binpkg unpacking. Each [`MergeUnit`] derives from its resolved
//! [`GraphEntry`] via [`merge_unit_for_entry`]; the engine seam owns
//! per-unit execution, not DAG scheduling.
// Portage: MergeListItem.py, _start

use mrg_director::{MergeContext, MergeEngine, MergeKind, MergeOutcome, MergeUnit};
use portage_repo::{CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// One resolved [`GraphEntry`] as a director [`MergeUnit`]. `None` for
/// `AlreadyInstalled` (a silent no-op) and `NoVisibleCandidate` (nothing to
/// merge; the caller reports it).
pub fn merge_unit_for_entry(
    entry: &GraphEntry,
    root: &Path,
    progress: mrg_director::MergeProgress,
) -> Option<MergeUnit> {
    let version = match &entry.outcome {
        // A blocker-removal task is not a merge unit.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => {
            return None;
        }
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
    };
    let kind = match entry.source {
        CandidateSource::Binary => MergeKind::Binary,
        CandidateSource::Ebuild => MergeKind::Source,
    };
    Some(MergeUnit {
        cpv: format!("{}/{}-{version}", entry.category, entry.package),
        kind,
        repo: entry.repo_name.clone(),
        root: root.to_path_buf(),
        replaces_same_slot: None,
        remote_binary: entry.remote_binary,
        build_id: entry.build_id.clone(),
        slot: entry.slot.clone(),
        sub_slot: entry.sub_slot.clone(),
        use_flags: entry.use_flags_display.clone(),
        progress,
    })
}

/// Rebuild the [`GraphEntry`] a merge function needs from a [`MergeUnit`].
/// The outcome is always `New { version }`: the merge functions only read
/// the version out of it, and display-only fields are never consulted
/// mid-merge. `None` when the unit's `cpv` does not parse.
fn rebuild_entry(unit: &MergeUnit, source: CandidateSource) -> Option<GraphEntry> {
    let candidate = portage_dep::parse_candidate(&unit.cpv)?;
    // The `-rN` revision is split off `version`; the merge reads the full
    // `package-version[-rN]`.
    let version = match &candidate.revision {
        Some(r) => format!("{}-r{r}", candidate.version),
        None => candidate.version.clone(),
    };
    Some(GraphEntry {
        discovery: 0,
        category: candidate.category,
        package: candidate.package,
        outcome: PretendOutcome::New { version },
        blockers: Vec::new(),
        slot: unit.slot.clone(),
        sub_slot: unit.sub_slot.clone(),
        repo_name: unit.repo.clone(),
        oldbest: Vec::new(),
        use_flags_display: unit.use_flags.clone(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: unit.remote_binary,
        build_id: unit.build_id.clone(),
        deps: Vec::new(),
    })
}

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
