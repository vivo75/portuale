/// Graph-input unit tests (mutation cluster 5): walk/installed/vdb,
/// merge-order, blockers, use. Each test observes the function's own
/// return value. Portage grounding: 3.0.82.2 (`3rdparty/portage`).
use crate::*;
use portage_util::TempDir;

/// Fresh unique scratch dir per test (vdb + repo live under it, so
/// the per-root memo caches -- `all_installed_packages`,
/// `installed_candidates` -- never see a reused path).
fn dir_165(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-165-{tag}")).keep()
}

fn install_165(root: &Path, cat: &str, pf: &str, slot: &str, files: &[(&str, &str)]) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    for (name, contents) in files {
        std::fs::write(d.join(name), format!("{contents}\n")).unwrap();
    }
}

// ---- `find_repos_impl` survivors (Portage RepoConfigLoader). ----

/// The main repo defaults to priority -1000, every other repo to 0
/// (Portage `config.py`'s own default).
#[test]
fn find_repos_impl_defaults_the_main_repo_to_priority_minus_1000() {
    let root = dir_165("repos-priority");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n\n[overlay]\nlocation = overlay\n",
    )
    .unwrap();
    let repos = find_repos(&root).expect("repos.conf resolves");
    assert_eq!(
        repos.iter().find(|r| r.name == "main").unwrap().priority,
        -1000
    );
    assert_eq!(
        repos.iter().find(|r| r.name == "overlay").unwrap().priority,
        0
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn cfg_165() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

fn entry_165(cat: &str, pkg: &str, outcome: PretendOutcome, slot: Option<&str>) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: slot.map(str::to_string),
        sub_slot: slot.map(str::to_string),
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
        deps: Vec::new(),
    }
}

fn new_165(cat: &str, pkg: &str, ver: &str) -> GraphEntry {
    entry_165(
        cat,
        pkg,
        PretendOutcome::New {
            version: ver.to_string(),
        },
        Some("0"),
    )
}

fn ipkg_165(cat: &str, pkg: &str, ver: &str, slot: &str) -> InstalledPackage {
    InstalledPackage {
        category: cat.to_string(),
        package: pkg.to_string(),
        version: ver.to_string(),
        slot: slot.to_string(),
    }
}

