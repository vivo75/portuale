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

/// Arguments of [`write_pkg_165`].
struct WritePkg165Args<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    iuse: &'a str,
    depend: &'a str,
    rdepend: &'a str,
    bdepend: &'a str,
}

/// One scratch ebuild plus its md5-cache entry, with optional
/// dependency keys.
fn write_pkg_165(args: WritePkg165Args<'_>) {
    let WritePkg165Args {
        repo,
        cp,
        pv,
        slot,
        iuse,
        depend,
        rdepend,
        bdepend,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body =
        format!("EAPI=8\nDESCRIPTION=\"165 graph inputs\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    if !depend.is_empty() {
        writeln!(body, "DEPEND=\"{depend}\"").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
    }
    if !bdepend.is_empty() {
        writeln!(body, "BDEPEND=\"{bdepend}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=165 graph inputs\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    if !depend.is_empty() {
        writeln!(entry, "DEPEND={depend}").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(entry, "RDEPEND={rdepend}").unwrap();
    }
    if !bdepend.is_empty() {
        writeln!(entry, "BDEPEND={bdepend}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

fn repo_165(dir: &Path, pkgs: &[(&str, &str, &str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse, depend, rdepend, bdepend) in pkgs {
        write_pkg_165(WritePkg165Args {
            repo: &repo,
            cp,
            pv,
            slot,
            iuse,
            depend,
            rdepend,
            bdepend,
        });
    }
    vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo,
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }]
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

/// A nomerge owner in the required-set closure still blocks on a
/// merge-bound match (Portage's "digraph node with parents").
#[test]
fn resolve_blockers_unblocks_a_nomerge_row_outside_the_required_sets() {
    let root = dir_165("blocker-sets");
    install_165(&root, "dev-libs", "target-1.0", "0", &[]);
    let owner = entry_165(
        "dev-libs",
        "owner",
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        },
        Some("0"),
    );
    let entries = vec![
        owner,
        entry_165(
            "dev-libs",
            "target",
            PretendOutcome::New {
                version: "2.0".to_string(),
            },
            Some("0"),
        ),
    ];
    let pending = vec![pending_165(Pending165Args {
        atom: "!dev-libs/target",
        strong: false,
        buildtime: false,
        target: ("dev-libs", "target"),
        owner: ("dev-libs", "owner"),
        owner_version: "1.0",
        owner_merging: false,
        owner_installed: false,
    })];
    // Outside every required set the merge-bound match resolves by
    // removing the owner.
    let solvable = resolve_blockers(&root, &pending, &entries, &HashSet::new(), false);
    let by_version: HashMap<String, &FiledBlocker> = solvable
        .iter()
        .map(|c| (c.conflict.matched_version.clone(), c))
        .collect();
    assert_eq!(by_version.len(), 2);
    assert!(!by_version["2.0"].conflict.unsolvable);
    // Inside the closure the same row is unresolved.
    let closure = HashSet::from([("dev-libs".to_string(), "owner".to_string())]);
    let stuck = resolve_blockers(&root, &pending, &entries, &closure, false);
    let row_2_0 = stuck
        .iter()
        .find(|c| c.conflict.matched_version == "2.0")
        .expect("merge-bound row filed");
    assert!(row_2_0.conflict.unsolvable);
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

/// Two owned conflicts against one removal share it (blockers pile
/// up, the anchor is recorded once).
#[test]
fn file_blocker_conflicts_merges_two_owned_rows_into_one_removal() {
    let root = dir_165("file-merge");
    install_165(&root, "dev-libs", "victim-1.0", "0", &[]);
    let mut entries = vec![new_165("dev-libs", "owner", "1.0")];
    let mut orphans = Vec::new();
    let anchor = ("dev-libs", "owner");
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![
            filed_165(Filed165Args {
                owner: ("dev-libs", "owner"),
                owner_version: "1.0",
                atom: "!!dev-libs/victim",
                strong: true,
                target: ("dev-libs", "victim", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            }),
            filed_165(Filed165Args {
                owner: ("dev-libs", "owner"),
                owner_version: "1.0",
                atom: "!dev-libs/victim",
                strong: false,
                target: ("dev-libs", "victim", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            }),
        ],
        &mut orphans,
    );
    assert!(orphans.is_empty());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].blockers.len(), 2);
    assert_eq!(entries[1].blockers.len(), 0);
    assert_eq!(entries[1].required_by, vec![anchor_to_vec_165(anchor)]);
    let _ = std::fs::remove_dir_all(&root);
}

fn anchor_to_vec_165(anchor: (&str, &str)) -> (String, String) {
    (anchor.0.to_string(), anchor.1.to_string())
}

/// Conflicts against different removals stay separate.
#[test]
fn file_blocker_conflicts_keeps_different_removals_separate() {
    let root = dir_165("file-separate");
    install_165(&root, "dev-libs", "victim-1.0", "0", &[]);
    install_165(&root, "dev-libs", "other-1.0", "0", &[]);
    let mut entries = vec![new_165("dev-libs", "owner", "1.0")];
    let mut orphans = Vec::new();
    let anchor = ("dev-libs", "owner");
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![
            filed_165(Filed165Args {
                owner: ("dev-libs", "owner"),
                owner_version: "1.0",
                atom: "!!dev-libs/victim",
                strong: true,
                target: ("dev-libs", "victim", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            }),
            filed_165(Filed165Args {
                owner: ("dev-libs", "owner"),
                owner_version: "1.0",
                atom: "!!dev-libs/other",
                strong: true,
                target: ("dev-libs", "other", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165("dev-libs/other-1.0", anchor)),
            }),
        ],
        &mut orphans,
    );
    assert!(orphans.is_empty());
    assert_eq!(entries.len(), 3);
    let mut pkgs: Vec<String> = entries[1..].iter().map(|e| e.package.clone()).collect();
    pkgs.sort();
    assert_eq!(pkgs, vec!["other".to_string(), "victim".to_string()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// With no owner entry anywhere, `Uninstall` rows still ride their
/// removal (the vdb-scan nomerge shape): two rows against one share
/// it, a same-version other-package row stays separate.
#[test]
fn file_blocker_conflicts_rides_ownerless_rows_on_their_removals() {
    let root = dir_165("file-ownerless");
    install_165(&root, "dev-libs", "victim-1.0", "0", &[]);
    install_165(&root, "dev-libs", "other-1.0", "0", &[]);
    let mut entries: Vec<GraphEntry> = Vec::new();
    let mut orphans = Vec::new();
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![
            filed_165(Filed165Args {
                owner: ("dev-libs", "scanner"),
                owner_version: "1.0",
                atom: "!!dev-libs/victim",
                strong: true,
                target: ("dev-libs", "victim", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165(
                    "dev-libs/victim-1.0",
                    ("dev-libs", "scanner"),
                )),
            }),
            filed_165(Filed165Args {
                owner: ("dev-libs", "scanner"),
                owner_version: "1.0",
                atom: "!dev-libs/victim",
                strong: false,
                target: ("dev-libs", "victim", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165(
                    "dev-libs/victim-1.0",
                    ("dev-libs", "scanner2"),
                )),
            }),
            filed_165(Filed165Args {
                owner: ("dev-libs", "scanner"),
                owner_version: "1.0",
                atom: "!!dev-libs/other",
                strong: true,
                target: ("dev-libs", "other", "1.0"),
                unsolvable: false,
                satisfied_by: Some(uninstall_165("dev-libs/other-1.0", ("dev-libs", "scanner"))),
            }),
        ],
        &mut orphans,
    );
    assert!(orphans.is_empty());
    assert_eq!(entries.len(), 2);
    let victim = entries
        .iter()
        .find(|e| e.package == "victim")
        .expect("victim removal exists");
    assert_eq!(victim.blockers.len(), 2);
    assert_eq!(victim.required_by.len(), 2);
    let other = entries
        .iter()
        .find(|e| e.package == "other")
        .expect("other removal exists");
    assert_eq!(other.blockers.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// An unsolvable row with no owner entry and no removal is not
/// dropped: it rides the orphan list Portage still reports.
#[test]
fn file_blocker_conflicts_orphans_an_unsolvable_ownerless_row() {
    let root = dir_165("file-orphan");
    let mut entries: Vec<GraphEntry> = Vec::new();
    let mut orphans = Vec::new();
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![filed_165(Filed165Args {
            owner: ("dev-libs", "scanner"),
            owner_version: "1.0",
            atom: "!!dev-libs/gone",
            strong: true,
            target: ("dev-libs", "gone", "9.9"),
            unsolvable: true,
            satisfied_by: None,
        })],
        &mut orphans,
    );
    assert!(entries.is_empty());
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].owner_package, "scanner");
    assert_eq!(orphans[0].conflict.atom_str, "!!dev-libs/gone");
    let _ = std::fs::remove_dir_all(&root);
}

/// An already-walked cp is never scanned, and with nothing
/// merge-bound the scan is a no-op.
#[test]
fn collect_unwalked_installed_blockers_skips_walked_cps_and_nomerge_runs() {
    let root = dir_165("scan-skip");
    install_165(
        &root,
        "dev-libs",
        "blk-1.0",
        "0",
        &[("RDEPEND", "!!dev-libs/victim")],
    );
    // `blk` itself walked: its blocker is the walk's own business.
    let entries = vec![
        new_165("dev-libs", "victim", "2.0"),
        new_165("dev-libs", "blk", "1.0"),
    ];
    let mut pending = Vec::new();
    let mut memo = HashMap::new();
    collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
        repos: &[],
        root: &root,
        config: &cfg_165(),
        dynamic_deps: false,
        ignore_built_slot_operator_deps: false,
        disj_constraints: &HashMap::new(),
        root_deps_running_root: None,
        entries: &entries,
        pending_blockers: &mut pending,
        installed_meta_memo: &mut memo,
    });
    assert!(pending.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// The duplicate guard is exact: a pre-seeded identical row is not
/// filed twice, while a same-owner different-atom row still files.
#[test]
fn collect_unwalked_installed_blockers_dedups_exact_rows_only() {
    let root = dir_165("scan-dedup");
    install_165(
        &root,
        "dev-libs",
        "blk-1.0",
        "0",
        &[("RDEPEND", "!!dev-libs/victim !!dev-libs/other")],
    );
    let entries = vec![new_165("dev-libs", "victim", "2.0")];
    let seed = PendingBlocker {
        atom_str: "!!dev-libs/victim".to_string(),
        strong: true,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "victim".to_string(),
        owner_key: ("dev-libs".to_string(), "blk".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: false,
        owner_installed: true,
    };
    let mut pending = vec![seed];
    let mut memo = HashMap::new();
    collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
        repos: &[],
        root: &root,
        config: &cfg_165(),
        dynamic_deps: false,
        ignore_built_slot_operator_deps: false,
        disj_constraints: &HashMap::new(),
        root_deps_running_root: None,
        entries: &entries,
        pending_blockers: &mut pending,
        installed_meta_memo: &mut memo,
    });
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].atom_str, "!!dev-libs/other");
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

/// A plain RDEPEND on an installed package is queued (not skipped).
#[test]
fn enqueue_dependencies_queues_a_plain_dep_on_an_installed_package() {
    let root = dir_165("enqueue-basic");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("RDEPEND", "dev-libs/dep")],
    );
    install_165(&root, "dev-libs", "dep-1.0", "0", &[]);
    let repos = repo_165(
        &root,
        &[
            ("dev-libs/consumer", "1.0", "0", "", "", "", ""),
            ("dev-libs/dep", "1.0", "0", "", "", "", ""),
        ],
    );
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    let pullers = enqueue_165(Enqueue165Args {
        repos: &repos,
        root: &root,
        config: &cfg_165(),
        cat: "dev-libs",
        pkg: "consumer",
        ver: "1.0",
        with_bdeps: false,
        entries: &mut entries,
        queue: &mut queue,
        pending: &mut pending,
    });
    assert!(pending.is_empty());
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].atom, "dev-libs/dep");
    assert_eq!(queue[0].depth, 1);
    assert_eq!(
        pullers.get(&("dev-libs".to_string(), "dep".to_string())),
        Some(&vec![(
            "dev-libs".to_string(),
            "consumer".to_string(),
            "1.0".to_string(),
            "dev-libs/dep".to_string()
        )])
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A dependency on the installed instance itself is the satisfied
/// self-edge Portage drops.
#[test]
fn enqueue_dependencies_drops_a_dep_satisfied_by_the_instance_itself() {
    let root = dir_165("enqueue-self");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    let repos = repo_165(&root, &[("dev-libs/consumer", "1.0", "0", "", "", "", "")]);
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    enqueue_165(Enqueue165Args {
        repos: &repos,
        root: &root,
        config: &cfg_165(),
        cat: "dev-libs",
        pkg: "consumer",
        ver: "1.0",
        with_bdeps: false,
        entries: &mut entries,
        queue: &mut queue,
        pending: &mut pending,
    });
    assert!(queue.is_empty());
    assert!(pending.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
