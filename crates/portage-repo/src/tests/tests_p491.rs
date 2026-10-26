//! The installed-parent update probe, hermetically.
//!
//! Installed `p491con-1` (`:6`, vdb `>=dev-libs/p491lib-1:6/7=`) with tree
//! replacement `p491con-2` (`:6`, `>=dev-libs/p491lib-1:6`) pulling new child
//! `p491lib-2` (`6/8`), while installed `p491keep-1` (no tree ebuild, pins
//! `dev-libs/p491lib:6/7=`) keeps the old child graphed. Mirrors the live
//! libplasma shape: installed plasma-pa-6.7.5 vs the 6.8.1 ebuilds, keeper
//! merkuro-26.08.1-r1 ("cannot be rebuilt, ebuild unavailable").
//!
//! Pass 0 seeds installed consumers whose replacement parent pulls a new
//! child sub-slot, keeps the installed child graphed, and retries —
//! `fold_rebuild_scan` only runs inside SlotConflict feedback, so nothing
//! seeds try 1.
//! Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls,
//! _slot_operator_update_probe, _slot_operator_update_backtrack.

use super::*;

/// Temp vdb: installed `p491lib-1` (`lib_slot`), `p491con-1` (`:6`, baked
/// `>=dev-libs/p491lib-1:6/7=`), `p491keep-1` (`:6`, `keep_rdepend`).
/// Built fresh per test: the vdb backend snapshots file content.
fn p491_vdb(lib_slot: &str, keep_rdepend: &str) -> PathBuf {
    let dir = slotundo_temp_dir("p491-seed");
    for (pf, slot, rdepend) in [
        ("p491lib-1", lib_slot, None),
        ("p491con-1", "6", Some(">=dev-libs/p491lib-1:6/7=")),
        ("p491keep-1", "6", Some(keep_rdepend)),
    ] {
        let d = dir.join(format!("var/db/pkg/dev-libs/{pf}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs").unwrap();
        fs::write(d.join("EAPI"), "8").unwrap();
        fs::write(d.join("SLOT"), slot).unwrap();
        if let Some(deps) = rdepend {
            fs::write(d.join("RDEPEND"), deps).unwrap();
        }
        fs::write(d.join("repository"), "testrepo").unwrap();
    }
    dir
}

/// An installed entry the pass graphs (selective-keep, not replaced).
fn installed_entry(cat: &str, pkg: &str, ver: &str, slot: &str, sub: &str) -> GraphEntry {
    GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: ver.to_string(),
        },
        slot: Some(slot.to_string()),
        sub_slot: Some(sub.to_string()),
        ..graph_entry(cat, pkg, ver)
    }
}

fn p491_entries() -> Vec<GraphEntry> {
    vec![
        installed_entry("dev-libs", "p491con", "1", "6", "6"),
        installed_entry("dev-libs", "p491lib", "1", "6", "7"),
        installed_entry("dev-libs", "p491keep", "1", "6", "6"),
    ]
}

fn p491_probe(
    root: &Path,
    entries: &[GraphEntry],
    top_level_atoms: HashSet<String>,
    update: bool,
) -> InstalledParentUpdateProbe {
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let installed = InstalledReachability::new(root);
    // The keeper the pass keeps: its pin judges the candidate child.
    let live = HashSet::from([("dev-libs".to_string(), "p491keep".to_string())]);
    installed_parent_update_probe(InstalledParentUpdateProbeArgs {
        root,
        config: &test_config(),
        entries,
        with_bdeps: false,
        excluded: &[],
        masked: &HashMap::new(),
        installed: &installed,
        live: &live,
        walked: &HashSet::new(),
        top_level_atoms: &top_level_atoms,
        update,
        gate: crate::test_gate(&repos),
    })
}

#[test]
fn p491_pass0_seed_files_the_installed_consumer_and_the_kept_child() {
    // Portage oracle (analysis.md: try-1 24-atom `slot_operator_replace_installed`
    // set): the installed consumer joins with its installed child, the keeper's
    // built pin does not veto (it relaxes: the keeper "may need to be rebuilt").
    // Portage: depgraph.py, _slot_operator_update_backtrack (both filings).
    let root = p491_vdb("6/7", "dev-libs/p491lib:6/7=");
    let entries = p491_entries();
    let replace = p491_probe(&root, &entries, HashSet::new(), true);
    assert_eq!(
        replace,
        BTreeSet::from([
            ("dev-libs".to_string(), "p491con".to_string()),
            ("dev-libs".to_string(), "p491lib".to_string()),
        ]),
        "the installed parent and the kept installed child seed the retry"
    );
}

#[test]
fn p491_pass0_seed_refuses_an_argument_pinned_parent() {
    // Portage oracle (`captures/p491-debug-v1.stderr`: no update-probe line —
    // the pinned consumer's dep wants no update, so the tries stay pure
    // slot-conflict/missing-dep masks): `=dev-libs/p491con-1` pins the parent
    // below its tree replacement.
    // Portage: depgraph.py, _slot_operator_trigger_reinstalls (want_update gate).
    let root = p491_vdb("6/7", "dev-libs/p491lib:6/7=");
    let entries = p491_entries();
    let replace = p491_probe(
        &root,
        &entries,
        HashSet::from(["=dev-libs/p491con-1".to_string()]),
        true,
    );
    assert!(
        replace.is_empty(),
        "an argument-pinned parent wants no update: {replace:?}"
    );
}

