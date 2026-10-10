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
        ig: Some(n_ignore_runtime),
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

#[test]
fn add_installed_dependency_closure_seeds_and_expands_the_installed_tree() {
    let root = mo_tmp_vdb(
        "closure-seed",
        &[
            (
                "dev-libs",
                "one-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/two")],
            ),
            (
                "dev-libs",
                "two-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/three")],
            ),
            ("dev-libs", "three-1.0", &[("SLOT", "0")]),
            ("dev-libs", "three-2.0", &[("SLOT", "0")]),
        ],
    );
    // 1 is an installed entry with no deps yet (seed 2 fills it).
    let mut two = new_entry("dev-libs", "two", "1.0", Vec::new());
    two.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let top = new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![
            runtime_edge("<dev-libs/three-2", "dev-libs", "three"),
            runtime_edge("dev-libs/one", "dev-libs", "one"),
        ],
    );
    let mut entries = vec![top, two];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &[],
        system_atoms: &[],
        virtuals_only: false,
        dynamic_deps: true,
    });
    // top, two, three-1.0 (the version-qualified pick), one, three-2.0
    // (the highest match of two's bare atom).
    assert_eq!(
        entries.len(),
        5,
        "{:#?}",
        entries
            .iter()
            .map(|e| (&e.package, outcome_version(e)))
            .collect::<Vec<_>>()
    );
    assert_eq!(entries[2].package, "three");
    assert_eq!(
        outcome_version(&entries[2]),
        Some("1.0"),
        "the qualified atom picks 1.0"
    );
    assert_eq!(entries[3].package, "one");
    assert_eq!(
        entries[3]
            .deps
            .iter()
            .map(|e| e.atom.as_str())
            .collect::<Vec<_>>(),
        vec!["dev-libs/two"],
        "the seeded node's own vdb deps are filled"
    );
    assert_eq!(entries[4].package, "three");
    assert_eq!(
        outcome_version(&entries[4]),
        Some("2.0"),
        "the bare atom picks the highest"
    );
    assert_eq!(
        entries[1]
            .deps
            .iter()
            .map(|e| e.atom.as_str())
            .collect::<Vec<_>>(),
        vec!["dev-libs/three"],
        "seed 2 fills an installed entry's deps"
    );
}

#[test]
fn add_installed_dependency_closure_keeps_the_first_of_vercmp_equal_versions() {
    // `1.0` and `1.0-r0` compare equal (a missing revision is 0), so
    // `pick_installed`'s `best` scan must keep the first installed
    // version it saw rather than replace it on the equality.
    let root = mo_tmp_vdb(
        "closure-vercmp-eq",
        &[
            (
                "dev-libs",
                "eqver-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/leaf")],
            ),
            ("dev-libs", "eqver-1.0-r0", &[("SLOT", "1")]),
            ("dev-libs", "leaf-1.0", &[("SLOT", "0")]),
        ],
    );
    let mut entries = vec![new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![runtime_edge("dev-libs/eqver", "dev-libs", "eqver")],
    )];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &[],
        system_atoms: &[],
        virtuals_only: false,
        dynamic_deps: true,
    });
    let picked = entries
        .iter()
        .find(|e| e.package == "eqver")
        .expect("the installed dependency is seeded");
    assert_eq!(
        outcome_version(picked),
        Some("1.0"),
        "the first vercmp-equal installed version is kept"
    );
}
