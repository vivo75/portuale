use super::*;

/// #134 (Phase 5b S4): an installed `Child:` row's `USE="…"`
/// column must carry the vdb-recorded USE, not `""`. The fixture's
/// vdb is `IUSE=flip` / `USE=` (built -flip), so
/// `installed_use_str` renders `-flip` -- the minimum bar S0's
/// capture sets (`port-rest-debug.log:5990`'s shape). Real's
/// parenthesised / USE_EXPAND / cpv-suffix richness is a recorded
/// residual, not this pin's subject.
#[test]
fn installed_child_row_displays_the_vdb_use_not_an_empty_string() {
    use crate::resolver_trace::child_use_str;
    let root = fixtures_root();
    let parent = GraphEntry {
        discovery: 0,
        category: "dev-libs".to_string(),
        package: "deepusedepparent".to_string(),
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        },
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
        build_time: None,
        deps: Vec::new(),
    };
    assert_eq!(child_use_str(&parent, &root), "-flip");
}