#[test]
fn p491_pass0_seed_refuses_a_strict_pin_on_the_new_child() {
    // Portage oracle (`_slot_operator_check_reverse_dependencies` returns
    // False): a non-built pin the candidate child violates vetoes the probe —
    // only built `:S/SS=` pins relax. The keeper here pins `<dev-libs/p491lib-2`
    // (strict), which `p491lib-2` does not satisfy.
    let root = p491_vdb("6/7", "<dev-libs/p491lib-2");
    let entries = p491_entries();
    let replace = p491_probe(&root, &entries, HashSet::new(), true);
    assert!(
        replace.is_empty(),
        "a strict pin on the new child vetoes the seed: {replace:?}"
    );
}

#[test]
fn p491_pass0_seed_stays_quiet_without_update() {
    // Portage oracle (plain-mode probe cells: backtrack 0/20, both managers
    // agree): without `--update` an installed parent's dep wants no update.
    // Portage: depgraph.py, _slot_operator_trigger_reinstalls (want_update gate).
    let root = p491_vdb("6/7", "dev-libs/p491lib:6/7=");
    let entries = p491_entries();
    let replace = p491_probe(&root, &entries, HashSet::new(), false);
    assert!(replace.is_empty(), "no update mode, no seed: {replace:?}");
}

#[test]
fn p491_pass0_seed_stays_quiet_when_the_child_already_moved() {
    // Control: the installed child already sits in the new sub-slot, so no
    // tree instance offers another one (mirrors the built-parent probe's
    // same-subslot control).
    let root = p491_vdb("6/8", "dev-libs/p491lib:6/7=");
    let entries = p491_entries();
    let replace = p491_probe(&root, &entries, HashSet::new(), true);
    assert!(
        replace.is_empty(),
        "no sub-slot change, no seed: {replace:?}"
    );
}

/// Temp vdb for the `||` probe: the installed parent's slot-operator atom
/// sits in a `||` branch the installed set does not select, while the
/// sibling branch names an installed package. TempDir-guarded (no `.keep()`).
fn p491or_vdb() -> (TempDir, PathBuf) {
    let guard = TempDir::new("p491-or-seed");
    let root = guard.path().to_path_buf();
    for (pf, slot, rdepend) in [
        ("p491lib-1", "6/7", None),
        (
            "p491orcon-1",
            "6",
            Some("|| ( =dev-libs/p491lib-2:6/7= dev-libs/p491oralt )"),
        ),
        ("p491oralt-1", "0", None),
        ("p491keep-1", "6", Some("dev-libs/p491lib:6/7=")),
    ] {
        let d = root.join(format!("var/db/pkg/dev-libs/{pf}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs").unwrap();
        fs::write(d.join("EAPI"), "8").unwrap();
        fs::write(d.join("SLOT"), slot).unwrap();
        if let Some(deps) = rdepend {
            fs::write(d.join("RDEPEND"), deps).unwrap();
        }
        fs::write(d.join("repository"), "testrepo").unwrap();
    }
    (guard, root)
}

#[test]
fn p491or_seed_ignores_a_deselected_or_branch_slot_operator_atom() {
    // Portage `dep_zapdeps` (`3rdparty/portage/lib/portage/dep/dep_check.py`,
    // the `||` choice bins) resolves each `||` group installed-preferred:
    // the `=dev-libs/p491lib-2:6/7=` branch matches no installed instance
    // while the `dev-libs/p491oralt` sibling matches installed `p491oralt-1`,
    // so only the sibling becomes a parent atom and the slot-operator
    // trigger (`_emerge/depgraph.py`, `_slot_operator_trigger_reinstalls`,
    // which ranges over recorded `_slot_operator_deps` only) never sees the
    // slot-operator branch. Host Portage 3.0.83 on the matching cell
    // (world `[dev-libs/p491orcon, dev-libs/p491keep]`, `emerge --pretend
    // --update --deep --backtrack=4 @world` with the fixture configroot and
    // an ad-hoc ROOT): merges `p491orcon-2` only, keeps `p491lib-1`
    // (`6/7`), and prints no `missed slot abi update` debug line — no seed.
    // Portage: dep_check.py, dep_zapdeps; depgraph.py,
    // _slot_operator_trigger_reinstalls.
    let (_guard, root) = p491or_vdb();
    let entries = vec![
        installed_entry("dev-libs", "p491orcon", "1", "6", "6"),
        installed_entry("dev-libs", "p491lib", "1", "6", "7"),
        installed_entry("dev-libs", "p491oralt", "1", "0", "0"),
        installed_entry("dev-libs", "p491keep", "1", "6", "6"),
    ];
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let installed = InstalledReachability::new(&root);
    let live = HashSet::from([("dev-libs".to_string(), "p491keep".to_string())]);
    let replace = installed_parent_update_probe(InstalledParentUpdateProbeArgs {
        root: &root,
        config: &test_config(),
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        masked: &HashMap::new(),
        installed: &installed,
        live: &live,
        walked: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        update: true,
        gate: crate::test_gate(&repos),
    });
    assert!(
        replace.is_empty(),
        "a deselected || branch is no probe atom: {replace:?}"
    );
}
