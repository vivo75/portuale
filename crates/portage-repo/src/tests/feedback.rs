use super::*;

/// A stuck pass records an empty rebuild list instead of scanning.
#[test]
fn collect_feedback_reports_no_rebuilds_when_the_pass_is_stuck() {
    let dir = slotundo_temp_dir("161-cf-nvc");
    let d = dir.join("var/db/pkg/dev-libs/stale-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    let config = test_config();
    // Excluded so the scan stays quiet; isolates the gate below.
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "stale".to_string())]);
    let excluded = vec!["dev-libs/stale".to_string()];
    let opts = CtxOpts161 {
        reachable: reachable.clone(),
        backtrack_max: 1,
        excluded: excluded.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, Vec::new(), &opts);
    let bar_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    let mut pass = pass_161();
    pass.entries = vec![bar_upgrade];
    pass.suppressed_nvc = true;
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    assert!(
        matches!(decision, PassDecision::DeadEnd { .. }),
        "an unsatisfiable pass is a dead end"
    );
    assert_eq!(pass.abi_rebuilds, Some(Vec::new()));
    let _ = fs::remove_dir_all(&dir);
}

/// An enforced reverse pin is retained once (append then dedup legs).
#[test]
fn collect_feedback_keeps_an_enforced_pin_single() {
    let dir = slotundo_temp_dir("161-cf-enf");
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("pinner-1.0", "=dev-libs/souprov-1.0");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "pinner".to_string())]);
    let opts = CtxOpts161 {
        reachable: reachable.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let souprov_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "souprov".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        repo_name: Some("testrepo".into()),
        ..graph_entry("dev-libs", "souprov", "2.0")
    };
    // The scan finds exactly this pin.
    let pin = RevDepPin {
        cp: ("dev-libs".to_string(), "souprov".to_string()),
        atom: "=dev-libs/souprov-1.0".to_string(),
        raw_atom: "=dev-libs/souprov-1.0".to_string(),
        consumer: (
            "dev-libs".to_string(),
            "pinner".to_string(),
            "1.0".to_string(),
        ),
    };
    let mut pass = pass_161();
    pass.entries = vec![souprov_upgrade.clone()];
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::RevDep { params: out }) = decision else {
        panic!("a newly enforced pin retries with config");
    };
    assert_eq!(out.reverse_dep_pins, vec![pin.clone()]);
    let mut pass2 = pass_161();
    pass2.entries = vec![souprov_upgrade];
    let mut grown = BacktrackParams::default();
    grown.reverse_dep_pins.push(pin.clone());
    grown.reverse_dep_masked.insert((
        ("dev-libs".to_string(), "souprov".to_string()),
        "=dev-libs/souprov-1.0".to_string(),
    ));
    let decision2 = collect_feedback(&ctx, &grown, &mut pass2, &config);
    let PassDecision::Settle { params: out2 } = decision2 else {
        panic!("a quiet pass settles");
    };
    assert_eq!(out2.reverse_dep_pins, vec![pin]);
    let _ = fs::remove_dir_all(&dir);
}

/// A dropped reverse pin is collected once across the three scan sites.
#[test]
fn collect_feedback_collects_a_dropped_pin_once() {
    let dir = slotundo_temp_dir("161-cf-drop");
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("pinner-1.0", "=dev-libs/souprov-1.0");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "pinner".to_string())]);
    let opts = CtxOpts161 {
        reachable: reachable.clone(),
        backtrack_max: 1,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let souprov_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "souprov".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        repo_name: Some("testrepo".into()),
        ..graph_entry("dev-libs", "souprov", "2.0")
    };
    let pin = RevDepPin {
        cp: ("dev-libs".to_string(), "souprov".to_string()),
        atom: "=dev-libs/souprov-1.0".to_string(),
        raw_atom: "=dev-libs/souprov-1.0".to_string(),
        consumer: (
            "dev-libs".to_string(),
            "pinner".to_string(),
            "1.0".to_string(),
        ),
    };
    let hard_double = vec![
        ">=dev-libs/souprov-2.0".to_string(),
        "dev-libs/souprov".to_string(),
    ];
    let hard_single = vec![">=dev-libs/souprov-2.0".to_string()];
    let pass_for = |wants: Vec<String>| {
        let mut pass = pass_161();
        pass.entries = vec![souprov_upgrade.clone()];
        pass.slot_conflicts = vec![souprov_conflict_161()];
        pass.slot_want
            .insert(("dev-libs".to_string(), "souprov".to_string()), wants);
        pass
    };
    let mut pass_a = pass_for(hard_double);
    let decision_a = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass_a, &config);
    let PassDecision::Feedback(BacktrackFeedback::Config { params: base_a }) = decision_a else {
        panic!("a solvable conflict retries with constraints");
    };
    assert_eq!(base_a.dropped_pins, vec![pin.clone()]);
    let mut pass_b = pass_for(hard_single.clone());
    let mut grown_b = BacktrackParams::default();
    grown_b.dropped_pins.push(pin.clone());
    let decision_b = collect_feedback(&ctx, &grown_b, &mut pass_b, &config);
    let PassDecision::Feedback(BacktrackFeedback::SlotConflict { base: base_b, .. }) = decision_b
    else {
        panic!("an unsolvable conflict masks");
    };
    assert_eq!(base_b.dropped_pins, vec![pin.clone()]);
    let sc = souprov_conflict_161();
    let probe = slot_conflict_mask_choices(SlotConflictMaskChoicesArgs {
        repos: &ctx.repos,
        excluded: &[],
        params: &BacktrackParams::default(),
        slot_pullers: &HashMap::new(),
        sc: &sc,
        config: &config,
    });
    let mut grown_c = BacktrackParams::default();
    for c in &probe {
        for ((cat, pkg, ver), parents) in c
            .similar
            .iter()
            .map(|s| (&s.target, &s.parents))
            .chain(std::iter::once((&c.target, &c.parents)))
        {
            grown_c
                .runtime_pkg_mask
                .entry((cat.clone(), pkg.clone()))
                .or_default()
                .push(MaskEntry {
                    neg: format!("!={cat}/{pkg}-{ver}"),
                    reason: MaskReason::SlotConflict {
                        parents: parents.clone(),
                    },
                });
        }
    }
    grown_c.dropped_pins.push(pin);
    let mut pass_c = pass_for(hard_single);
    let decision_c = collect_feedback(&ctx, &grown_c, &mut pass_c, &config);
    let PassDecision::Settle { params: base_c } = decision_c else {
        panic!("a quiet pass settles");
    };
    assert_eq!(base_c.dropped_pins.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}
