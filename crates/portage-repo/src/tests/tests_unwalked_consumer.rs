//! Complete mode and the installed consumers the walk never recursed into.
//!
//! Expected values come from host Portage 3.0.83 with the same argv on a scratch
//! copy of the fixtures (`LLM/evidence/2026-11-06-sweep/<item>/`), cited per test.
//! `D` = `lib/_emerge/depgraph.py`.

use super::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_complete_in};
use super::*;

/// Writes an installed package (EAPI 8, slot 0, `testrepo`) into `root`'s vdb.
fn install(root: &Path, cpv: &str, files: &[(&str, &str)]) {
    let (category, pf) = cpv.split_once('/').expect("cat/pf");
    let dir = root.join("var/db/pkg").join(category).join(pf);
    fs::create_dir_all(&dir).unwrap();
    let base = [
        ("CATEGORY", category),
        ("EAPI", "8"),
        ("SLOT", "0"),
        ("repository", "testrepo"),
    ];
    for (name, content) in base.iter().chain(files) {
        fs::write(dir.join(name), format!("{content}\n")).unwrap();
    }
}

/// `emerge --pretend <atoms>` against `root`, with the world file as the
/// complete-graph seeds and `selective` off, as the CLI feeds a plain
/// `emerge -p <atom>`.
fn resolve_with_world(root: &Path, atoms: &[&str]) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.selective = false;
    let seeds: Vec<String> = fs::read_to_string(root.join("var/lib/portage/world"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .map(String::from)
        .collect();
    req.config.blocker_retry_seed_atoms = seeds.clone();
    req.config.complete_seed_atoms = seeds;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

fn outcome_of<'a>(result: &'a GraphResult, package: &str) -> &'a PretendOutcome {
    &result
        .entries
        .iter()
        .find(|e| e.package == package)
        .unwrap_or_else(|| panic!("{package} not in {:?}", result.entries))
        .outcome
}

#[test]
fn unwalked_consumer_atom_matching_nothing_installed_does_not_pin() {
    // #603. Installed `w603lib-3` and world-only `w603cons-1` (`<dev-libs/w603lib-2`).
    // Portage 3.0.83 `emerge -p dev-libs/w603lib` (`603/plain.portage.txt`): complete
    // mode walks the consumer, its atom matches no installed package, so it is filed
    // as initially unsatisfied and never a parent atom (D:8797-8806):
    // `U w603lib-3.1 [3]`, 0/20, no skip block.
    let root = hermetic_root(&["app-misc/w603cons"]);
    install(&root, "dev-libs/w603lib-3", &[]);
    install(
        &root,
        "app-misc/w603cons-1",
        &[("RDEPEND", "<dev-libs/w603lib-2")],
    );
    let result = resolve_with_world(&root, &["dev-libs/w603lib"]);
    assert_eq!(result.backtrack_restarts, 0);
    assert!(
        result.skipped_updates.is_empty(),
        "{:?}",
        result.skipped_updates
    );
    assert_eq!(
        outcome_of(&result, "w603lib"),
        &PretendOutcome::Upgrade {
            from: "3".into(),
            to: "3.1".into()
        }
    );
}

#[test]
fn unwalked_consumer_atom_matching_an_installed_package_still_pins() {
    // #603 counterpart. Installed `w603lib-1` matches `<dev-libs/w603lib-2`, so
    // `_complete_graph` adds it and the conflict forms (D:8807-8834). Portage 3.0.83
    // (`603/matching.portage.txt`): `R w603lib-1`, 1/20, skip row from `w603cons-1`.
    let root = hermetic_root(&["app-misc/w603cons"]);
    install(&root, "dev-libs/w603lib-1", &[]);
    install(
        &root,
        "app-misc/w603cons-1",
        &[("RDEPEND", "<dev-libs/w603lib-2")],
    );
    let result = resolve_with_world(&root, &["dev-libs/w603lib"]);
    assert_eq!(result.backtrack_restarts, 1);
    let skipped: Vec<&str> = result
        .skipped_updates
        .iter()
        .map(|r| r.atom.as_str())
        .collect();
    assert_eq!(skipped, vec!["<dev-libs/w603lib-2"]);
}

