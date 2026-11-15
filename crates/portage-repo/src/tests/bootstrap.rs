//! Charter 2a bootstrap: the resolver synthesizes a reduced-USE boot row for the
//! owner's pair (three rows, marker on the boot row only); unbreakable shapes
//! keep failing.

use super::*;

/// The owner's pair, both flags default-on: a genuine build-time 2-cycle.
fn bootstrap_graph(atoms: &[&str]) -> GraphResult {
    bootstrap_graph_with(atoms, &test_config())
}

/// [`bootstrap_graph`] under `config`; every boot row built must only
/// remove USE flags: its enabled set is a strict subset of the declared USE.
fn bootstrap_graph_with(atoms: &[&str], config: &portage_profile::Config) -> GraphResult {
    let root = fixtures_root();
    let owned: Vec<String> = atoms.iter().map(|a| a.to_string()).collect();
    let result = ResolveRequest::new(
        &root,
        &root,
        &owned,
        config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {owned:?} failed: {e}"));
    for boot in result.entries.iter().filter(|e| e.bootstrap.is_some()) {
        let marker = boot.bootstrap.as_ref().unwrap();
        let declared: BTreeSet<String> = marker.declared_use.iter().cloned().collect();
        let on = enabled_use(boot);
        assert!(
            on.is_subset(&declared) && declared.contains(&marker.reduced_flag),
            "boot USE {on:?} must be declared {declared:?} minus {}",
            marker.reduced_flag
        );
        assert!(!on.contains(&marker.reduced_flag), "{on:?}");
    }
    result
}

/// `config` with each `(atom, flag)` forced on, as a profile's
/// `package.use.force` lines.
fn forcing(lines: &[(&str, &str)]) -> portage_profile::Config {
    let mut config = test_config();
    config.use_mask_force_levels = vec![portage_profile::UseMaskForceLevel {
        package_use_force: lines
            .iter()
            .map(|(atom, flag)| (atom.to_string(), vec![flag.to_string()]))
            .collect(),
        ..Default::default()
    }];
    config
}

/// The one boot row of `result` as `(package, reduced flag)`.
fn boot_of(result: &GraphResult) -> Option<(String, String)> {
    let mut boots = result.entries.iter().filter_map(|e| {
        e.bootstrap
            .as_ref()
            .map(|b| (e.package.clone(), b.reduced_flag.clone()))
    });
    let first = boots.next();
    assert!(boots.next().is_none(), "at most one boot row");
    first
}

/// The enabled USE flags one row was built with, from its own display.
fn enabled_use(entry: &GraphEntry) -> BTreeSet<String> {
    entry
        .use_flags_display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.clone())
        .collect()
}

/// Charter 2a: the owner's pair serializes to the boot build, the middle and
/// the definitive rebuild, with exact USE sets, marker and `merge_order`.
#[test]
fn bootstrap_sequence_is_boot_middle_definitive() {
    let result = bootstrap_graph(&["dev-libs/w753pkgnew", "dev-libs/w753otherpkg"]);
    assert!(
        result.circular_deps.is_empty(),
        "the bootstrapped cycle no longer reports: {:?}",
        result.circular_deps
    );
    assert_eq!(result.entries.len(), 3, "{:#?}", result.entries);
    let boot = &result.entries[0];
    let middle = &result.entries[1];
    let definitive = &result.entries[2];
    assert_eq!(
        (boot.category.as_str(), boot.package.as_str()),
        ("dev-libs", "w753otherpkg")
    );
    assert_eq!(
        (middle.category.as_str(), middle.package.as_str()),
        ("dev-libs", "w753pkgnew")
    );
    assert_eq!(
        (definitive.category.as_str(), definitive.package.as_str()),
        ("dev-libs", "w753otherpkg")
    );
    for entry in [&boot, &middle, &definitive] {
        assert!(
            matches!(entry.outcome, PretendOutcome::New { .. }),
            "{entry:?}"
        );
    }
    let marker = boot.bootstrap.as_ref().expect("the boot row is marked");
    assert_eq!(marker.reduced_flag, "useboh");
    assert!(middle.bootstrap.is_none() && definitive.bootstrap.is_none());
    assert_eq!(enabled_use(boot), BTreeSet::from(["useblah".to_string()]));
    assert!(enabled_use(definitive).contains("useboh"));
    assert!(enabled_use(definitive).contains("useblah"));
    assert_eq!(
        enabled_use(middle),
        BTreeSet::from(["usefoo".to_string(), "use3".to_string()])
    );
}

