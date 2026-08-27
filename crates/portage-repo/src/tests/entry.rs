use super::*;

/// `skipped_update_roots` attributes the miss to the root the rejecting edge
/// points at, and a merge parent to its entry's root.
#[test]
fn skipped_update_roots_attributes_miss_and_parent_per_edge() {
    fn entry(package: &str, version: &str, running: bool, deps: Vec<DepEdge>) -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-libs".to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: version.to_string(),
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
            source: CandidateSource::Ebuild,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: running,
            remote_binary: false,
            build_id: None,
            deps,
        }
    }
    fn edge(child: &str, atom: &str, key: u8) -> DepEdge {
        DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string(),
            category: "dev-libs".to_string(),
            package: child.to_string(),
            priority: DepPriority::default(),
            disjunctive: false,
            alt: None,
            key,
        }
    }
    // Cross-active: target parent plus a running entry elsewhere.
    let running_twin = entry("blk0x", "1", true, Vec::new());
    let parent = entry(
        "blk0b",
        "1",
        false,
        vec![edge("blk0x", "<dev-libs/blk0x-2", 3)],
    );
    let entries = vec![running_twin, parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "<dev-libs/blk0x-2",
        ),
        (true, false),
        "DEPEND-rejected miss lands running, target parent stays target",
    );
    // Same edges, single-rooted: everything attributes target.
    let parent = entry(
        "blk0b",
        "1",
        false,
        vec![edge("blk0x", "<dev-libs/blk0x-2", 3)],
    );
    let entries = vec![parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "<dev-libs/blk0x-2",
        ),
        (false, false),
        "single-root runs keep the target attribution",
    );
    // A runtime edge keeps the miss in the parent's (running) root.
    let parent = entry("blk0b", "1", true, vec![edge("blk0x", "dev-libs/blk0x", 0)]);
    let entries = vec![parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "dev-libs/blk0x",
        ),
        (true, true),
        "RDEPEND keeps the miss in the parent's (running) root",
    );
    // Unknown parent: both target.
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "nope", "1"),
            "x",
        ),
        (false, false),
    );
}