/// The #602 shape: installed `w602prov-1` (`0/1`) and `w602cons-1`
/// (`dev-libs/w602prov:0/1=`), in world.
fn w602_root() -> TempDir {
    let root = hermetic_root(&["app-misc/w602cons"]);
    install(&root, "dev-libs/w602prov-1", &[("SLOT", "0/1")]);
    install(
        &root,
        "app-misc/w602cons-1",
        &[("RDEPEND", "dev-libs/w602prov:0/1=")],
    );
    root
}

#[test]
fn slot_conflict_while_backtracking_is_reported_for_complete_mode() {
    // #602. `w602s` picks 2, `w602a` needs `<w602s-2`: a slot conflict. Portage
    // turns complete mode on when one appears while backtracking is allowed
    // (D:9473-9480); the CLI's second walk reads this flag (`602/` captures:
    // `r U w602prov-2`, `rR w602cons-1` with the complete-graph switches off).
    let root = w602_root();
    let three = ["=dev-libs/w602prov-2", "dev-libs/w602s", "app-misc/w602a"];
    let mut req = request_in(&root, &three);
    req.selective = false;
    let result = req.resolve().expect("resolves");
    assert!(result.slot_conflict_backtracking);
    // `--backtrack=0`: `_allow_backtracking` is false, no complete mode.
    let mut req = request_in(&root, &three);
    req.selective = false;
    req.backtrack_max = 0;
    assert!(!req.resolve().expect("resolves").slot_conflict_backtracking);
    // No conflict at all.
    let mut req = request_in(&root, &["=dev-libs/w602prov-2"]);
    req.selective = false;
    assert!(!req.resolve().expect("resolves").slot_conflict_backtracking);
}

/// The #605 shapes: installed `w605x-1` built with `w605flag` and one world
/// consumer per variant (`a`: `|| ( x[w605flag] y )` with `w605y-1` installed,
/// `c`: plain `x[w605flag]`). `w605x-2` merges with the flag off (profile default).
fn w605_root(variant: char) -> TempDir {
    let (consumer, rdepend) = match variant {
        'a' => ("w605cona", "|| ( dev-libs/w605x[w605flag] dev-libs/w605y )"),
        _ => ("w605conc", "dev-libs/w605x[w605flag]"),
    };
    let world = format!("app-misc/{consumer}");
    let root = hermetic_root(&[world.as_str()]);
    install(
        &root,
        "dev-libs/w605x-1",
        &[("IUSE", "w605flag"), ("USE", "w605flag")],
    );
    if variant == 'a' {
        install(&root, "dev-libs/w605y-1", &[]);
    }
    install(
        &root,
        &format!("app-misc/{consumer}-1"),
        &[("RDEPEND", rdepend)],
    );
    root
}

#[test]
fn post_merge_use_reads_the_merge_node_use() {
    // #605: in `_graph_trees` the `[use]` of an `||` arm is matched against the
    // merge node's own USE (DC:492-495), not the tree or the vdb.
    let root = w605_root('c');
    let node = |on: bool| GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1".into(),
            to: "2".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        use_flags_display: vec![("w605flag".to_string(), on)],
        ..graph_entry("dev-libs", "w605x", "2")
    };
    let config = test_config();
    let off = PostMergeSet::build(&root, &[node(false)]);
    assert!(off.matches("dev-libs/w605x"));
    assert!(!off.use_matches("dev-libs/w605x[w605flag]", &config));
    let on = PostMergeSet::build(&root, &[node(true)]);
    assert!(on.use_matches("dev-libs/w605x[w605flag]", &config));
    // No merge node: the installed `w605x-1` keeps its recorded USE.
    let kept = PostMergeSet::build(&root, &[]);
    assert!(kept.use_matches("dev-libs/w605x[w605flag]", &config));
}

