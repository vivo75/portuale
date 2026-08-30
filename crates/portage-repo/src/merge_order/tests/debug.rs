use super::*;

#[test]
fn outcome_version_pins_every_outcome() {
    let mut e = new_entry("dev-libs", "x", "1.0", Vec::new());
    e.outcome = PretendOutcome::NoVisibleCandidate;
    assert_eq!(outcome_version(&e), None);
    e.outcome = PretendOutcome::New {
        version: "1.0".into(),
    };
    assert_eq!(outcome_version(&e), Some("1.0"));
    e.outcome = PretendOutcome::Upgrade {
        from: "0.9".into(),
        to: "1.2".into(),
    };
    assert_eq!(outcome_version(&e), Some("1.2"));
    e.outcome = PretendOutcome::Downgrade {
        from: "2.0".into(),
        to: "1.0".into(),
    };
    assert_eq!(outcome_version(&e), Some("1.0"));
    e.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    assert_eq!(outcome_version(&e), Some("1.0"));
    e.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: vec!["flip".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    assert_eq!(outcome_version(&e), Some("1.0"));
    e.outcome = PretendOutcome::Uninstall {
        version: "1.0".into(),
    };
    assert_eq!(outcome_version(&e), None);
}
