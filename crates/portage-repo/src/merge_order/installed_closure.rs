//! Installed packages pulled into the merge-order graph.

use crate::merge_order::*;

/// Whether an installed package matches `edge`'s atom. For a slot-operator edge
/// the match must carry the resolved child's own slot/sub-slot.
/// `child_slot` `None` means no resolved child (plain atom check).
pub(crate) fn edge_satisfied_with(
    installed_by_cp: &HashMap<(String, String), Vec<String>>,
    edge: &DepEdge,
    child_slot: Option<(&str, &str)>,
) -> bool {
    let Some(cands) = installed_by_cp.get(&(edge.category.clone(), edge.package.clone())) else {
        return false;
    };
    let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(&edge.atom, &refs) else {
        return false;
    };
    if matched.is_empty() {
        return false;
    }
    if edge.priority.buildtime_slot_op || edge.priority.runtime_slot_op {
        let Some((slot, sub_slot)) = child_slot else {
            return true;
        };
        return matched.iter().any(|m| {
            portage_dep::parse_candidate(m).is_some_and(|c| {
                c.slot.as_deref() == Some(slot) && c.sub_slot.as_deref() == Some(sub_slot)
            })
        });
    }
    true
}

/// A synthetic `AlreadyInstalled` node for an installed package the resolve never
/// made an entry for. Filtered back out of the scheduled list before returning.
pub(crate) fn synthetic_installed_entry(
    category: String,
    package: String,
    version: String,
    deps: Vec<DepEdge>,
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category,
        package,
        outcome: PretendOutcome::AlreadyInstalled { version },
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
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
