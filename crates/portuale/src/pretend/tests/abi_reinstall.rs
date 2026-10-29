//! The `r` bracket's pair rule: a merge-bound consumer of an ABI-rebuild edge
//! is not force-reinstalled, an installed one is.
//!
//! Portage tags a merge row `r` when one of its atoms carries
//! `arg.force_reinstall` (`resolver/output.py`, `PkgAttrDisplay.force_reinstall`;
//! `depgraph.py`, `_iter_atoms_for_pkg`), and the only such atoms here are the
//! `@__auto_slot_operator_replace_installed__` set's (`_gen_reinstall_sets`,
//! `depgraph.py:5479-5480`). A merge-bound parent is never filed into that set:
//! the trigger probes only a built parent (`:3124-3127`) and
//! `_slot_operator_update_backtrack` files `dep.parent` only when it is
//! installed (`:2453-2456`). It still appears in the "causing rebuilds" block,
//! which walks `_slot_operator_deps` and needs no `built` flag
//! (`_compute_abi_rebuild_info`, `:931-1145`).

use super::*;
use portage_repo::{CandidateSource, PretendOutcome, VisibilityProvenance};
use std::fs;

/// One graph entry with everything but the outcome/slot left empty.
fn entry(
    category: &str,
    package: &str,
    outcome: PretendOutcome,
    slot: &str,
    sub_slot: &str,
) -> GraphEntry {
    GraphEntry {
        category: category.to_string(),
        package: package.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: Some(slot.to_string()),
        sub_slot: Some(sub_slot.to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        build_time: None,
        deps: Vec::new(),
        discovery: 0,
    }
}

/// A ROOT with the pair's provider installed, so the provider side of the edge
/// passes its own `in_slot` check and the assertions stay about the consumer.
fn root_with_provider(tag: &str) -> portage_util::TempDir {
    let base = portage_util::TempDir::new(&format!("pretend-abi-r-{tag}"));
    let dir = base.join("var/db/pkg/dev-libs/abiprov-2");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(dir.join("SLOT"), "0/2\n").unwrap();
    fs::write(dir.join("repository"), "testrepo\n").unwrap();
    base
}

/// `(dev-libs/abiprov-2, dev-libs/abicons-*)`: the pair both halves are read from.
fn pair(consumer_version: &str) -> Vec<(String, String)> {
    vec![(
        "dev-libs/abiprov-2".to_string(),
        format!("dev-libs/abicons-{consumer_version}"),
    )]
}

fn cps(
    pairs: &[(String, String)],
    entries: &[GraphEntry],
    replaced: &[(String, String)],
    root: &Path,
) -> HashSet<(String, String)> {
    abi_rebuild_force_reinstall_cps(AbiRebuildForceReinstallArgs {
        abi_rebuilds: pairs,
        entries,
        slot_operator_replaced: replaced,
        root,
    })
}

/// A consumer upgrading in the same run is already merging: no `r`, even
/// though it is one of the block's rows.
#[test]
fn a_merge_bound_upgrade_consumer_is_not_force_reinstalled() {
    let base = root_with_provider("upgrade");
    let entries = vec![
        entry(
            "dev-libs",
            "abiprov",
            PretendOutcome::Upgrade {
                from: "1".to_string(),
                to: "2".to_string(),
            },
            "0",
            "2",
        ),
        entry(
            "dev-libs",
            "abicons",
            PretendOutcome::Upgrade {
                from: "1".to_string(),
                to: "2".to_string(),
            },
            "0",
            "0",
        ),
    ];
    let got = cps(&pair("2"), &entries, &[], &base);
    assert!(
        got.contains(&("dev-libs".to_string(), "abiprov".to_string())),
        "the provider side keeps its `r`: {got:?}"
    );
    assert!(
        !got.contains(&("dev-libs".to_string(), "abicons".to_string())),
        "a merge-bound upgrade consumer is not force-reinstalled: {got:?}"
    );
}

/// The same consumer named by the replace set (`new_dep.parent.slot_atom`,
/// `depgraph.py:2452-2456`) does get the `r`.
#[test]
fn a_merge_bound_upgrade_consumer_in_the_replace_set_is_force_reinstalled() {
    let base = root_with_provider("upgrade-replaced");
    let entries = vec![
        entry(
            "dev-libs",
            "abiprov",
            PretendOutcome::Upgrade {
                from: "1".to_string(),
                to: "2".to_string(),
            },
            "0",
            "2",
        ),
        entry(
            "dev-libs",
            "abicons",
            PretendOutcome::Upgrade {
                from: "1".to_string(),
                to: "2".to_string(),
            },
            "0",
            "0",
        ),
    ];
    let replaced = vec![("dev-libs".to_string(), "abicons".to_string())];
    let got = cps(&pair("2"), &entries, &replaced, &base);
    assert!(
        got.contains(&("dev-libs".to_string(), "abicons".to_string())),
        "a replace-set consumer keeps its `r`: {got:?}"
    );
}

/// The `p589b`/`p569` shape: the consumer is an installed rebuild (`rR`), not a
/// version move, so the pair alone still tags it.
#[test]
fn an_installed_consumer_of_an_abi_edge_is_force_reinstalled() {
    let base = root_with_provider("installed");
    let entries = vec![
        entry(
            "dev-libs",
            "abiprov",
            PretendOutcome::Upgrade {
                from: "1".to_string(),
                to: "2".to_string(),
            },
            "0",
            "2",
        ),
        entry(
            "dev-libs",
            "abicons",
            PretendOutcome::Reinstall {
                version: "1".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: true,
            },
            "0",
            "0",
        ),
    ];
    let got = cps(&pair("1"), &entries, &[], &base);
    assert!(
        got.contains(&("dev-libs".to_string(), "abicons".to_string())),
        "an installed rebuild consumer keeps its `r`: {got:?}"
    );
}