#[test]
fn upgrade_breaking_an_unwalked_consumer_use_dep_conflicts() {
    // #671. Portage 3.0.83 `emerge -p =dev-libs/w605x-2` (`605/C-p`): complete mode
    // walks `w605conc`, `w605x[w605flag]` still matches the installed `w605x-1`, so
    // it is pulled back in: a slot conflict, rc 1, "Change USE: +w605flag".
    // Variant A (`605/A-p`): the post-merge `||` takes `w605y` (#605), no conflict.
    let c = w605_root('c');
    let result = resolve_complete_in(&c, &["=dev-libs/w605x-2"]);
    let rows: Vec<&str> = result
        .slot_conflicts
        .iter()
        .flat_map(|sc| sc.instances.iter().map(|i| i.version.as_str()))
        .collect();
    assert!(rows.contains(&"1") && rows.contains(&"2"), "{rows:?}");
    let a = w605_root('a');
    let result = resolve_complete_in(&a, &["=dev-libs/w605x-2"]);
    assert!(
        result.slot_conflicts.is_empty(),
        "{:?}",
        result.slot_conflicts
    );
}

/// `emerge -puD @world`: the world atoms are both the request and the seeds.
fn resolve_update_deep_world(root: &Path) -> GraphResult {
    let world: Vec<String> = fs::read_to_string(root.join("var/lib/portage/world"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .map(String::from)
        .collect();
    let atoms: Vec<&str> = world.iter().map(String::as_str).collect();
    let mut req = request_in(root, &atoms);
    req.update = true;
    req.deep = Deep::Unlimited;
    req.selective = true;
    req.config.blocker_retry_seed_atoms = world.clone();
    req.config.complete_seed_atoms = world;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving -uD @world failed: {e}"))
}

#[test]
fn update_held_back_by_an_installed_consumers_use_dep_is_reported() {
    // #699. Installed `w605x-1` (built `w605flag`), world consumer `w605conc-1`
    // (`dev-libs/w605x[w605flag]`), visible `w605x-2` with the flag off. `-uD`
    // keeps `w605x-1`; the plan is healthy but the user must be told that
    // `w605x-2` exists and `+w605flag` lets it in (charter property 5).
    // Portage 3.0.83 fails with the same hint (comparison only).
    let root = w605_root('c');
    let result = resolve_update_deep_world(&root);
    assert_eq!(
        result.held_back_use,
        vec![HeldBackUse {
            category: "dev-libs".to_string(),
            package: "w605x".to_string(),
            slot: "0".to_string(),
            installed_version: "1".to_string(),
            skipped_version: "2".to_string(),
            atom: "dev-libs/w605x[w605flag]".to_string(),
            consumer: "app-misc/w605conc-1".to_string(),
            changes: vec!["+w605flag".to_string()],
        }]
    );
}

#[test]
fn an_or_arm_the_installed_consumer_does_not_need_holds_nothing_back() {
    // #699 negative: `|| ( w605x[w605flag] w605y )` with `w605y` installed is
    // met without `w605x`, so `w605x-2` is simply not required by the consumer.
    let root = w605_root('a');
    let result = resolve_update_deep_world(&root);
    assert!(
        result.held_back_use.is_empty(),
        "{:?}",
        result.held_back_use
    );
}

#[test]
fn p700_empty_world_consumer_reached_through_a_merge_bound_package() {
    // #700, charter property 1. The #676 shape with an empty world: `p590vpull-2` names the
    // installed `virtual/p590vvb-1` (`=p590vprov-5.42*` or `=5.38*`), which keeps
    // `p590vprov-5.44` out of the plan even though no world atom reaches it.
    let root = hermetic_root(&[]);
    let result = resolve_with_world(root.path(), &["dev-libs/p590vpull"]);
    assert!(matches!(
        outcome_of(&result, "p590vpull"),
        PretendOutcome::New { version } if version == "1"
    ));
    assert!(matches!(
        outcome_of(&result, "p590vprov"),
        PretendOutcome::AlreadyInstalled { version } if version == "5.42"
    ));
    assert!(
        result
            .skipped_updates
            .iter()
            .any(|r| r.package == "p590vprov" && r.consumer_cpv.starts_with("virtual/p590vvb-1")),
        "{:?}",
        result.skipped_updates
    );
}
