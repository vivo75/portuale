//! Charter 2a bootstrap (backlog #753): the resolver synthesizes a
//! reduced-USE boot row after the sort instead of failing the genuine
//! USE-conditional cycle. The owner's pair resolves to three rows —
//! `otherpkg[boot] -> pkgnew -> otherpkg` — with the `bootstrap` marker
//! on the boot row only; unbreakable shapes keep today's failure.

use super::*;

/// The owner's pair over the fixture tree, both flags default-on: the
/// genuine build-time 2-cycle the #749 report names.
fn bootstrap_graph(atoms: &[&str]) -> GraphResult {
    let root = fixtures_root();
    let owned: Vec<String> = atoms.iter().map(|a| a.to_string()).collect();
    ResolveRequest::new(
        &root,
        &root,
        &owned,
        &test_config(),
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {owned:?} failed: {e}"))
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

/// Charter 2a (backlog #753): the owner's pair serializes to exactly three
/// rows — the reduced-USE boot build, the middle, the definitive rebuild —
/// with exact USE sets, the exact marker, and exact `merge_order` 0/1/2.
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
