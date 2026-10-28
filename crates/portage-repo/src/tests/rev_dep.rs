use super::*;

/// A built slot op names both slot and sub-slot with `=`; `:=`,
/// `:2=` and `:2` are not built ops (Portage `slot_operator_built`).
#[test]
fn is_built_slot_op_requires_equals_operator_and_both_slots() {
    let built = |s: &str| {
        let atom = portage_dep::parse_atom(s).expect("test atom parses");
        is_built_slot_op(&atom)
    };
    assert!(built("dev-libs/bar:2/2="));
    assert!(!built("dev-libs/bar:="));
    assert!(!built("dev-libs/bar:2="));
    assert!(!built("dev-libs/bar:2"));
    assert!(!built("dev-libs/bar"));

    let norm = |s: &str| {
        let atom = portage_dep::parse_atom(s).expect("test atom parses");
        reverse_dep_constraint_atom(s, &atom)
    };
    assert_eq!(norm("dev-libs/bar:2/2="), "dev-libs/bar");
    assert_eq!(norm("dev-libs/bar:2/2=[x]"), "dev-libs/bar");
    assert_eq!(norm("dev-libs/bar:2"), "dev-libs/bar:2");
    assert_eq!(norm("dev-libs/bar"), "dev-libs/bar");
}

/// A recorded `:S/SS=` binding relaxes to bare `:=`, keeping version,
/// use-deps and repo; anything not built passes through verbatim.
/// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
#[test]
fn relax_built_slot_operator_atom_keeps_everything_but_the_binding() {
    assert_eq!(
        relax_built_slot_operator_atom(">=dev-libs/provpkg-1.0:0/1="),
        ">=dev-libs/provpkg-1.0:="
    );
    assert_eq!(
        relax_built_slot_operator_atom("<dev-libs/r25lib-2.0:0/1="),
        "<dev-libs/r25lib-2.0:="
    );
    assert_eq!(
        relax_built_slot_operator_atom(">=dev-libs/provpkg-1.0:0/1=[x]"),
        ">=dev-libs/provpkg-1.0:=[x]"
    );
    assert_eq!(
        relax_built_slot_operator_atom(">=dev-libs/provpkg-1.0:0/1=::testrepo"),
        ">=dev-libs/provpkg-1.0:=::testrepo"
    );
    // Not built: verbatim.
    assert_eq!(
        relax_built_slot_operator_atom("dev-libs/provpkg:="),
        "dev-libs/provpkg:="
    );
    assert_eq!(
        relax_built_slot_operator_atom("dev-libs/slotoptarget:2="),
        "dev-libs/slotoptarget:2="
    );
    assert_eq!(
        relax_built_slot_operator_atom(">=dev-libs/provpkg-1.0"),
        ">=dev-libs/provpkg-1.0"
    );
    assert_eq!(
        relax_built_slot_operator_atom("dev-libs/provpkg"),
        "dev-libs/provpkg"
    );
    assert_eq!(
        relax_built_slot_operator_atom("not an atom ((("),
        "not an atom ((("
    );
    // The relaxed form matches both slots; the raw form rejects the
    // new one -- the property the rebuild-trigger predicate needs.
    let cands = [
        "dev-libs/provpkg-1.0:0/1::testrepo",
        "dev-libs/provpkg-2.0:0/2::testrepo",
    ];
    let raw = ">=dev-libs/provpkg-1.0:0/1=";
    let relaxed = relax_built_slot_operator_atom(raw);
    assert_eq!(
        portage_dep::match_from_list(raw, &cands).map(|m| m.len()),
        Some(1)
    );
    assert_eq!(
        portage_dep::match_from_list(&relaxed, &cands).map(|m| m.len()),
        Some(2)
    );
}

/// Fires for a recorded binding the probe's relaxation accepts;
/// silent for a version veto, a non-built atom, unparseable text.
#[test]
fn built_slot_operator_rebuild_trigger_only_forgives_rebuilds() {
    assert!(built_slot_operator_rebuild_trigger(
        ">=dev-libs/provpkg-1.0:0/1=",
        "dev-libs/provpkg-2.0:0/2"
    ));
    assert!(!built_slot_operator_rebuild_trigger(
        "<dev-libs/r25lib-2.0:0/1=",
        "dev-libs/r25lib-2.0:0/2"
    ));
    assert!(!built_slot_operator_rebuild_trigger(
        "dev-libs/provpkg:=",
        "dev-libs/provpkg-2.0:0/2"
    ));
    assert!(!built_slot_operator_rebuild_trigger(
        ">=dev-libs/provpkg-1.0",
        "dev-libs/provpkg-2.0:0/2"
    ));
    assert!(!built_slot_operator_rebuild_trigger(
        "not an atom (((",
        "dev-libs/provpkg-2.0:0/2"
    ));
}

