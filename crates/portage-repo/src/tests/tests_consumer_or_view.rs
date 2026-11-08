//! An installed consumer's `||` is chosen the way `dep_zapdeps` chooses it, in the
//! view Portage uses (`_filtered_trees` in the walk, `_graph_trees` in a complete
//! graph), and not at all when Portage never evaluates it.
//!
//! Expected values come from host Portage 3.0.83 on scratch copies of the fixtures
//! (`LLM/evidence/2026-10-26-575-residues/t10b/captures/`), named per test.
//! `D` = `lib/_emerge/depgraph.py`, `DC` = `lib/portage/dep/dep_check.py`.

use super::tests_consumer_pin_conflict::{
    complete_request_in, hermetic_root, request_in, resolve_in,
};
use super::*;

/// The merge rows `(cat/pkg, version)` of a result, `U`/`N` alike, in list order.
fn merges(result: &GraphResult) -> Vec<(String, String)> {
    result
        .entries
        .iter()
        .filter_map(|e| {
            let version = match &e.outcome {
                PretendOutcome::New { version } => version,
                PretendOutcome::Upgrade { to, .. } => to,
                _ => return None,
            };
            Some((format!("{}/{}", e.category, e.package), version.clone()))
        })
        .collect()
}

fn rows(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

/// The re-walk `--deep` makes after the first walk: seeds fed, `complete` off.
fn deep_request_in(root: &Path, atoms: &[&str]) -> ResolveRequest {
    let seeds: Vec<String> = fs::read_to_string(root.join("var/lib/portage/world"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .map(String::from)
        .collect();
    let mut req = request_in(root, atoms);
    req.deep = Deep::Unlimited;
    let mut seeds = seeds;
    seeds.extend(req.config.profile_packages.iter().cloned());
    seeds.extend(req.config.system_packages.iter().cloned());
    seeds.sort();
    seeds.dedup();
    req.config.blocker_retry_seed_atoms = seeds.clone();
    req.config.complete_seed_atoms = seeds;
    req
}

/// `--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n`.
fn without_auto_complete(mut req: ResolveRequest) -> ResolveRequest {
    req.complete_if_new_ver = false;
    req.complete_if_new_use = false;
    req.rebuild_if_new_slot = false;
    req
}

#[test]
fn deep_rewalk_keeps_the_walkers_pick_for_a_recursed_consumer() {
    // Portage 3.0.83 `emerge -pv --deep dev-libs/p589apull` (captures
    // `t1-deep-portage.txt`): `p589avirt` is added in the walk and `deep is True`, so its
    // `||` is chosen in `_filtered_trees` (D:3834, D:3838-3842): `~p589acore-1.0`
    // is graphed and merged, `=5.42*` is never a parent atom. 0/20, rc 0.
    let root = hermetic_root(&[]);
    let result = deep_request_in(&root, &["dev-libs/p589apull"])
        .resolve()
        .expect("resolves");
    assert!(
        matches!(result.outcome, ResolveOutcome::Complete),
        "no spurious slot conflict: {:?}",
        result.slot_conflicts
    );
    assert!(
        result.slot_conflicts.is_empty(),
        "{:?}",
        result.slot_conflicts
    );
    assert_eq!(result.backtrack_restarts, 0);
    assert!(result.skipped_updates.is_empty());
    assert_eq!(
        merges(&result),
        rows(&[
            ("dev-libs/p589aprov", "5.44"),
            ("dev-libs/p589acore", "1.0"),
            ("dev-libs/p589apull", "2"),
        ])
    );
}

#[test]
fn consumer_or_without_complete_mode_forms_no_pin() {
    // Portage 3.0.83 with the three `=n` switches (captures `t3-nocomplete-portage.txt`,
    // `t1-nocomplete-portage.txt`): complete mode is never entered, so the installed
    // virtual's `||` is never evaluated (D:8693-8694) and adds no parent atom. T3's
    // `=5.42*` first arm would pin otherwise.
    let root = hermetic_root(&[]);
    for (puller, prov) in [
        ("dev-libs/p589cpull", "dev-libs/p589cprov"),
        ("dev-libs/p589apull", "dev-libs/p589aprov"),
    ] {
        let result = without_auto_complete(request_in(&root, &[puller]))
            .resolve()
            .expect("resolves");
        assert_eq!(result.backtrack_restarts, 0, "{puller}");
        assert!(result.skipped_updates.is_empty(), "{puller}");
        assert_eq!(
            merges(&result),
            vec![
                (prov.to_string(), "5.44".to_string()),
                (puller.to_string(), "2".to_string())
            ],
            "{puller}"
        );
    }
}

#[test]
fn explicit_complete_graph_chooses_the_consumer_or_postmerge() {
    // Portage 3.0.83 `--complete-graph` with the `=n` switches (capture
    // `t1-cg-portage.txt`): the virtual's `||` is chosen in `_graph_trees`, where 5.42 is
    // replaced by the 5.44 node. No arm is available, the first listed (`=5.40*`)
    // wins (DC:379-380), it matches no installed package, so no pin forms
    // (D:8803-8809): `U p589aprov-5.44`, `N p589apull-2`, 0/20.
    let root = hermetic_root(&[]);
    let result = without_auto_complete(complete_request_in(&root, &["dev-libs/p589apull"]))
        .resolve()
        .expect("resolves");
    assert_eq!(result.backtrack_restarts, 0);
    assert!(result.skipped_updates.is_empty());
    assert_eq!(
        merges(&result),
        rows(&[("dev-libs/p589aprov", "5.44"), ("dev-libs/p589apull", "2")])
    );
}

#[test]
fn t1_swap_noslotop_merges_the_update_like_portage() {
    // T1. Portage 3.0.83 `emerge -pv dev-libs/p589apull` (capture `t1-plain-portage.txt`):
    // `U p589aprov-5.44`, `N p589apull-2`, no skip block, 0/20. The virtual is parked
    // in `_ignored_deps`, so a complete graph chooses its `||` in `_graph_trees`
    // (`s0a-t1-nodeep`): `=5.40*` is chosen and matches nothing installed.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p589apull"]);
    assert_eq!(result.backtrack_restarts, 0);
    assert!(result.skipped_updates.is_empty());
    assert!(result.slot_conflicts.is_empty());
    assert_eq!(
        merges(&result),
        rows(&[("dev-libs/p589aprov", "5.44"), ("dev-libs/p589apull", "2")])
    );
}

#[test]
fn t2_swap_slotop_matches_portage() {
    // T2. Portage 3.0.83 `emerge -pv dev-libs/p589bpull` (capture `t2-plain-portage.txt`):
    // 4/20, `N p589bpull-1`, one skip row `dev-libs/p589bprov:0/5.42=` from the
    // installed virtual. The `||` arm `=5.40*` is chosen and matches nothing installed,
    // so it forms no parent atom; the built `:=` atom is the only conflict parent.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p589bpull"]);
    assert_eq!(result.backtrack_restarts, 4);
    assert_eq!(
        merges(&result),
        rows(&[("dev-libs/p589bpull", "1")]),
        "the old puller merges"
    );
    let skipped: Vec<&str> = result
        .skipped_updates
        .iter()
        .map(|r| r.atom.as_str())
        .collect();
    assert_eq!(skipped, vec!["dev-libs/p589bprov:0/5.42="]);
}

#[test]
fn t3_keeps_the_pin_of_a_selected_arm_the_vdb_matches() {
    // T3. Portage 3.0.83 `emerge -pv dev-libs/p589cpull` (capture `t3-plain-portage.txt`):
    // the virtual lists `=5.42*` first, the set holds no replaced arm, `=5.42*` is
    // chosen and matches the installed 5.42, so it still pins: 4/20, `N p589cpull-1`,
    // skip row `=dev-libs/p589cprov-5.42*`.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p589cpull"]);
    assert_eq!(result.backtrack_restarts, 4);
    let skipped: Vec<&str> = result
        .skipped_updates
        .iter()
        .map(|r| r.atom.as_str())
        .collect();
    assert_eq!(skipped, vec!["=dev-libs/p589cprov-5.42*"]);
}

#[test]
fn world_only_consumer_under_deep_is_chosen_postmerge() {
    // Portage 3.0.83 `emerge -pv --deep =dev-libs/p589aprov-5.44` with `p589avirt` in
    // world (capture `t1-revdep-deep-inworld-portage.txt`): the virtual is reached only
    // through `@world` (depth `_UNREACHABLE_DEPTH`), so its `||` is chosen in
    // `_graph_trees` even with `--deep` and forms no pin: `U p589aprov-5.44`, 0/20, rc 0.
    let root = hermetic_root(&["dev-libs/p589avirt"]);
    let result = deep_request_in(&root, &["=dev-libs/p589aprov-5.44"])
        .resolve()
        .expect("resolves");
    assert!(
        matches!(result.outcome, ResolveOutcome::Complete),
        "{:?}",
        result.slot_conflicts
    );
    assert_eq!(result.backtrack_restarts, 0);
    assert!(
        result.slot_conflicts.is_empty(),
        "{:?}",
        result.slot_conflicts
    );
    assert_eq!(merges(&result), rows(&[("dev-libs/p589aprov", "5.44")]));
}

#[test]
fn sibling_virtuals_share_the_atom_a_sibling_chose() {
    // Quirk Q19. `p590vpar` (installed, in world) expands `virtual/p590vva` (arms
    // `=5.44*`, `=5.42*`) and `virtual/p590vvb` (arms `=5.42*`, `=5.38*`). The second
    // chooses `=5.42*`, the first `=5.44*`; interning makes the two `=5.42*` atoms one
    // object, so the first carries it too (D:6122-6133, D:6164-6167). Portage 3.0.83
    // `emerge -pv dev-libs/p590vpull` (capture `q19-order-portage.txt`): both virtuals
    // are listed as `=dev-libs/p590vprov-5.42*` parents in the skip block.
    let root = hermetic_root(&["dev-libs/p590vpar"]);
    let result = complete_request_in(&root, &["dev-libs/p590vpull"])
        .resolve()
        .expect("resolves");
    let mut parents: Vec<(&str, &str)> = result
        .skipped_updates
        .iter()
        .map(|r| (r.consumer_cpv.as_str(), r.atom.as_str()))
        .collect();
    parents.sort_unstable();
    assert_eq!(
        parents,
        vec![
            (
                "virtual/p590vva-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
            (
                "virtual/p590vvb-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
        ]
    );
}

// ---- API-level tests (the selection function, the demotions, the walker's record) ----

/// The installed consumer's `RDEPEND` and recorded USE.
fn installed_rdepend(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> (String, HashSet<String>) {
    (
        read_vdb_string(root, category, package, version, "RDEPEND"),
        read_vdb_flag_set(root, category, package, version, "USE"),
    )
}

/// The `||` arms `dep_zapdeps` selects for an installed consumer, post-merge.
fn postmerge_selection(
    root: &Path,
    entries: &[GraphEntry],
    consumer: (&str, &str, &str),
) -> HashSet<String> {
    let req = request_in(root, &["dev-libs/p589apull"]);
    let ctx = ResolveCtx::new(&req).expect("ctx");
    let (depstr, use_flags) = installed_rdepend(root, consumer.0, consumer.1, consumer.2);
    let split = split_or_groups(&depstr, &use_flags).expect("parses");
    let set = PostMergeSet::build(root, entries);
    installed_consumer_or_selection(InstalledConsumerOrSelectionArgs {
        split: &split,
        use_flags: &use_flags,
        consumer: &(consumer.0.to_string(), consumer.1.to_string()),
        config: ctx.config,
        root,
        entries,
        update: false,
        root_deps_running_root: None,
        gate: ctx.gate(),
        view: OrView::PostMerge(&set),
        // PostMerge consumers never want an update (pinned).
        want_parent: None,
        want_depth: 0,
        want_deep: Deep::NotRequested,
        want_arg_atoms: &[],
        want_set_args: None,
        want_excluded: &[],
        want_complete: false,
    })
}

fn set_of(list: &[&str]) -> HashSet<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn consumer_or_postmerge_picks_the_first_available_arm() {
    // T1 shape. Portage 3.0.83 (`s0a-t1-nodeep`): with 5.42 replaced by the 5.44
    // node and nothing else graphed, no arm of `p589avirt` is available, so the
    // first listed wins (DC:379-380, PT:230-252). With `--deep` the walk merges
    // `p589acore` (`s0a-t1-deep`), `~p589acore-1.0` is available in the set and beats
    // the earlier unavailable arms.
    let root = hermetic_root(&[]);
    let plain = resolve_in(&root, &["dev-libs/p589apull"]);
    let virt = ("dev-libs", "p589avirt", "1");
    assert_eq!(
        postmerge_selection(&root, &plain.entries, virt),
        set_of(&["=dev-libs/p589aprov-5.40*"])
    );
    let deep = deep_request_in(&root, &["dev-libs/p589apull"])
        .resolve()
        .expect("resolves");
    assert_eq!(
        postmerge_selection(&root, &deep.entries, virt),
        set_of(&["~dev-libs/p589acore-1.0"])
    );
}

#[test]
fn consumer_or_postmerge_keeps_an_unreplaced_installed_arm() {
    // DC:603-616, PT:230-252. With no node replacing the installed 5.42, the arm
    // `=5.42*` is available through the installed package and beats the first-listed
    // `=5.40*`.
    let root = hermetic_root(&[]);
    assert_eq!(
        postmerge_selection(&root, &[], ("dev-libs", "p589avirt", "1")),
        set_of(&["=dev-libs/p589aprov-5.42*"])
    );
}

/// The arms of `p589?virt` the scan leaves out of its parent atoms after `entries`.
fn demoted_arms(root: &Path, entries: &[GraphEntry], virt: &str) -> HashSet<String> {
    let req = request_in(root, &["dev-libs/p589apull"]);
    let ctx = ResolveCtx::new(&req).expect("ctx");
    let (depstr, use_flags) = installed_rdepend(root, "dev-libs", virt, "1");
    let walked = HashMap::new();
    let scan = OrScan {
        portage_complete: true,
        installed_walked: &walked,
        update: false,
        selective: true,
        backtrack_allowed: true,
        excluded: &[],
        arg_atoms: &[],
        root_deps_running_root: None,
        post_merge: PostMergeSet::build(root, entries),
    };
    let consumer = InstalledPackage {
        category: "dev-libs".into(),
        package: virt.into(),
        version: "1".into(),
        slot: "0".into(),
    };
    consumer_or_demotions(ConsumerOrDemotionsArgs {
        scan: &scan,
        consumer: &consumer,
        depstr: &depstr,
        use_flags: &use_flags,
        leaked: None,
        entries,
        root,
        config: ctx.config,
        gate: ctx.gate(),
    })
}

#[test]
fn selected_arm_matching_nothing_forms_no_pin() {
    // D:8803-8809: a dependency of a complete-graph consumer that matches no
    // installed package is filed as initially unsatisfied and forms no parent atom.
    // T1: the chosen `=5.40*` matches nothing installed, so every arm is demoted. T3:
    // the chosen `=5.42*` matches the installed 5.42 and stays (Portage 4/20 with
    // that skip row, capture `t3-plain-portage.txt`).
    let root = hermetic_root(&[]);
    let t1 = resolve_in(&root, &["dev-libs/p589apull"]);
    let all_a = set_of(&[
        "=dev-libs/p589aprov-5.40*",
        "=dev-libs/p589aprov-5.42*",
        "=dev-libs/p589aprov-5.38*",
        "~dev-libs/p589acore-1.0",
    ]);
    assert_eq!(demoted_arms(&root, &t1.entries, "p589avirt"), all_a);
    let t3 = resolve_in(&root, &["dev-libs/p589cpull"]);
    let upgraded_prov = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "5.42".into(),
            to: "5.44".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("5.44".into()),
        ..graph_entry("dev-libs", "p589cprov", "5.44")
    };
    let mut entries: Vec<GraphEntry> = t3
        .entries
        .iter()
        .filter(|e| e.package != "p589cprov")
        .cloned()
        .collect();
    entries.push(upgraded_prov);
    let demoted = demoted_arms(&root, &entries, "p589cvirt");
    assert!(
        !demoted.contains("=dev-libs/p589cprov-5.42*"),
        "the vdb-matched arm keeps its pin: {demoted:?}"
    );
    assert!(demoted.contains("=dev-libs/p589cprov-5.40*"));
}

#[test]
fn walker_records_the_picks_of_an_installed_consumer_it_recursed_into() {
    // `--deep` recurses into `p589avirt` (D:3834); the pick the walk made for its
    // `||` is `~p589acore-1.0` (capture `s0a-t1-deep`). Without `--deep` the walk
    // does not recurse and records nothing.
    let root = hermetic_root(&[]);
    let req = deep_request_in(&root, &["dev-libs/p589apull"]);
    let ctx = ResolveCtx::new(&req).expect("ctx");
    let params = BacktrackParams::initial(&req);
    let pass = run_pass(&ctx, &params, true).expect("pass");
    let key = (
        "dev-libs".to_string(),
        "p589avirt".to_string(),
        "1".to_string(),
    );
    assert_eq!(
        pass.installed_walked.get(&key),
        Some(&set_of(&["~dev-libs/p589acore-1.0"]))
    );
    let req = request_in(&root, &["dev-libs/p589apull"]);
    let ctx = ResolveCtx::new(&req).expect("ctx");
    let pass = run_pass(&ctx, &BacktrackParams::initial(&req), true).expect("pass");
    assert!(pass.installed_walked.is_empty());
}

#[test]
fn sibling_virtuals_named_in_a_build_time_key_leak_through_their_rdepend() {
    // Quirk Q19 with the virtuals named in the installed parent's BDEPEND only
    // (`p590wpar`, in world). A virtual contributes its RDEPEND whatever key the
    // parent names it in (GLEP 37, `dep_check.py` 217-220), so both virtuals carry
    // the sibling's `=5.42*`. Bed evidence: Portage 3.0.83 evaluates
    // `dev-perl/List-MoreUtils-XS`'s BDEPEND virtuals in complete mode
    // (`t9/captures/bed/xmlparser-vids.txt`) and lists `virtual/perl-File-Path`'s
    // `=5.42*` among the skip rows (L0 `l0-20261027T183625Z`). The fixture cell cannot
    // be captured with host Portage: with ROOT other than `/` it resolves BDEPEND
    // against the host root.
    let root = hermetic_root(&["dev-libs/p590wpar"]);
    let mut req = complete_request_in(&root, &["dev-libs/p590wpull"]);
    req.with_bdeps = true;
    let result = req.resolve().expect("resolves");
    let mut parents: Vec<(&str, &str)> = result
        .skipped_updates
        .iter()
        .map(|r| (r.consumer_cpv.as_str(), r.atom.as_str()))
        .collect();
    parents.sort_unstable();
    assert_eq!(
        parents,
        vec![
            (
                "virtual/p590vva-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
            (
                "virtual/p590vvb-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
        ]
    );
}

#[test]
fn sibling_virtuals_of_a_merge_bound_parent_share_the_atom_a_sibling_chose() {
    // Quirk Q19 with the parent a graph node being merged (`p590xpull-1` names both
    // installed virtuals in its RDEPEND), not an installed package: the walk's
    // dep_check of the merge-bound parent attributes the sibling's `=5.42*` to
    // `virtual/p590vva` too. Portage 3.0.83 `emerge -pv dev-libs/p590xpull`
    // (capture `q19-merge-portage.txt`): both virtuals' `=5.42*` are skip rows. On the
    // bed this is `dev-lang/perl-5.44.0`'s PDEPEND naming `virtual/perl-Data-Dumper` and
    // `virtual/perl-CPAN`, whose only parent is the merge-bound perl.
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p590xpull"]);
    let mut parents: Vec<(&str, &str)> = result
        .skipped_updates
        .iter()
        .map(|r| (r.consumer_cpv.as_str(), r.atom.as_str()))
        .collect();
    parents.sort_unstable();
    assert_eq!(
        parents,
        vec![
            (
                "virtual/p590vva-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
            (
                "virtual/p590vvb-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
        ]
    );
}

// ---- #604: the leak is keyed like Portage's atom intern key ----

/// A scratch ROOT with `w604prov-5.42` installed, plus `virtual/w604va-1` when
/// `with_virtual`.
fn w604_root(with_virtual: bool) -> TempDir {
    let root = hermetic_root(&[]);
    let mut pkgs = vec![("dev-libs", "w604prov-5.42", "0/5.42", "")];
    if with_virtual {
        pkgs.push((
            "virtual",
            "w604va-1",
            "0",
            "|| ( =dev-libs/w604prov-5.44* =dev-libs/w604prov-5.42* )",
        ));
    }
    for (cat, pf, slot, rdepend) in pkgs {
        let d = root.join("var/db/pkg").join(cat).join(pf);
        fs::create_dir_all(&d).unwrap();
        for (name, value) in [
            ("CATEGORY", cat),
            ("SLOT", slot),
            ("EAPI", "8"),
            ("repository", "testrepo"),
            ("RDEPEND", rdepend),
        ] {
            if !value.is_empty() {
                fs::write(d.join(name), format!("{value}\n")).unwrap();
            }
        }
    }
    root
}

/// The `w604prov` conflict's `(instance version, parent cpv, atom)` rows, sorted, of
/// `emerge -p --backtrack=0 --verbose-conflicts w604force <parent>`.
fn w604_rows(root: &Path, parent: &str) -> Vec<(String, String, String)> {
    let mut req = request_in(root, &["dev-libs/w604force", parent]);
    req.backtrack_max = 0;
    let result = req.resolve().expect("resolves");
    let conflicts: Vec<&SlotConflict> = result
        .slot_conflicts
        .iter()
        .filter(|c| c.package == "w604prov")
        .collect();
    assert_eq!(conflicts.len(), 1, "one conflict block: {conflicts:#?}");
    let mut rows: Vec<(String, String, String)> = conflicts[0]
        .instances
        .iter()
        .flat_map(|i| {
            i.parents
                .iter()
                .map(|p| (i.version.clone(), p.parent_cpv.clone(), p.atom.clone()))
        })
        .collect();
    rows.sort();
    rows
}

fn w604_row(ver: &str, parent: &str, atom_ver: &str) -> (String, String, String) {
    (
        ver.to_string(),
        format!("{parent}:0/0::testrepo"),
        format!("=dev-libs/w604prov-{atom_ver}*"),
    )
}

#[test]
fn installed_virtual_takes_no_atom_from_its_parent() {
    // #604. The parent's plain `=5.42*` never shares the virtual's dep_check
    // (D:4872-4904), and its `||` pick is parsed with EAPI 8 where the installed
    // virtual's atoms carry none (DC:866-871), so neither leaks into the installed
    // virtual. Portage 3.0.83, `-p --backtrack=0 --verbose-conflicts` (pmtest
    // `test_604_*`, shapes A and C): the virtual is never a `=5.42*` parent, and the
    // conflict prints once.
    for parent in ["dev-libs/w604par", "dev-libs/w604paror"] {
        let root = w604_root(true);
        let rows = w604_rows(&root, parent);
        assert!(
            !rows.contains(&w604_row("5.42", "virtual/w604va-1", "5.42")),
            "{parent}: {rows:#?}"
        );
        assert!(
            rows.contains(&w604_row("5.42", &format!("{parent}-1"), "5.42")),
            "{parent}: {rows:#?}"
        );
    }
}

#[test]
fn merge_bound_virtual_takes_the_parents_or_pick_of_its_eapi() {
    // #604. With the virtual merge-bound its RDEPEND is parsed with its own EAPI 8,
    // the parent's: the parent's `||` pick `=5.42*` is the virtual's own atom object,
    // so the virtual is a `=5.42*` parent too (D:6122-6133). A plain parent atom is
    // not in the call and does not leak. Portage 3.0.83 (shapes D and B).
    let root = w604_root(false);
    assert_eq!(
        w604_rows(&root, "dev-libs/w604paror"),
        vec![
            w604_row("5.42", "dev-libs/w604paror-1", "5.42"),
            w604_row("5.42", "virtual/w604va-1", "5.42"),
            w604_row("5.44", "dev-libs/w604force-1", "5.44"),
            w604_row("5.44", "virtual/w604va-1", "5.44"),
        ]
    );
    assert_eq!(
        w604_rows(&root, "dev-libs/w604par"),
        vec![
            w604_row("5.42", "dev-libs/w604par-1", "5.42"),
            w604_row("5.44", "dev-libs/w604force-1", "5.44"),
            w604_row("5.44", "virtual/w604va-1", "5.44"),
        ]
    );
}

// ---- #606: an installed consumer's selected arm is a parent of the merge it selects ----

#[test]
fn installed_virtual_selecting_the_merge_is_its_parent() {
    // #606. Complete mode evaluates the installed `virtual/p590vva`; with
    // `p590vprov-5.44` in the graph its `=5.44*` arm is chosen, and a dependency the
    // graph satisfies is a parent atom of that node (D:8799-8809 only sends
    // graph-unsatisfied ones to the vdb test). Portage 3.0.83 `emerge -p
    // --backtrack=0 --verbose-conflicts dev-libs/p590vpull` (clusterB `q19-bt0`): the
    // 5.44 instance lists `p590vpull-2` and `p590vva-1`; the 5.42 one both virtuals.
    let root = hermetic_root(&["dev-libs/p590vpar"]);
    let mut req = complete_request_in(&root, &["dev-libs/p590vpull"]);
    req.backtrack_max = 0;
    let result = req.resolve().expect("resolves");
    let sc = result
        .slot_conflicts
        .iter()
        .find(|c| c.package == "p590vprov")
        .expect("a p590vprov conflict");
    let mut rows: Vec<(String, String, String)> = sc
        .instances
        .iter()
        .flat_map(|i| {
            i.parents
                .iter()
                .map(|p| (i.version.clone(), p.parent_cpv.clone(), p.atom.clone()))
        })
        .collect();
    rows.sort();
    let row = |v: &str, p: &str, a: &str| {
        (
            v.to_string(),
            format!("{p}:0/0::testrepo"),
            format!("=dev-libs/p590vprov-{a}*"),
        )
    };
    assert_eq!(
        rows,
        vec![
            row("5.42", "virtual/p590vva-1", "5.42"),
            row("5.42", "virtual/p590vvb-1", "5.42"),
            row("5.44", "dev-libs/p590vpull-2", "5.44"),
            row("5.44", "virtual/p590vva-1", "5.44"),
        ]
    );
}

#[test]
fn q19_backtracks_six_times_like_portage() {
    // #606. Both choices carry two conflict atoms, so the version-desc order stands
    // and 5.42 is masked first: 5.42, +5.44, +p590vpull-2, the dead end on the
    // argument, then 5.44 alone and +p590vpull-2 (Portage 3.0.83 `--debug`, clusterB
    // `q19-portage-debug`): `backtrack: 6/20`, `N p590vpull-1`.
    let root = hermetic_root(&["dev-libs/p590vpar"]);
    let result = complete_request_in(&root, &["dev-libs/p590vpull"])
        .resolve()
        .expect("resolves");
    assert_eq!(result.backtrack_restarts, 6);
    assert_eq!(merges(&result), rows(&[("dev-libs/p590vpull", "1")]));
}