/// The survivor invariant: the final row equals the declared build — the
/// same version, USE (reduced flags restored) and sub-slot as resolving
/// the slot once normally with the guard off (the S0 reference shape).
#[test]
fn bootstrap_survivor_equals_the_declared_build() {
    let result = bootstrap_graph(&["dev-libs/w753pkgnew", "dev-libs/w753otherpkg"]);
    assert_eq!(result.entries.len(), 3);
    let definitive = &result.entries[2];
    let root = fixtures_root();
    let mut off = test_config();
    off.use_tokens = vec!["-useboh".to_string()];
    let reference = ResolveRequest::new(
        &root,
        &root,
        &["dev-libs/w753otherpkg".to_string()],
        &off,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect("the guard-off reference resolves")
    .entries
    .into_iter()
    .find(|e| e.package == "w753otherpkg")
    .expect("the reference builds otherpkg");
    let version = |e: &GraphEntry| match &e.outcome {
        PretendOutcome::New { version } => version.clone(),
        other => panic!("a New row, not {other:?}"),
    };
    assert_eq!(version(definitive), version(&reference));
    assert_eq!(definitive.sub_slot, reference.sub_slot);
    let mut expected = enabled_use(&reference);
    expected.insert("useboh".to_string());
    assert_eq!(enabled_use(definitive), expected);
}

/// STOP rule 2: the reduced boot build still reaches the other member
/// through an unguarded build-time path (the soft `r1 -> r3 -> r2` arm),
/// so the bootstrap is refused and the cycle keeps today's failure.
#[test]
fn a_boot_build_that_still_needs_the_other_package_is_refused() {
    let result = bootstrap_graph(&["dev-libs/w753r1", "dev-libs/w753r2", "dev-libs/w753r3"]);
    assert!(result.entries.iter().all(|e| e.bootstrap.is_none()));
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// STOP rule 3: a longer loop needs more than 3 affected rows, so the
/// bootstrap is refused and the cycle keeps today's failure.
#[test]
fn more_than_three_builds_is_refused() {
    let result = bootstrap_graph(&["dev-libs/w753c1", "dev-libs/w753c2", "dev-libs/w753c3"]);
    assert!(result.entries.iter().all(|e| e.bootstrap.is_none()));
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// `--onlydeps` suppresses the top-level target from the merge: a boot
/// row for it would print nowhere and satisfy nothing, so the reduction
/// is refused and the cycle keeps today's failure.
#[test]
fn bootstrap_refused_for_a_suppressed_onlydeps_target() {
    let root = fixtures_root();
    let owned = ["dev-libs/usecyclea".to_string()];
    let result = ResolveRequest {
        onlydeps: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &owned,
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("the onlydeps resolve runs");
    assert!(result.entries.iter().all(|e| e.bootstrap.is_none()));
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// A bootstrap *is* merges: under `--buildpkgonly` the reduction is
/// `None`, so the cycle keeps today's failure instead of a half-applied
/// lie.
#[test]
fn bootstrap_refused_under_buildpkgonly() {
    let root = fixtures_root();
    let owned = [
        "dev-libs/w753pkgnew".to_string(),
        "dev-libs/w753otherpkg".to_string(),
    ];
    let result = ResolveRequest {
        buildpkgonly: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &owned,
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .expect("the buildpkgonly resolve runs");
    assert!(result.entries.iter().all(|e| e.bootstrap.is_none()));
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// A forced flag cannot be dropped (PMS `package.use.force`): the first
/// candidate `w753otherpkg`/`useboh` is forced, so the next one is tried.
#[test]
fn a_forced_flag_is_skipped_for_the_next_candidate() {
    let config = forcing(&[("dev-libs/w753otherpkg", "useboh")]);
    let result = bootstrap_graph_with(&["dev-libs/w753pkgnew", "dev-libs/w753otherpkg"], &config);
    assert_eq!(
        boot_of(&result),
        Some(("w753pkgnew".to_string(), "usefoo".to_string()))
    );
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
}

/// Every candidate flag forced: no bootstrap, the cycle is reported as before.
#[test]
fn only_forced_candidates_keep_the_cycle_report() {
    let config = forcing(&[
        ("dev-libs/w753otherpkg", "useboh"),
        ("dev-libs/w753pkgnew", "usefoo"),
    ]);
    let result = bootstrap_graph_with(&["dev-libs/w753pkgnew", "dev-libs/w753otherpkg"], &config);
    assert_eq!(boot_of(&result), None);
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// REQUIRED_USE is preferred, never required: `w759a` would be first but
/// removing `fa` violates its `REQUIRED_USE="fa"`, so `w759b`/`fb` wins.
#[test]
fn a_candidate_keeping_required_use_is_preferred() {
    let result = bootstrap_graph(&["dev-libs/w759a", "dev-libs/w759b"]);
    assert_eq!(
        boot_of(&result),
        Some(("w759b".to_string(), "fb".to_string()))
    );
}

/// With only a REQUIRED_USE-violating candidate the bootstrap still happens:
/// the boot build is temporary.
#[test]
fn a_required_use_violating_only_candidate_still_bootstraps() {
    let result = bootstrap_graph(&["dev-libs/w759c", "dev-libs/w759d"]);
    assert_eq!(
        boot_of(&result),
        Some(("w759c".to_string(), "fc".to_string()))
    );
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
}

/// Removing `fe` would switch on `!fe? ( dev-libs/w759x )`, a build-time
/// need the boot row cannot carry in order: the bootstrap is refused.
#[test]
fn a_negated_build_arm_switched_on_by_the_removal_refuses() {
    let result = bootstrap_graph(&["dev-libs/w759e", "dev-libs/w759f"]);
    assert_eq!(boot_of(&result), None);
    assert!(!result.circular_deps.is_empty(), "the cycle still reports");
}

/// A `!flag?` arm guards nothing: the flag is no dropping candidate for
/// it, and removing the flag switches the atom on.
#[test]
fn a_negated_guard_never_makes_the_flag_a_dropping_candidate() {
    let text = "!fe? ( dev-libs/x )";
    assert!(
        portage_use_reduce::conditional_guards(text)
            .iter()
            .all(|(_, guards)| guards.is_empty())
    );
    let metadata = HashMap::from([("DEPEND".to_string(), text.to_string())]);
    let declared = HashSet::from(["fe".to_string()]);
    assert!(flag_adds_build_dep(&metadata, &declared, "fe"));
    assert!(!flag_adds_build_dep(&metadata, &declared, "other"));
}