#[test]
fn reverse_dependency_constraints_skips_an_unreachable_installed_consumer() {
    // A pin is found only when its cp is reachable or itself a graph
    // entry -- not merely because it is installed.
    let dir = TempDir::new("portage-repo-revdep");
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
    // `pinner` records a pin this run's upgrade breaks.
    mk("pinner-1.0", "=dev-libs/target-1.0");
    mk("target-1.0", "");

    let target_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "target".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        ..graph_entry("dev-libs", "target", "2.0")
    };
    let entries = [target_upgrade];
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::new();

    // Not reachable and not a graph entry: skipped (Portage never
    // walks to it either).
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &HashSet::new(),
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "an unreachable installed consumer must yield no pin"
    );

    // Reachable: the pin is found (enforced-vs-dropped belongs to
    // `rev_dep_pin_holdable`, pinned by the contract suite).
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "pinner".to_string())]);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    assert_eq!(
        enforced.len() + dropped.len(),
        1,
        "a reachable installed consumer's pin must be found"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reverse_dependency_constraints_finds_a_slot_change_reinstall_pin() {
    // A same-version slot change without a revbump fills the upgrading
    // map (Portage `_complete_graph`), so the recorded pin is found.
    // Both pin flavours fire verbatim; a `slot_changed: false`
    // reinstall still yields nothing.
    let dir = TempDir::new("portage-repo-revdep-210");
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("target-1.0", "0/1", "");
    mk("consumer-1.0", "0", "dev-libs/target:0/1");
    mk("bound-1.0", "0", "dev-libs/target:0/1=");

    let reinstall = |slot_changed: bool| GraphEntry {
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "target", "1.0")
    };
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::new();
    let reachable: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "consumer".to_string()),
        ("dev-libs".to_string(), "bound".to_string()),
    ]);

    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &[reinstall(true)],
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    let mut atoms: Vec<String> = enforced
        .iter()
        .chain(dropped.iter())
        .map(|p| p.atom.clone())
        .collect();
    atoms.sort();
    assert_eq!(
        atoms,
        vec![
            "dev-libs/target:0/1".to_string(),
            "dev-libs/target:0/1=".to_string(),
        ],
        "both pin flavours must fire on a slot-change reinstall, got enforced={enforced:?} dropped={dropped:?}"
    );

    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &[reinstall(false)],
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "a slot-unchanged reinstall must yield no pin"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reverse_dependency_constraints_skips_a_noop_top_level_argument() {
    // A no-op top-level request is never a constraint source, even
    // though it carries a graph entry and is reachable (Portage).
    let dir = TempDir::new("portage-repo-revdep-210-gate");
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
    mk("target-1.0", "");
    mk("consumer-1.0", "dev-libs/target:0/1");

    let target_reinstall = GraphEntry {
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "target", "1.0")
    };
    let consumer_noop = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", "consumer", "1.0")
    };
    let entries = [target_reinstall, consumer_noop];
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::new();
    // Complete-mode reachable, arg-seeded like `slot_op_reachable`.
    let reachable: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "target".to_string()),
        ("dev-libs".to_string(), "consumer".to_string()),
    ]);
    let top_level: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "target".to_string()),
        ("dev-libs".to_string(), "consumer".to_string()),
    ]);

    // Requested but not world-reached: the pin is skipped (Portage
    // reinstalls silently here).
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &top_level,
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "a no-op top-level argument must yield no pin, got enforced={enforced:?} dropped={dropped:?}"
    );

    // World-reached as well (a world member that was also requested):
    // the pin fires.
    let world: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "consumer".to_string())]);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &top_level,
        world_reachable: &world,
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&[]),
    });
    assert_eq!(
        enforced.len() + dropped.len(),
        1,
        "a world-reached consumer stays a source when also requested, got enforced={enforced:?} dropped={dropped:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reverse_dependency_constraints_drops_a_use_break_from_a_use_change_reinstall() {
    // A USE-change reinstall moves no version but breaks the
    // installed consumer's `[use]` bound: dropped, never enforced.
    // Portage: depgraph.py, complete-mode end-of-walk loop
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let entry = GraphEntry {
        outcome: PretendOutcome::Reinstall {
            version: "2.8.0".into(),
            changed_flags: vec!["icu".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("2".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "cgp0x", "2.8.0")
    };
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::from([(
        ("dev-libs".to_string(), "cgp0x".to_string()),
        vec!["dev-libs/cgp0x".to_string()],
    )]);
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "cgp0q".to_string())]);

    // Complete mode off: no pin at all.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &root,
        entries: std::slice::from_ref(&entry),
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &reachable,
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&repos),
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "complete mode off must yield no USE-break pin, got enforced={enforced:?} dropped={dropped:?}"
    );

    // Complete mode on: the broken pin drops for the residual report.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &root,
        entries: &[entry],
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &reachable,
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: true,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&repos),
    });
    assert!(
        enforced.is_empty(),
        "a USE-break pin must never be enforced, got {enforced:?}"
    );
    assert_eq!(
        dropped.len(),
        1,
        "expected one USE-break pin, got {dropped:?}"
    );
    assert_eq!(
        (dropped[0].cp.1.as_str(), dropped[0].consumer.1.as_str(),),
        ("cgp0x", "cgp0q"),
    );

    // Unreachable consumer: still nothing.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &root,
        entries: &[GraphEntry {
            outcome: PretendOutcome::Reinstall {
                version: "2.8.0".into(),
                changed_flags: vec!["icu".to_string()],
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
            slot: Some("2".into()),
            sub_slot: Some("0".into()),
            ..graph_entry("dev-libs", "cgp0x", "2.8.0")
        }],
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &HashSet::new(),
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        report_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: true,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&repos),
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "an unreachable consumer must yield no pin, got enforced={enforced:?} dropped={dropped:?}"
    );
}

