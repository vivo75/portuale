use super::*;

#[test]
fn mo_sel_trace_line_pins_the_harness_format() {
    // The shared contract with `TEST/scripts/mo-trace/real-trace.py`
    // and `align-traces.py`: one line per `select_nodes` iteration,
    // fields `iter retlist alive asap prefer_asap drop_satisfied ig
    // pick`. Changing this string breaks the aligner, so it is
    // pinned.
    let line = mo_sel_trace_line(MoSelTraceLineArgs {
        iter: 7,
        retlist: 3,
        alive: 42,
        asap: &["m:llvm-runtimes/clang-runtime-22".to_string()],
        prefer_asap: false,
        drop_satisfied: true,
        ig: Some(Ignore::NRuntime),
        pick: &["m:dev-libs/a-1".to_string(), "n:dev-libs/b-2".to_string()],
    });
    assert_eq!(
        line,
        "MO_SEL iter=7 retlist=3 alive=42 asap=[m:llvm-runtimes/clang-runtime-22] \
         prefer_asap=0 drop_satisfied=1 ig=ignore_runtime pick=m:dev-libs/a-1 n:dev-libs/b-2"
    );
    assert_eq!(
        mo_sel_trace_line(MoSelTraceLineArgs {
            iter: 1,
            retlist: 0,
            alive: 5,
            asap: &[],
            prefer_asap: true,
            drop_satisfied: false,
            ig: None,
            pick: &[]
        }),
        "MO_SEL iter=1 retlist=0 alive=5 asap=[] prefer_asap=1 \
         drop_satisfied=0 ig=none pick="
    );
}

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
