//! Complete mode and the installed consumers the walk never recursed into.
//!
//! Expected values come from host Portage 3.0.83 with the same argv on a scratch
//! copy of the fixtures (`LLM/evidence/2026-11-06-sweep/<item>/`), cited per test.
//! `D` = `lib/_emerge/depgraph.py`.

use super::tests_consumer_pin_conflict::{hermetic_root, request_in};
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
