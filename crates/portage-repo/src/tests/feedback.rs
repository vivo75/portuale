use super::*;

/// Backlog #253 (half 1): the prune trigger chains the conflict
/// half of real's missed updates (`_get_missed_updates`,
/// `depgraph.py:1529-1565`, the `:1533-1536` chain). At this layer
/// the conflict half rides `pass.skipped_updates` (the direct
/// solve's removal rows, filed before the trigger runs), while the
/// mask half comes out of [`backtrack_missed_updates`] -- so a
/// conflict-source-only pass (non-empty replace set, no live
/// conflicts, mask half empty, direct rows present) must take the
/// prune restart exactly like a mask-half one. RED before the
/// predicate chains both sources (the pass settles after the
/// replace-growth restart instead).
#[test]
fn slot_operator_prune_rebuilds_fires_on_conflict_source_only() {
    let dir = slotundo_temp_dir("253-prune-conflict");
    let config = test_config();
    let opts = CtxOpts161 {
        backtrack_max: 20,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, Vec::new(), &opts);
    let mut grown = BacktrackParams::default();
    grown
        .slot_operator_replace_installed
        .insert(("app-misc".to_string(), "pcons".to_string()));
    let mut pass = pass_161();
    pass.skipped_updates.push(SkippedUpdate {
        category: "dev-libs".to_string(),
        package: "slotconflicttarget".to_string(),
        slot: "0".to_string(),
        skipped_version: "2.0".to_string(),
        skipped_sub_slot: "0".to_string(),
        skipped_repo: "testrepo".to_string(),
        skipped_use: Vec::new(),
        atom: "<dev-libs/slotconflicttarget-2.0".to_string(),
        consumer_cpv: "dev-libs/slotconflictoldconsumer-1.0".to_string(),
        consumer_installed: false,
        consumer_use: Vec::new(),
        missed_targets_running_root: false,
        consumer_targets_running_root: false,
    });
    let decision = collect_feedback(&ctx, &grown, &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::Config { params: out }) = decision else {
        panic!("a conflict-source-only pass restarts with prune_rebuilds");
    };
    assert!(out.prune_rebuilds, "the prune latch is set");
    assert!(
        out.slot_operator_replace_installed.is_empty(),
        "the prune clears the replace set"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #253 (half 1, no-over-fire guard): with neither half
/// present the trigger stays quiet -- a replace set alone settles.
#[test]
fn slot_operator_prune_rebuilds_stays_quiet_without_missed_updates() {
    let dir = slotundo_temp_dir("253-prune-quiet");
    let config = test_config();
    let opts = CtxOpts161 {
        backtrack_max: 20,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, Vec::new(), &opts);
    let mut grown = BacktrackParams::default();
    grown
        .slot_operator_replace_installed
        .insert(("app-misc".to_string(), "pcons".to_string()));
    let mut pass = pass_161();
    let decision = collect_feedback(&ctx, &grown, &mut pass, &config);
    assert!(
        matches!(decision, PassDecision::Settle { .. }),
        "no missed updates, no prune restart"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S4: the use-overlay fold appends new flips and
/// records while an exact `(atom, token)` duplicate stays single.
#[test]
fn collect_feedback_folds_the_use_overlay_without_duplicating() {
    let dir = slotundo_temp_dir("161-cf-over");
    let config = test_config();
    let opts = CtxOpts161::default();
    let ctx = ctx_161(&dir, &config, Vec::new(), &opts);
    let cp = ("dev-libs".to_string(), "overpkg".to_string());
    let mut pass = pass_161();
    pass.use_overlay
        .insert(cp.clone(), HashMap::from([("flag".to_string(), true)]));
    pass.use_change_overlay = vec![
        AutounmaskChange {
            atom: "=dev-libs/overpkg-1.0".to_string(),
            token: "+flag".to_string(),
            dep_chain: Vec::new(),
            trigger: None,
        },
        AutounmaskChange {
            atom: "=dev-libs/overpkg-1.0".to_string(),
            token: "-other".to_string(),
            dep_chain: Vec::new(),
            trigger: None,
        },
    ];
    let params = BacktrackParams::default();
    // One record already logged: the same `(atom, token)` must not
    // append twice, while a new token for the same atom must.
    let mut grown = params.clone();
    grown.autounmask_use_change_records.push(AutounmaskChange {
        atom: "=dev-libs/overpkg-1.0".to_string(),
        token: "-other".to_string(),
        dep_chain: Vec::new(),
        trigger: None,
    });
    let decision = collect_feedback(&ctx, &grown, &mut pass, &config);
    let PassDecision::Settle { params: out } = decision else {
        panic!("a quiet pass settles");
    };
    assert_eq!(
        out.autounmask_use_config.get(&cp),
        Some(&HashMap::from([("flag".to_string(), true)]))
    );
    let records: Vec<(&str, &str)> = out
        .autounmask_use_change_records
        .iter()
        .map(|r| (r.atom.as_str(), r.token.as_str()))
        .collect();
    assert_eq!(
        records,
        vec![
            ("=dev-libs/overpkg-1.0", "-other"),
            ("=dev-libs/overpkg-1.0", "+flag")
        ]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S4: two want-atoms a single version satisfies become
/// that package's retry constraints (real's solvable-conflict
/// re-resolve).
#[test]
fn collect_feedback_solves_a_two_want_conflict_into_constraints() {
    let dir = slotundo_temp_dir("161-cf-solv");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 1,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let mut pass = pass_161();
    pass.slot_conflicts = vec![souprov_conflict_161()];
    let key = ("dev-libs".to_string(), "souprov".to_string());
    pass.slot_want.insert(
        key.clone(),
        vec![
            "dev-libs/souprov".to_string(),
            ">=dev-libs/souprov-2.0".to_string(),
        ],
    );
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::Config { params: out }) = decision else {
        panic!("a solvable conflict retries with constraints");
    };
    assert_eq!(
        out.slot_constraints.get(&key),
        Some(&vec![
            "dev-libs/souprov".to_string(),
            ">=dev-libs/souprov-2.0".to_string()
        ])
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S4: a lone want-atom is not a solvable conflict --
/// the guard needs two -- so the pass falls through to the mask
/// trial instead of constraining a retry.
#[test]
fn collect_feedback_keeps_a_single_want_out_of_constraints() {
    let dir = slotundo_temp_dir("161-cf-single");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 1,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let key = ("dev-libs".to_string(), "souprov".to_string());
    let mut pass = pass_161();
    pass.slot_conflicts = vec![souprov_conflict_161()];
    pass.slot_want
        .insert(key.clone(), vec!["dev-libs/souprov".to_string()]);
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    assert!(
        matches!(
            decision,
            PassDecision::Feedback(BacktrackFeedback::SlotConflict { .. })
        ),
        "a single want falls through to the mask trial"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S4: retry constraints append only the missing wants
/// -- an already-logged atom is not pushed twice.
#[test]
fn collect_feedback_does_not_duplicate_logged_constraints() {
    let dir = slotundo_temp_dir("161-cf-dup");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 1,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let mut pass = pass_161();
    pass.slot_conflicts = vec![souprov_conflict_161()];
    let key = ("dev-libs".to_string(), "souprov".to_string());
    let wants = vec![
        "dev-libs/souprov".to_string(),
        ">=dev-libs/souprov-2.0".to_string(),
    ];
    pass.slot_want.insert(key.clone(), wants.clone());
    let mut grown = BacktrackParams::default();
    grown
        .slot_constraints
        .insert(key.clone(), vec![wants[0].clone()]);
    let decision = collect_feedback(&ctx, &grown, &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::Config { params: out }) = decision else {
        panic!("a solvable conflict retries with constraints");
    };
    assert_eq!(out.slot_constraints.get(&key), Some(&wants));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S4: with the search present but unsatisfiable, the
/// slot-operator trigger stays off and the pass records an empty
/// rebuild list instead of scanning.
#[test]
fn collect_feedback_reports_no_rebuilds_when_the_pass_is_stuck() {
    let dir = slotundo_temp_dir("161-cf-nvc");
    let d = dir.join("var/db/pkg/dev-libs/stale-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    let config = test_config();
    // Reachable so a retried scan would find the stale consumer, but
    // excluded so the reverse-dependency scan itself stays quiet --
    // the legs isolate the slot-operator gate below.
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

/// Backlog #161 S4: an enforced reverse pin is retained once --
/// a reachable consumer's runtime pin on a version the tree still
/// carries is enforced (holdable), not dropped. Append leg (fresh
/// params) and dedup leg (pre-retained pin) share the setup.
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
    // `souprov-1.0` stays a satisfying candidate, so the pin holds
    // and is enforced rather than dropped for the residual report.
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
    // Append leg: fresh params collect the pin via config feedback.
    // Backlog #209: the enforced-pin feedback travels as `RevDep`
    // (same working copy as `Config`; the driver feeds it back
    // in-pass under `--backtrack=0`).
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::RevDep { params: out }) = decision else {
        panic!("a newly enforced pin retries with config");
    };
    assert_eq!(out.reverse_dep_pins, vec![pin.clone()]);
    // Dedup leg: a pre-retained pin is not pushed twice, and the
    // latched constraint keeps the pass quiet.
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

/// Backlog #161 S4: a dropped reverse pin is collected once -- the
/// `pinner`/`souprov` shape with a hard upgrade requirement drops
/// the pin for the residual report. Three legs isolate the three
/// sites: the conflict-gated first scan (append), its dedup, and
/// the unconditional second scan (dedup after mask-trial fall-
/// through).
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
    // Leg A: two solvable wants retry with constraints -- the
    // early return isolates the first scan, which collects the pin.
    let mut pass_a = pass_for(hard_double);
    let decision_a = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass_a, &config);
    let PassDecision::Feedback(BacktrackFeedback::Config { params: base_a }) = decision_a else {
        panic!("a solvable conflict retries with constraints");
    };
    assert_eq!(base_a.dropped_pins, vec![pin.clone()]);
    // Leg B: a lone want falls through to the mask trial -- the
    // first scan's dedup skips a retained pin.
    let mut pass_b = pass_for(hard_single.clone());
    let mut grown_b = BacktrackParams::default();
    grown_b.dropped_pins.push(pin.clone());
    let decision_b = collect_feedback(&ctx, &grown_b, &mut pass_b, &config);
    let PassDecision::Feedback(BacktrackFeedback::SlotConflict { base: base_b, .. }) = decision_b
    else {
        panic!("an unsolvable conflict masks");
    };
    assert_eq!(base_b.dropped_pins, vec![pin.clone()]);
    // Leg C: with the mask trial's entries pre-covered, the pass
    // settles -- the second scan's dedup skips the retained pin.
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

/// Backlog #205 R4, stale-trigger leg: when the recorded trigger is
/// no longer a requirer (masking retry, parent-flip rescue), the
/// fill falls back to the legacy first-requirer start. Direct
/// `fill_use_change_chains` shape: `abk0c` is required by `abk0a`
/// (which ascends to the `abk0d` argument), while the recorded
/// trigger names a `abk0d` that requires nothing here -- stale.
#[test]
fn stale_trigger_falls_back_to_the_first_requirer() {
    let mut abk0c = graph_entry("dev-libs", "abk0c", "1");
    abk0c.required_by = vec![("dev-libs".to_string(), "abk0a".to_string())];
    let mut abk0a = graph_entry("dev-libs", "abk0a", "2");
    abk0a.required_by = vec![("dev-libs".to_string(), "abk0d".to_string())];
    let abk0d = graph_entry("dev-libs", "abk0d", "1");
    let entries = vec![abk0c, abk0a, abk0d];
    let top_level: std::collections::HashSet<&str> = ["dev-libs/abk0d"].into_iter().collect();
    let mut changes = vec![AutounmaskChange {
        atom: ">=dev-libs/abk0c-1".to_string(),
        token: "x y".to_string(),
        dep_chain: Vec::new(),
        trigger: Some(("dev-libs".to_string(), "abk0d".to_string())),
    }];
    fill_use_change_chains(
        &entries,
        &mut changes,
        &top_level,
        Path::new("/nonexistent-root-for-this-test"),
    );
    assert_eq!(
        changes[0].dep_chain,
        vec![
            "required by dev-libs/abk0a-2::testrepo".to_string(),
            "required by dev-libs/abk0d-1::testrepo".to_string(),
            "required by dev-libs/abk0d (argument)".to_string(),
        ]
    );
}

/// Backlog #248: a USE change on an argument package (no requirers)
/// yields exactly the `(argument)` row -- real `_get_dep_chain`
/// never prints the start node (fresh host 3.0.82.2 staged-fixture
/// probe 2026-09-29: `emerge --pretend dev-libs/useflagpkg[-foo]`
/// prints only `# required by dev-libs/useflagpkg[-foo]
/// (argument)`). Direct `fill_use_change_chains` shape: the
/// `useflagpkg` entry requires nothing, so there is no forcing
/// parent to start from.
#[test]
fn use_change_on_an_argument_package_yields_only_the_argument_row() {
    let own = graph_entry("dev-libs", "useflagpkg", "1.0");
    assert!(own.required_by.is_empty());
    let entries = vec![own];
    let top_level: std::collections::HashSet<&str> =
        ["dev-libs/useflagpkg[-foo]"].into_iter().collect();
    let mut changes = vec![AutounmaskChange {
        atom: ">=dev-libs/useflagpkg-1.0".to_string(),
        token: "-foo".to_string(),
        dep_chain: Vec::new(),
        trigger: None,
    }];
    fill_use_change_chains(
        &entries,
        &mut changes,
        &top_level,
        Path::new("/nonexistent-root-for-this-test"),
    );
    assert_eq!(
        changes[0].dep_chain,
        vec!["required by dev-libs/useflagpkg[-foo] (argument)".to_string()]
    );
}

/// Backlog #248, keyword twin: a keyword change on an argument
/// package yields exactly the `(argument)` row (same fresh host
/// probe family as the contract pin:
/// `emerge --pretend --autounmask dev-libs/autounmaskkeywordpkg`
/// prints only `# required by dev-libs/autounmaskkeywordpkg
/// (argument)`).
#[test]
fn keyword_change_on_an_argument_package_yields_only_the_argument_row() {
    let own = graph_entry("dev-libs", "autounmaskkeywordpkg", "1.0");
    assert!(own.required_by.is_empty());
    let entries = vec![own];
    let top_level: std::collections::HashSet<&str> =
        ["dev-libs/autounmaskkeywordpkg"].into_iter().collect();
    let mut changes = vec![AutounmaskChange {
        atom: "=dev-libs/autounmaskkeywordpkg-1.0".to_string(),
        token: "~amd64".to_string(),
        dep_chain: Vec::new(),
        trigger: None,
    }];
    fill_non_use_change_chains(
        &entries,
        &mut changes,
        &top_level,
        Path::new("/nonexistent-root-for-this-test"),
    );
    assert_eq!(
        changes[0].dep_chain,
        vec!["required by dev-libs/autounmaskkeywordpkg (argument)".to_string()]
    );
}
