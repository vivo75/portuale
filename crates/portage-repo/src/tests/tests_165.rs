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