/// A built `:S/SS=` atom the *kept* instance still satisfies is a parent of the
/// skipped update in Portage's missed-update tail, but never a pin: it must
/// not constrain the merge. Report-only, and only in complete mode.
/// Portage: depgraph.py, `_process_slot_conflicts` ("Record missed updates").
#[test]
fn built_atom_the_kept_instance_satisfies_is_a_report_only_parent() {
    let dir = TempDir::new("portage-repo-573-report");
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0/1\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    // The real-package shape: only the built atom, satisfied by the installed
    // 1.0 the merge keeps.
    mk("real-1.0", "dev-libs/target:0/1=");
    mk("target-1.0", "");
    let entries = [GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "target", "2.0")
    }];
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::new();
    let empty_cps: HashSet<(String, String)> = HashSet::new();
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "real".to_string())]);
    let gate = crate::test_gate(&[]);
    let config = portage_profile::Config::default();
    // The complete walk recursed into the consumer: its built atom is a
    // parent Portage records, but the kept instance still satisfies it, so it
    // never becomes a constraint.
    let walked: HashMap<(String, String, String), HashSet<String>> = HashMap::from([(
        (
            "dev-libs".to_string(),
            "real".to_string(),
            "1.0".to_string(),
        ),
        HashSet::new(),
    )]);
    let args = |portage_complete: bool| ReverseDependencyConstraintsArgs {
        root: &dir,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &empty_cps,
        world_reachable: &empty_cps,
        report_reachable: &empty_cps,
        config: &config,
        complete: portage_complete,
        portage_complete,
        installed_walked: &walked,
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate,
    };
    let (enforced, dropped, report) = reverse_dependency_constraints_reported(args(true));
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "the kept instance satisfies the built atom, so nothing is pinned: \
         enforced={enforced:?} dropped={dropped:?}"
    );
    assert_eq!(
        report
            .iter()
            .map(|p| (p.atom.as_str(), p.consumer.1.as_str()))
            .collect::<Vec<_>>(),
        vec![("dev-libs/target:0/1=", "real")],
        "the built row is a report-only parent"
    );
    let (_, _, quiet) = reverse_dependency_constraints_reported(args(false));
    assert!(
        quiet.is_empty(),
        "no complete graph, no missed-update parent: {quiet:?}"
    );
}

/// The report rows ride the mask an enforced pin creates, and a second pass
/// over the same scan appends nothing twice.
#[test]
fn report_only_parents_append_once_to_the_mask_they_ride() {
    let dir = slotundo_temp_dir("573-report-mask");
    let config = test_config();
    let opts = CtxOpts161 {
        backtrack_max: 20,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, Vec::new(), &opts);
    let consumer = (
        "dev-libs".to_string(),
        "real".to_string(),
        "1.0".to_string(),
    );
    let pin = |atom: &str| RevDepPin {
        cp: ("dev-libs".to_string(), "target".to_string()),
        atom: atom.to_string(),
        raw_atom: atom.to_string(),
        built_render: Vec::new(),
        consumer: consumer.clone(),
    };
    let mut pass = pass_161();
    pass.entries.push(GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "target", "2.0")
    });
    let scan = || ReverseScan {
        enforced: vec![pin("=dev-libs/target-1.0")],
        dropped: Vec::new(),
        report: vec![pin("dev-libs/target:0/1=")],
    };
    let mut grown = BacktrackParams::default();
    let first = step_reverse_dep_pins(&ctx, &pass, &mut grown, scan());
    let std::ops::ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::RevDep { params })) =
        first
    else {
        panic!("the enforced pin asks for a restart");
    };
    let mut grown = *params;
    let parents = |grown: &BacktrackParams| {
        grown
            .runtime_pkg_mask
            .get(&("dev-libs".to_string(), "target".to_string()))
            .and_then(|bucket| bucket.first())
            .map(|entry| match &entry.reason {
                MaskReason::SlotConflict { parents } => parents.clone(),
                _ => Vec::new(),
            })
            .unwrap_or_default()
    };
    assert_eq!(
        parents(&grown),
        vec![
            (consumer.clone(), "=dev-libs/target-1.0".to_string()),
            (consumer.clone(), "dev-libs/target:0/1=".to_string()),
        ],
        "the enforced pin creates the mask and the report row joins it"
    );
    let _ = step_reverse_dep_pins(&ctx, &pass, &mut grown, scan());
    assert_eq!(
        parents(&grown),
        vec![
            (consumer.clone(), "=dev-libs/target-1.0".to_string()),
            (consumer.clone(), "dev-libs/target:0/1=".to_string()),
        ],
        "a repeated pass does not append the same rows again"
    );
}
