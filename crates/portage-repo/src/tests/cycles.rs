use super::*;

#[test]
fn find_hard_cycles_reports_a_build_time_cycle_and_ignores_a_run_time_one() {
    let hardcpv = |p: &str| format!("dev-libs/{p}-1.0");
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());

    // Pure hard cycle: reported from the earliest-inserted node; no `deps`, so no
    // slot-op arm and no vdb consulted.
    let mut hard: EdgeKindMap = HashMap::new();
    hard.insert((cp("hcb"), cp("hca")), (true, false));
    hard.insert((cp("hca"), cp("hcb")), (true, false));
    // Explicit `discovery`: hca discovered first (unit entries default to 0).
    let mut first = graph_entry("dev-libs", "hca", "1.0");
    first.discovery = 0;
    let mut second = graph_entry("dev-libs", "hcb", "1.0");
    second.discovery = 1;
    let entries = vec![first, second];
    assert_eq!(
        find_hard_cycles(
            &entries,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        ),
        vec![vec![hardcpv("hcb"), hardcpv("hca")]]
    );

    // One edge with a run-time alternative: breakable, nothing reported.
    let mut mixed: EdgeKindMap = HashMap::new();
    mixed.insert((cp("hcb"), cp("hca")), (true, false));
    mixed.insert((cp("hca"), cp("hcb")), (true, true));
    assert!(
        find_hard_cycles(
            &entries,
            &mixed,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );

    // An AlreadyInstalled node can't be in a build-time cycle.
    let installed = vec![
        graph_entry("dev-libs", "hca", "1.0"),
        GraphEntry {
            discovery: 0,
            outcome: PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string(),
            },
            ..graph_entry("dev-libs", "hcb", "1.0")
        },
    ];
    assert!(
        find_hard_cycles(
            &installed,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );
}
