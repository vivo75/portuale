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

#[test]
fn reverse_dependency_constraints_skips_an_unreachable_installed_consumer() {
    // A pin is found only when its cp is reachable or itself a graph
    // entry -- not merely because it is installed.
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

    // Reachable: the pin is found (enforced-vs-dropped belongs to
    // `rev_dep_pin_holdable`, pinned by the contract suite).
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
    // A same-version slot change without a revbump fills the upgrading
    // map (Portage `_complete_graph`), so the recorded pin is found.
    // Both pin flavours fire verbatim; a `slot_changed: false`
    // reinstall still yields nothing.
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
    // A no-op top-level request is never a constraint source, even
    // though it carries a graph entry and is reachable (Portage).
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

    // Complete mode on: the broken pin drops for the residual report.
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

    // Unreachable consumer: still nothing.
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
