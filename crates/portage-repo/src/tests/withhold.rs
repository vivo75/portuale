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
        consumer: (
            "dev-libs".to_string(),
            "whtarget".to_string(),
            "1.0".to_string(),
        ),
    }];
    let skipped = constraint_withheld_updates(&repos, &root, &config, &entries, &enforced);
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
