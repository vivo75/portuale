use super::*;

#[test]
fn reverse_pin_withhold_reports_a_skip_notice() {
    // Walk-level #92 shape on the committed wh fixtures: the
    // installed `whtarget-1.0` runtime pin is enforced (S1), the
    // upgrade settles installed, and the withheld 2.0 is reported
    // -- no conflict ever forms. Grounded on the staged real
    // capture (merge `whpuller-2.0` + `WARNING`, rc 0).
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let mut puller = graph_entry("dev-libs", "whpuller", "2.0");
    puller.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "2.0".into(),
    };
    let mut blocker = graph_entry("dev-libs", "whblocker", "1.0");
    blocker.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![puller, blocker];
    let enforced = vec![RevDepPin {
        cp: ("dev-libs".to_string(), "whblocker".to_string()),
        atom: "~dev-libs/whblocker-1.0".to_string(),
        raw_atom: "~dev-libs/whblocker-1.0".to_string(),
        built_render: Vec::new(),
        consumer: (
            "dev-libs".to_string(),
            "whtarget".to_string(),
            "1.0".to_string(),
        ),
    }];
    let skipped = constraint_withheld_updates(
        crate::test_gate(&repos),
        &root,
        &config,
        &entries,
        &enforced,
    );
    assert_eq!(skipped.len(), 1);
    let s = &skipped[0];
    assert_eq!(
        (s.category.as_str(), s.package.as_str()),
        ("dev-libs", "whblocker")
    );
    assert_eq!(s.skipped_version, "2.0");
    assert_eq!(s.atom, "~dev-libs/whblocker-1.0");
    assert!(s.consumer_cpv.contains("whtarget-1.0"));
    assert!(s.consumer_installed);
}

/// Two installed consumers recording the same pin atom are two parents:
/// Portage keeps one `(parent, atom)` pair per parent
/// (`depgraph.py:2107-2121`), so the skip block names both.
#[test]
fn two_consumers_of_the_same_pin_atom_each_get_a_row() {
    let root = super::tests_consumer_pin_conflict::hermetic_root(&[]);
    // The fixture vdb ships `whtarget-1.0` recording `~whblocker-1.0`; add a
    // second installed consumer with the same recorded atom.
    let second = root.join("var/db/pkg/dev-libs/whconsumer-b-1.0");
    fs::create_dir_all(&second).unwrap();
    fs::write(second.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(second.join("SLOT"), "0\n").unwrap();
    fs::write(second.join("USE"), "\n").unwrap();
    fs::write(second.join("RDEPEND"), "~dev-libs/whblocker-1.0\n").unwrap();
    fs::write(second.join("repository"), "testrepo\n").unwrap();

    let repos = find_repos(&fixtures_root()).expect("fixture repos.conf must resolve");
    let mut puller = graph_entry("dev-libs", "whpuller", "2.0");
    puller.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "2.0".into(),
    };
    let mut blocker = graph_entry("dev-libs", "whblocker", "1.0");
    blocker.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let pin = |consumer: &str| RevDepPin {
        cp: ("dev-libs".to_string(), "whblocker".to_string()),
        atom: "~dev-libs/whblocker-1.0".to_string(),
        raw_atom: "~dev-libs/whblocker-1.0".to_string(),
        built_render: Vec::new(),
        consumer: (
            "dev-libs".to_string(),
            consumer.to_string(),
            "1.0".to_string(),
        ),
    };
    let enforced = vec![pin("whtarget"), pin("whconsumer-b")];
    let skipped = constraint_withheld_updates(
        crate::test_gate(&repos),
        &root,
        &test_config(),
        &[puller, blocker],
        &enforced,
    );
    let mut consumers: Vec<&str> = skipped
        .iter()
        .map(|s| s.consumer_cpv.split(':').next().unwrap())
        .collect();
    consumers.sort();
    assert_eq!(
        consumers,
        vec!["dev-libs/whconsumer-b-1.0", "dev-libs/whtarget-1.0"],
        "both installed consumers of the withheld version are named: {skipped:?}"
    );
    assert_eq!(skipped.len(), 2);
    assert!(skipped.iter().all(|s| s.skipped_version == "2.0"));
}