/// A cross-category edge orders too.
#[test]
fn topological_removal_order_follows_a_cross_category_edge() {
    let root = dir_165("order-cross");
    let d = root.join("var/db/pkg/dev-libs").join("aa-1.0");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), "0\n").unwrap();
    std::fs::write(d.join("RDEPEND"), "sys-libs/bb\n").unwrap();
    let e = root.join("var/db/pkg/sys-libs").join("bb-1.0");
    std::fs::create_dir_all(&e).unwrap();
    std::fs::write(e.join("SLOT"), "0\n").unwrap();
    let (ordered, out) = topological_removal_order(
        &root,
        vec![
            ipkg_165("sys-libs", "bb", "1.0", "0"),
            ipkg_165("dev-libs", "aa", "1.0", "0"),
        ],
    );
    assert!(ordered);
    assert_eq!(
        out.iter().map(|p| p.cpv()).collect::<Vec<_>>(),
        vec!["dev-libs/aa-1.0", "sys-libs/bb-1.0"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The highest-version guard does not depend on scan order (sorted
/// readdir or creation order): 10.0 is kept over 9.0 either way.
/// Portage comparison is order-independent.
#[test]
fn prune_cleanlist_protects_the_highest_regardless_of_install_order() {
    let root = dir_165("prune-reverse-order");
    install_165(&root, "dev-libs", "aa-10.0", "0", &[]);
    install_165(&root, "dev-libs", "aa-9.0", "0", &[]);
    let result = prune_cleanlist(&root, &[], &[], &[]);
    assert_eq!(
        result.cleanlist.iter().map(|p| p.cpv()).collect::<Vec<_>>(),
        vec!["dev-libs/aa-9.0"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---- `resolve_blockers` (Portage `_validate_blockers`). ----

fn upgrade_165(cat: &str, pkg: &str, from: &str, to: &str) -> GraphEntry {
    entry_165(
        cat,
        pkg,
        PretendOutcome::Upgrade {
            from: from.to_string(),
            to: to.to_string(),
        },
        Some("0"),
    )
}
/// Arguments of [`pending_165`].
struct Pending165Args<'a> {
    atom: &'a str,
    strong: bool,
    buildtime: bool,
    target: (&'a str, &'a str),
    owner: (&'a str, &'a str),
    owner_version: &'a str,
    owner_merging: bool,
    owner_installed: bool,
}

fn pending_165(args: Pending165Args<'_>) -> PendingBlocker {
    let Pending165Args {
        atom,
        strong,
        buildtime,
        target,
        owner,
        owner_version,
        owner_merging,
        owner_installed,
    } = args;
    PendingBlocker {
        atom_str: atom.to_string(),
        strong,
        buildtime,
        target_category: target.0.to_string(),
        target_package: target.1.to_string(),
        owner_key: (owner.0.to_string(), owner.1.to_string()),
        owner_version: owner_version.to_string(),
        owner_merging,
        owner_installed,
    }
}

/// `Upgrade`, `Downgrade` and `Reinstall` merge-bound targets join
/// the candidate set exactly like `New` ones.
#[test]
fn resolve_blockers_matches_upgrade_downgrade_and_reinstall_targets() {
    let mk = |outcome| {
        resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &[pending_165(Pending165Args {
                atom: "!!dev-libs/target",
                strong: true,
                buildtime: true,
                target: ("dev-libs", "target"),
                owner: ("dev-libs", "owner"),
                owner_version: "1.0",
                owner_merging: true,
                owner_installed: false,
            })],
            &[
                new_165("dev-libs", "owner", "1.0"),
                entry_165("dev-libs", "target", outcome, Some("0")),
            ],
            &HashSet::new(),
            false,
        )
    };
    for (outcome, version) in [
        (
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            "2.0",
        ),
        (
            PretendOutcome::Downgrade {
                from: "2.0".to_string(),
                to: "1.0".to_string(),
            },
            "1.0",
        ),
        (
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
            "1.0",
        ),
    ] {
        let conflicts = mk(outcome);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict.matched_version, version);
        assert!(conflicts[0].conflict.unsolvable);
    }
}

/// A merge-bound target at a new slot joins the candidates beside
/// the installed one.
#[test]
fn resolve_blockers_sees_a_merge_bound_target_beside_its_installed_slot() {
    let root = dir_165("blocker-slotmove");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let entries = vec![
        new_165("dev-libs", "owner", "1.0"),
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "1.0".to_string(),
            },
            Some("1"),
        ),
    ];
    let conflicts = resolve_blockers(
        &root,
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/target:1",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict.matched_version, "1.0");
    assert!(conflicts[0].conflict.unsolvable);
    let _ = std::fs::remove_dir_all(&root);
}

/// `installed_match` needs slot and version: installed `target-1.0:0`
/// beside the merge-bound twin leaves the `:1` row an unsolvable
/// uninstall.
#[test]
fn resolve_blockers_requires_slot_and_version_for_an_installed_match() {
    let root = dir_165("blocker-installed-match");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let entries = vec![
        new_165("dev-libs", "owner", "1.0"),
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "1.0".to_string(),
            },
            Some("1"),
        ),
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "2.0".to_string(),
            },
            Some("1"),
        ),
    ];
    let conflicts = resolve_blockers(
        &root,
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/target:1",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 2);
    let twin = conflicts
        .iter()
        .find(|c| c.conflict.matched_version == "1.0")
        .expect("the :1 twin files");
    assert!(twin.conflict.unsolvable);
    assert!(twin.conflict.satisfied_by.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// A strong blocker against its owner's own slot is exempt from
/// the soft same-slot skip.
#[test]
fn resolve_blockers_keeps_a_strong_blocker_against_its_own_slot() {
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/owner",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "owner"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &[new_165("dev-libs", "owner", "1.0")],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].conflict.unsolvable);
}

/// Cell f: an installed-only match for a nomerge owner is ignored
/// outright.
#[test]
fn resolve_blockers_ignores_an_installed_only_match_for_a_nomerge_owner() {
    let root = dir_165("blocker-cell-f");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let entries = vec![entry_165(
        "dev-libs",
        "owner",
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        },
        Some("0"),
    )];
    let conflicts = resolve_blockers(
        &root,
        &[pending_165(Pending165Args {
            atom: "!dev-libs/target",
            strong: false,
            buildtime: false,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: false,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// A use-dep blocker is evaluated against the matched version's own
/// entry: `targetA` (flag off) does not satisfy, `targetB` (flag
/// on) does, so only the 2.0 row files.
#[test]
fn resolve_blockers_evaluates_use_deps_against_the_matched_versions_entry() {
    let mut target_a = new_165("dev-libs", "target", "1.0");
    target_a.use_flags_display = vec![("wantblock".to_string(), false)];
    let mut target_b = upgrade_165("dev-libs", "target", "1.0", "2.0");
    target_b.use_flags_display = vec![("wantblock".to_string(), true)];
    let entries = vec![target_a, target_b, new_165("dev-libs", "owner", "1.0")];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/target[wantblock]",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict.matched_version, "2.0");
    assert!(conflicts[0].conflict.unsolvable);
}

/// `merge_bound_match` needs the entry's slot and version: an
/// installed 1.0 beside a slot-moved merge-bound 9.9 leaves the 1.0
/// row solvable (removed by uninstall).
#[test]
fn resolve_blockers_needs_slot_and_version_for_a_merge_bound_match() {
    let root = dir_165("blocker-merge-bound-match");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let entries = vec![
        new_165("dev-libs", "owner", "1.0"),
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "9.9".to_string(),
            },
            Some("5"),
        ),
    ];
    let conflicts = resolve_blockers(
        &root,
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/target",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 2);
    let row_1_0 = conflicts
        .iter()
        .find(|c| c.conflict.matched_version == "1.0")
        .expect("installed row filed");
    assert!(!row_1_0.conflict.unsolvable);
    assert!(matches!(
        row_1_0.conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall { .. })
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// The non-merge fallback owner lookup is same-cp: with no owner
/// entry, another cp's `AlreadyInstalled` row must not stand in.
#[test]
fn resolve_blockers_falls_back_to_a_same_cp_nomerge_owner_only() {
    let root = dir_165("blocker-owner-fallback");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let other = entry_165(
        "dev-libs",
        "other",
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        },
        Some("0"),
    );
    let entries = vec![
        other,
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "2.0".to_string(),
            },
            Some("0"),
        ),
    ];
    let conflicts = resolve_blockers(
        &root,
        &[pending_165(Pending165Args {
            atom: "!!dev-libs/target",
            strong: true,
            buildtime: true,
            target: ("dev-libs", "target"),
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            owner_merging: true,
            owner_installed: false,
        })],
        &entries,
        &HashSet::new(),
        false,
    );
    let row_2_0 = conflicts
        .iter()
        .find(|c| c.conflict.matched_version == "2.0")
        .expect("merge-bound row filed");
    assert!(row_2_0.conflict.unsolvable);
    let _ = std::fs::remove_dir_all(&root);
}

// ---- `file_blocker_conflicts` and
// `collect_unwalked_installed_blockers` (Portage `_validate_blockers`). ----
/// Arguments of [`filed_165`].
struct Filed165Args<'a> {
    owner: (&'a str, &'a str),
    owner_version: &'a str,
    atom: &'a str,
    strong: bool,
    target: (&'a str, &'a str, &'a str),
    unsolvable: bool,
    satisfied_by: Option<BlockerSatisfiedBy>,
}

fn filed_165(args: Filed165Args<'_>) -> FiledBlocker {
    let Filed165Args {
        owner,
        owner_version,
        atom,
        strong,
        target,
        unsolvable,
        satisfied_by,
    } = args;
    FiledBlocker {
        owner: (owner.0.to_string(), owner.1.to_string()),
        owner_version: owner_version.to_string(),
        conflict: BlockerConflict {
            atom_str: atom.to_string(),
            strong,
            matched_category: target.0.to_string(),
            matched_package: target.1.to_string(),
            matched_version: target.2.to_string(),
            unsolvable,
            satisfied_by,
            tree_scheduled_uninstall: false,
        },
    }
}

fn uninstall_165(cpv: &str, anchor: (&str, &str)) -> BlockerSatisfiedBy {
    BlockerSatisfiedBy::Uninstall {
        cpv: cpv.to_string(),
        anchor: (anchor.0.to_string(), anchor.1.to_string()),
    }
}

/// An owned conflict lands on its entry *and* spawns the removal
/// its `Uninstall` tag names.
#[test]
fn file_blocker_conflicts_files_an_owned_row_on_its_entry_and_removal() {
    let root = dir_165("file-owned");
    install_165(&root, "dev-libs", "victim-1.0", "0", &[]);
    let mut entries = vec![new_165("dev-libs", "owner", "1.0")];
    let mut orphans = Vec::new();
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![filed_165(Filed165Args {
            owner: ("dev-libs", "owner"),
            owner_version: "1.0",
            atom: "!!dev-libs/victim",
            strong: true,
            target: ("dev-libs", "victim", "1.0"),
            unsolvable: false,
            satisfied_by: Some(uninstall_165("dev-libs/victim-1.0", ("dev-libs", "owner"))),
        })],
        &mut orphans,
    );
    assert!(orphans.is_empty());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].blockers.len(), 1);
    assert_eq!(entries[0].blockers[0].atom_str, "!!dev-libs/victim");
    assert!(matches!(
        entries[1].outcome,
        PretendOutcome::Uninstall { .. }
    ));
    assert_eq!(entries[1].category, "dev-libs");
    assert_eq!(entries[1].package, "victim");
    assert!(entries[1].blockers.is_empty());
    assert_eq!(
        entries[1].required_by,
        vec![("dev-libs".to_string(), "owner".to_string())]
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---- `enqueue_dependencies` (`--deep` AlreadyInstalled recursion). ----
/// Arguments of [`enqueue_165`].
struct Enqueue165Args<'a> {
    repos: &'a [RepoConfig],
    root: &'a Path,
    config: &'a portage_profile::Config,
    cat: &'a str,
    pkg: &'a str,
    ver: &'a str,
    with_bdeps: bool,
    entries: &'a mut Vec<GraphEntry>,
    queue: &'a mut std::collections::VecDeque<QueueItem>,
    pending: &'a mut Vec<PendingBlocker>,
}

fn enqueue_165(args: Enqueue165Args<'_>) -> SlotPullers {
    let Enqueue165Args {
        repos,
        root,
        config,
        cat,
        pkg,
        ver,
        with_bdeps,
        entries,
        queue,
        pending,
    } = args;
    let mut seen = HashSet::new();
    let mut memo = HashMap::new();
    let mut pullers = HashMap::new();
    enqueue_dependencies(
        &SelectSource {
            repos,
            root,
            config,
            local_binpkg: &std::sync::Arc::new(BinaryIndex::default()),
        },
        &DepOwner {
            // Same-root test walk: no cross-root routing here.
            target_root: root,
            category: cat,
            package: pkg,
            version: ver,
            child_depth: 1,
        },
        &EnqueueOptions {
            dynamic_deps: false,
            ignore_built_slot_operator_deps: false,
            with_bdeps,
            root_deps_running_root: None,
            update: false,
        },
        EnqueueSinks {
            queue,
            pending_blockers: pending,
            entries,
            root_deps_build_seen: &mut seen,
            installed_meta_memo: &mut memo,
            slot_pullers: &mut pullers,
        },
        &HashMap::new(),
    );
    pullers
}
