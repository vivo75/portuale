use super::*;

#[test]
fn slot_conflict_mask_choices_groups_missed_update_siblings() {
    // `mgfc` conflict as the `mgfa` walk records it: 2.0 pulled by
    // `mgfb-2`'s `=mgfc-2`, 1.0 by `mgfa`'s and `mgfb-1`'s `=mgfc-1`;
    // `mgfc-3.0` visible in the repo but pulled by nothing. The 2.0
    // choice groups 3.0 (both `=1.0` parents miss it); the 1.0 choice
    // groups 3.0 too (`=2.0` misses it). Longest-conflict first, so
    // the 2.0 choice sorts last and DFS explores it first.
    let repos = find_repos(&fixtures_root()).expect("fixture repos resolve");
    let config = test_config();
    let puller = |pkg: &str, ver: &str, atom: &str| {
        (
            "dev-libs".to_string(),
            pkg.to_string(),
            ver.to_string(),
            atom.to_string(),
        )
    };
    let mut pullers: SlotPullers = HashMap::new();
    pullers.insert(
        ("dev-libs".to_string(), "mgfc".to_string()),
        vec![
            puller("mgfb", "2", "=dev-libs/mgfc-2"),
            puller("mgfa", "1", "=dev-libs/mgfc-1"),
            puller("mgfb", "1", "=dev-libs/mgfc-1"),
        ],
    );
    let instance = |ver: &str| SlotConflictInstance {
        version: ver.to_string(),
        sub_slot: "0".to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed: false,
    };
    let sc = SlotConflict {
        category: "dev-libs".to_string(),
        package: "mgfc".to_string(),
        slot: "0".to_string(),
        resolved_version: "2".to_string(),
        conflicting_atom: "=dev-libs/mgfc-1".to_string(),
        instances: vec![instance("2"), instance("1")],
    };
    let choices = slot_conflict_mask_choices(SlotConflictMaskChoicesArgs {
        repos: &repos,
        excluded: &[],
        params: &BacktrackParams::default(),
        slot_pullers: &pullers,
        sc: &sc,
        config: &config,
    });
    assert_eq!(choices.len(), 2);
    // Ascending length: the 1.0 choice (one parent) first.
    assert_eq!(
        choices[0].target,
        ("dev-libs".to_string(), "mgfc".to_string(), "1".to_string())
    );
    assert_eq!(choices[0].parents.len(), 1);
    assert_eq!(choices[0].similar.len(), 1);
    assert_eq!(
        choices[0].similar[0].target,
        (
            "dev-libs".to_string(),
            "mgfc".to_string(),
            "3.0".to_string()
        )
    );
    assert_eq!(choices[0].similar[0].parents.len(), 1);
    assert_eq!(
        choices[1].target,
        ("dev-libs".to_string(), "mgfc".to_string(), "2".to_string())
    );
    assert_eq!(choices[1].parents.len(), 2);
    assert_eq!(choices[1].similar.len(), 1);
    assert_eq!(
        choices[1].similar[0].target,
        (
            "dev-libs".to_string(),
            "mgfc".to_string(),
            "3.0".to_string()
        )
    );
    assert_eq!(choices[1].similar[0].parents.len(), 2);
    // Content, not just counts: the 2.0 choice is opposed by both
    // `=1.0` pulls (neither matches 2.0), and so is its 3.0 sibling;
    // the 1.0 choice and its sibling answer only `=2.0`.
    fn atoms(ps: &[((String, String, String), String)]) -> Vec<String> {
        let mut v: Vec<String> = ps.iter().map(|(_, a)| a.clone()).collect();
        v.sort();
        v
    }
    assert_eq!(
        atoms(&choices[1].parents),
        ["=dev-libs/mgfc-1", "=dev-libs/mgfc-1"]
    );
    assert_eq!(
        atoms(&choices[1].similar[0].parents),
        ["=dev-libs/mgfc-1", "=dev-libs/mgfc-1"]
    );
    assert_eq!(atoms(&choices[0].parents), ["=dev-libs/mgfc-2"]);
    assert_eq!(atoms(&choices[0].similar[0].parents), ["=dev-libs/mgfc-2"]);
}

/// Backlog #161 S5: an already-masked sibling never joins the mask
/// group -- over `dev-libs/mgfc` (three same-slot versions) the
/// masked non-party 1 stays out of 3.0's group.
#[test]
fn slot_conflict_mask_choices_skips_an_already_masked_sibling() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let sc = SlotConflict {
        category: "dev-libs".to_string(),
        package: "mgfc".to_string(),
        slot: "0".to_string(),
        resolved_version: "3.0".to_string(),
        conflicting_atom: "<dev-libs/mgfc-3.0".to_string(),
        instances: vec![
            SlotConflictInstance {
                version: "3.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
            SlotConflictInstance {
                version: "2.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
        ],
    };
    let pullers: SlotPullers = HashMap::from([(
        ("dev-libs".to_string(), "mgfc".to_string()),
        vec![
            (
                "dev-libs".to_string(),
                "sounneed".to_string(),
                "1.0".to_string(),
                "<dev-libs/mgfc-3.0".to_string(),
            ),
            // Matches 2 only: without it the masked sibling shares
            // every conflict atom and the subset gate, not the mask
            // check, would exclude it.
            (
                "dev-libs".to_string(),
                "sounneed".to_string(),
                "1.0".to_string(),
                "=dev-libs/mgfc-2".to_string(),
            ),
        ],
    )]);
    let mut params = BacktrackParams::default();
    params.runtime_pkg_mask.insert(
        ("dev-libs".to_string(), "mgfc".to_string()),
        vec![MaskEntry {
            neg: "!=dev-libs/mgfc-1".to_string(),
            reason: MaskReason::MissingDependency {
                parent: ("dev-libs".to_string(), "sounneed".to_string()),
                atom: "<dev-libs/mgfc-3.0".to_string(),
            },
        }],
    );
    let choices = slot_conflict_mask_choices(SlotConflictMaskChoicesArgs {
        repos: &repos,
        excluded: &[],
        params: &params,
        slot_pullers: &pullers,
        sc: &sc,
        config: &config,
    });
    assert!(!choices.is_empty());
    assert!(
        choices
            .iter()
            .all(|c| c.similar.iter().all(|s| s.target.2 != "1")),
        "the masked 1 sibling never joins a mask group"
    );
    let _ = root;
}
