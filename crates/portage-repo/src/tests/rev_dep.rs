use super::*;

/// Backlog #161 S1: `is_built_slot_op` is real's
/// `Atom.slot_operator_built` gate -- a *built* `=` atom naming both
/// slot and sub-slot. Each conjunct is load-bearing: `:=` (no slot),
/// `:2=` (no sub-slot) and `:2` (no operator) are all *not* built
/// ops, and the companion normaliser strips the slot binding only
/// for a built op.
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

/// Backlog #25 S1b-fix: `relax_built_slot_operator_atom` is real's
/// `Atom.with_slot("=")` as `_slot_operator_check_reverse_-
/// dependencies` applies it (`depgraph.py:2494-2502`) -- the
/// recorded `:S/SS=` binding relaxes to bare `:=`, keeping the
/// version operator, `[use]` deps and `::repo`. Non-built atoms
/// (bare `:=`, `:S=` without sub-slot, plain slots, unversioned)
/// and unparseable text pass through verbatim.
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
    // The relaxed form still matches the new-slot candidate and the
    // old one, while the raw recorded form rejects the new slot --
    // the exact property the rebuild-trigger predicate needs.
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

/// Backlog #25 S1b-fix: `built_slot_operator_rebuild_trigger` fires
/// for a recorded binding the probe's relaxation accepts (the
/// slotop upgrade shape) and stays silent for a version veto (the
/// r25 withhold shape), for a non-built atom, and for unparseable
/// text.
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
    // #54: an installed consumer's pin is only found when its cp is in
    // `reachable` (the `@world ∪ @selected ∪ @system ∪` argument-cp
    // closure, `ResolveCtx::slot_op_reachable`) or is itself a graph
    // entry this pass -- not merely because it is installed.
    let dir = TempDir::new("portage-repo-revdep").keep();
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
    // `pinner` records a pin on `target` that this run's upgrade
    // breaks (the `keeper`/`paired` fixture-oracle shape, #54).
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

    // Not reachable, and `pinner` is not itself a graph entry -> the
    // consumer scan skips it entirely (real never walks to it either).
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "an unreachable installed consumer must yield no pin"
    );

    // Reachable (as if `pinner` were in @world, directly or
    // transitively) -> the pin is found. Enforced-vs-dropped is
    // `rev_dep_pin_holdable`'s call (exercised by the contract suite);
    // only "found at all" matters here.
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "pinner".to_string())]);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
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
    // Backlog #210: a same-version slot/sub-slot change without a
    // revbump counts as a version change (real `_complete_graph`,
    // `depgraph.py:8611-8618`) -- a `Reinstall { slot_changed: true }`
    // entry fills the upgrading map, so a reachable installed
    // consumer's recorded pin is found. Pre-fix the map held only
    // `Upgrade`/`Downgrade` and this shape yielded no pin at all.
    //
    // Both pin flavours fire: the plain slot-bound pin
    // (`target:0/1`) verbatim, and the built slot-operator binding
    // (`target:0/1=`) verbatim too -- the Upgrade path strips the
    // latter into the rebuild domain (#24 S5), but the reinstall
    // re-seed constrains selection with the recorded atom as-is
    // (`depgraph.py:8677-8754`). A `slot_changed: false` reinstall
    // still yields nothing.
    let dir = TempDir::new("portage-repo-revdep-210").keep();
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
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
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
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "a slot-unchanged reinstall must yield no pin"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reverse_dependency_constraints_skips_a_noop_top_level_argument() {
    // Backlog #210 (Q2a): a top-level argument that settles
    // `AlreadyInstalled` is a no-op request, not a digraph node in
    // real, so it is never a constraint source -- even though it
    // carries a graph entry and complete-mode arg-seeding pulls it
    // into `reachable`. A consumer the required sets reach stays a
    // source, whether or not it was also requested.
    let dir = TempDir::new("portage-repo-revdep-210-gate").keep();
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
    // Complete-mode `reachable`, arg-seeded: the requested consumer is
    // in it, exactly as `ResolveCtx::slot_op_reachable` would have it.
    let reachable: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "target".to_string()),
        ("dev-libs".to_string(), "consumer".to_string()),
    ]);
    let top_level: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "target".to_string()),
        ("dev-libs".to_string(), "consumer".to_string()),
    ]);

    // Requested but not world-reached: the no-op argument's pin is
    // skipped (real reinstalls silently here).
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
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
        repos: &[],
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
        config: &portage_profile::Config::default(),
        complete: false,
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
    // Backlog #222, on the committed fixtures (upstream
    // `test_complete_graph.py` pg0: `dev-libs/cgp0x-2.8.0` IUSE
    // `+icu`, installed with `USE=""`; installed `dev-libs/cgp0q`
    // records `dev-libs/cgp0x:2[-icu]`): a same-version USE-change
    // reinstall moves no version, so the stripped-constraint scan
    // finds nothing -- but the flip breaks the installed consumer's
    // `[use]` bound, which real fails in its complete-mode
    // end-of-walk loop (`depgraph.py:8751-8791`). The pin is
    // dropped for the residual report (the merge's USE is decided;
    // real fails rather than reflipping it), never enforced.
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

    // Complete mode off (`--complete-graph-if-new-use=n`, real rc 0):
    // no pin at all.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &repos,
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
        config: &portage_profile::Config::default(),
        complete: false,
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "complete mode off must yield no USE-break pin, got enforced={enforced:?} dropped={dropped:?}"
    );

    // Complete mode on (real rc 1): the broken `[use]` pin drops for
    // the residual installed-instance report.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &repos,
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
        config: &portage_profile::Config::default(),
        complete: true,
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

    // Unreachable consumer (the shared-world shape -- real rc 0 with
    // that world): still nothing.
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &repos,
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
        config: &portage_profile::Config::default(),
        complete: true,
    });
    assert!(
        enforced.is_empty() && dropped.is_empty(),
        "an unreachable consumer must yield no pin, got enforced={enforced:?} dropped={dropped:?}"
    );
}
