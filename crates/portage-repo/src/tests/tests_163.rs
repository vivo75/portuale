/// Backlog #163: `lib.rs` result/display-assembly unit tests.
///
/// Every test observes the assembled result structures (fields of the
/// returned result, rows, outcomes) directly -- never a contract pin's
/// full text output. Scratch-repo harness in the #161/#162 style
/// (`repo_pkgs_163` / `write_pkg_163` mirror `repo_pkgs_162` /
/// `write_pkg_162`); self-contained so the other Track U branches'
/// blocks rebase mechanically.
use crate::*;
use portage_util::TempDir;

/// Minimal merge-bound `GraphEntry` for the assembly legs (slot and
/// `new_slot` as the leg needs; everything display-side left at its
/// empty default). Mirrors `tests_162::entry_162`, which must stay
/// untouched.
fn entry_163(
    cat: &str,
    pkg: &str,
    outcome: PretendOutcome,
    required_by: &[(&str, &str)],
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
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
        required_by: required_by
            .iter()
            .map(|(c, p)| (c.to_string(), p.to_string()))
            .collect(),
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

fn new_163(version: &str) -> PretendOutcome {
    PretendOutcome::New {
        version: version.to_string(),
    }
}

fn installed_163(version: &str) -> PretendOutcome {
    PretendOutcome::AlreadyInstalled {
        version: version.to_string(),
    }
}

fn masked_163(cat: &str, pkg: &str, atom: &str) -> MaskedDepReport {
    MaskedDepReport {
        category: cat.to_string(),
        package: pkg.to_string(),
        atom: atom.to_string(),
        masked: Vec::new(),
        chain: Vec::new(),
    }
}

// ---- S1: `abort_outcome` (Slice 3: real's abandon sites as data --
// the renderer switches to `partial` in Slice 4). ----

/// A masked-only dep with a merge-bound requirer aborts as
/// `MaskedDep` with the report's atom, the first merge-bound
/// requirer's cpv, and an empty partial (real shows no list at
/// all). Pins the shape the S0 run already covers (its rows are
/// all caught there).
#[test]
fn abort_outcome_masked_dep_aborts_with_the_merge_bound_requirer() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "top")],
        ),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    let masked = vec![masked_163("dev-libs", "need", "dev-libs/need")];
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &masked,
        circular_deps: &[],
        cycle_display: &[],
        cycle_display_idx: &[],
        nvc_dep_atoms: &HashMap::new(),
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted {
            reason: AbortReason::MaskedDep { atom, parent_cpv },
            partial,
        } => {
            assert_eq!(atom, "dev-libs/need");
            assert_eq!(parent_cpv, "dev-libs/top-1.0");
            assert!(partial.is_empty());
        }
        other => panic!("expected MaskedDep, got {other:?}"),
    }
}

/// A merge-bound requirer whose cp is absent still completes, even
/// with a same-category merge-bound decoy present: the `&&`
/// conjunction (and its second `==`) must not match the decoy.
/// Pins the shape the S0 run already covers.
#[test]
fn abort_outcome_ghost_requirer_completes_despite_a_same_category_merge() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "ghost")],
        ),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    assert_eq!(
        abort_outcome(AbortOutcomeArgs {
            entries: &entries,
            masked_deps: &[],
            circular_deps: &[],
            cycle_display: &[],
            cycle_display_idx: &[],
            nvc_dep_atoms: &HashMap::new(),
            virtual_cycle: &[]
        }),
        ResolveOutcome::Complete
    );
}

/// An installed-only requirer aborts as `UnsatisfiedAtom` with the
/// recorded atom and the installed cpv. Kills the
/// `AlreadyInstalled` arm deletion (which drops the fallback and
/// completes) and both `==` flips of the installed requirer
/// lookup (which miss the requirer and complete).
#[test]
fn abort_outcome_installed_only_requirer_aborts_with_the_installed_cpv() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "oldie")],
        ),
        entry_163("dev-libs", "oldie", installed_163("1.0"), &[]),
    ];
    let mut atoms = HashMap::new();
    atoms.insert(
        ("dev-libs".to_string(), "need".to_string()),
        ">=dev-libs/need-2.0".to_string(),
    );
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &[],
        circular_deps: &[],
        cycle_display: &[],
        cycle_display_idx: &[],
        nvc_dep_atoms: &atoms,
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom { atom, parent_cpv },
            partial,
        } => {
            assert_eq!(atom, ">=dev-libs/need-2.0");
            assert_eq!(parent_cpv, "dev-libs/oldie-1.0");
            assert!(partial.is_empty());
        }
        other => panic!("expected UnsatisfiedAtom, got {other:?}"),
    }
}

/// A ghost requirer still completes with a same-category installed
/// decoy present: the installed lookup's `&&` must not match the
/// decoy. Kills the `&&` -> `||` flip and the second `==` -> `!=`
/// flip of the installed requirer lookup (both match the decoy
/// and abort).
#[test]
fn abort_outcome_ghost_requirer_completes_despite_a_same_category_installed_entry() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "ghost")],
        ),
        entry_163("dev-libs", "top", installed_163("1.0"), &[]),
    ];
    assert_eq!(
        abort_outcome(AbortOutcomeArgs {
            entries: &entries,
            masked_deps: &[],
            circular_deps: &[],
            cycle_display: &[],
            cycle_display_idx: &[],
            nvc_dep_atoms: &HashMap::new(),
            virtual_cycle: &[]
        }),
        ResolveOutcome::Complete
    );
}

/// A masked report for another package in the same category does
/// not abort as masked: the report lookup's `&&` must not match a
/// same-category report. Kills the `&&` -> `||` flip and the second
/// `==` -> `!=` flip of the masked-report lookup (both match the
/// neighbour report and abort as `MaskedDep`).
#[test]
fn abort_outcome_masked_report_for_another_package_does_not_abort_as_masked() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "top")],
        ),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    let masked = vec![masked_163("dev-libs", "other", "dev-libs/other")];
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &masked,
        circular_deps: &[],
        cycle_display: &[],
        cycle_display_idx: &[],
        nvc_dep_atoms: &HashMap::new(),
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom { atom, parent_cpv },
            partial,
        } => {
            assert_eq!(atom, "dev-libs/need");
            assert_eq!(parent_cpv, "dev-libs/top-1.0");
            assert!(partial.is_empty());
        }
        other => panic!("expected UnsatisfiedAtom, got {other:?}"),
    }
}

/// A masked report for the same package in another category does
/// not abort as masked either. Pins the shape the S0 run already
/// covers.
#[test]
fn abort_outcome_masked_report_for_the_same_package_in_another_category_does_not_abort_as_masked() {
    let entries = vec![
        entry_163(
            "dev-libs",
            "need",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "top")],
        ),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    let masked = vec![masked_163("sys-libs", "need", "sys-libs/need")];
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &masked,
        circular_deps: &[],
        cycle_display: &[],
        cycle_display_idx: &[],
        nvc_dep_atoms: &HashMap::new(),
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom { atom, parent_cpv },
            partial,
        } => {
            assert_eq!(atom, "dev-libs/need");
            assert_eq!(parent_cpv, "dev-libs/top-1.0");
            assert!(partial.is_empty());
        }
        other => panic!("expected UnsatisfiedAtom, got {other:?}"),
    }
}

/// An NVC entry with no requirer at all keeps the old rescue: the
/// loop moves on and the run completes.
#[test]
fn abort_outcome_nvc_entry_with_no_requirer_is_rescued() {
    let entries = vec![entry_163(
        "dev-libs",
        "lone",
        PretendOutcome::NoVisibleCandidate,
        &[],
    )];
    assert_eq!(
        abort_outcome(AbortOutcomeArgs {
            entries: &entries,
            masked_deps: &[],
            circular_deps: &[],
            cycle_display: &[],
            cycle_display_idx: &[],
            nvc_dep_atoms: &HashMap::new(),
            virtual_cycle: &[]
        }),
        ResolveOutcome::Complete
    );
}

/// No NVC entries and no cycles completes: the cycle gate's `!`
/// must stay. Pins the shape the S0 run already covers.
#[test]
fn abort_outcome_empty_graph_completes() {
    let entries = vec![entry_163("dev-libs", "top", new_163("1.0"), &[])];
    assert_eq!(
        abort_outcome(AbortOutcomeArgs {
            entries: &entries,
            masked_deps: &[],
            circular_deps: &[],
            cycle_display: &[],
            cycle_display_idx: &[],
            nvc_dep_atoms: &HashMap::new(),
            virtual_cycle: &[]
        }),
        ResolveOutcome::Complete
    );
}

/// A reported cycle aborts as `UnserializableCycle` with the
/// display members, and `partial` holds exactly the matching
/// entries in display order -- a merge-bound non-member stays
/// out. Pins the shape the S0 run already covers.
#[test]
fn abort_outcome_cycle_members_collect_the_matching_partial_in_order() {
    let entries = vec![
        entry_163("dev-libs", "a", new_163("1.0"), &[]),
        entry_163("dev-libs", "b", new_163("2.0"), &[]),
        entry_163("dev-libs", "c", new_163("3.0"), &[]),
    ];
    let display = vec!["dev-libs/b-2.0".to_string(), "dev-libs/a-1.0".to_string()];
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &[],
        circular_deps: &[vec!["x".to_string()]],
        cycle_display: &display,
        cycle_display_idx: &[],
        nvc_dep_atoms: &HashMap::new(),
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted {
            reason: AbortReason::UnserializableCycle { members },
            partial,
        } => {
            assert_eq!(members, display);
            assert_eq!(partial.len(), 2);
            assert_eq!(partial[0].package, "b");
            assert_eq!(partial[1].package, "a");
        }
        other => panic!("expected UnserializableCycle, got {other:?}"),
    }
}
#[test]
fn abort_outcome_cycle_idx_keeps_both_root_instances_of_one_cpv() {
    // Backlog #245/#287: a cp merged in both roots shares one cpv
    // string; the drain's indices keep the two rows apart and in order.
    let target = entry_163("dev-lang", "comp", new_163("1.0"), &[]);
    let mut running = entry_163("dev-lang", "comp", new_163("1.0"), &[]);
    running.targets_running_root = true;
    let entries = vec![target, running];
    let display = vec!["dev-lang/comp-1.0".to_string(); 2];
    match abort_outcome(AbortOutcomeArgs {
        entries: &entries,
        masked_deps: &[],
        circular_deps: &[vec!["x".to_string()]],
        cycle_display: &display,
        cycle_display_idx: &[0, 1],
        nvc_dep_atoms: &HashMap::new(),
        virtual_cycle: &[],
    }) {
        ResolveOutcome::Aborted { partial, .. } => {
            let flags: Vec<bool> = partial.iter().map(|e| e.targets_running_root).collect();
            assert_eq!(flags, vec![false, true]);
        }
        other => panic!("expected Aborted, got {other:?}"),
    }
}

/// Fresh unique scratch dir per test (vdb + repo live under it, so
/// the per-root memo caches never see a reused path). Mirrors
/// `tests_162::dir_162`, which must stay untouched.
fn dir_163(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-163-{tag}")).keep()
}

/// Fixed "amd64"-only, no-overrides config, mirroring `tests_162`'
/// `cfg_162` (a `KEYWORDS="amd64"` scratch ebuild is always
/// visible).
fn cfg_163() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

/// Arguments of [`write_pkg_163_ru`].
struct WritePkg163RuArgs<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    iuse: &'a str,
    required_use: &'a str,
}

/// Same shape with a `REQUIRED_USE` line instead of `RDEPEND`
/// (the S2 parent-flip legs' writer).
fn write_pkg_163_ru(args: WritePkg163RuArgs<'_>) {
    let WritePkg163RuArgs {
        repo,
        cp,
        pv,
        slot,
        iuse,
        required_use,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"163 result assembly\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    if !required_use.is_empty() {
        writeln!(body, "REQUIRED_USE=\"{required_use}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=163 result assembly\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    if !required_use.is_empty() {
        writeln!(entry, "REQUIRED_USE={required_use}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Same shape with `(cp, version, slot, iuse, required_use)`
/// ebuilds (the S2 parent-flip legs' repo).
fn repo_pkgs_163_ru(dir: &Path, pkgs: &[(&str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse, required_use) in pkgs {
        write_pkg_163_ru(WritePkg163RuArgs {
            repo: &repo,
            cp,
            pv,
            slot,
            iuse,
            required_use,
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

fn owner_163(cat: &str, pkg: &str) -> (String, String) {
    (cat.to_string(), pkg.to_string())
}

// ---- S2: `use_unsat_parent_row` (real `_show_unsatisfied_dep`'s
// parent-flag flip row, `depgraph.py:6756-6846`: the requesting
// parent flips its own USE to satisfy a conditional use-dep). ----

/// An `=`-conditional use-dep on a flag the parent has off (the
/// child has it on) suggests `+flag` with the parent's cpv. Real
/// `violated_conditionals` sees the parent-lacks-child-has mismatch
/// as violated-conditional (`dep/__init__.py:1541-1549`), so the row
/// fires; a `?` with the parent off would be satisfied instead (no
/// row). Kills the whole-body `None` row and every whole-body `Some`
/// row with a wrong value, plus the `!flag_is_settable` deletion
/// (which returns `None` here).
#[test]
fn use_unsat_parent_row_suggests_enabling_an_off_flag() {
    let dir = dir_163("parent-on");
    let repos = repo_pkgs_163_ru(
        &dir,
        &[
            ("dev-libs/parent", "1.0", "0", "flipme", ""),
            ("dev-libs/child", "1.0", "0", "+flipme", ""),
        ],
    );
    let config = cfg_163();
    let entries = vec![entry_163("dev-libs", "parent", new_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[flipme=]",
            owner: Some(&owner_163("dev-libs", "parent")),
            config: &config
        }),
        Some((
            "dev-libs/parent-1.0::testrepo".to_string(),
            vec!["Change USE: +flipme".to_string()],
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A conditional use-dep on a default-on flag the parent has on
/// suggests `-flag`. Kills the `!parent_use.contains` deletion in
/// the other direction (which suggests `+flag` here).
#[test]
fn use_unsat_parent_row_suggests_disabling_an_on_flag() {
    let dir = dir_163("parent-off");
    let repos = repo_pkgs_163_ru(
        &dir,
        &[
            ("dev-libs/parent", "1.0", "0", "+keepoff", ""),
            ("dev-libs/child", "1.0", "0", "keepoff", ""),
        ],
    );
    let config = cfg_163();
    let entries = vec![entry_163("dev-libs", "parent", new_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[keepoff?]",
            owner: Some(&owner_163("dev-libs", "parent")),
            config: &config
        }),
        Some((
            "dev-libs/parent-1.0::testrepo".to_string(),
            vec!["Change USE: -keepoff".to_string()],
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An *installed* requesting parent flips on its own vdb-recorded USE
/// (real `_pkg_use_enabled(myparent)` for a built package): the r135
/// S0 shape -- parent installed -flip, display atom `[flip=]`
/// (`EqualParent`), vdb `USE` empty and vdb `IUSE` declaring flip, so
/// the row proposes `+flip` on the parent's own cpv. Kills the
/// `AlreadyInstalled`-blindness (no row for installed parents).
#[test]
fn use_unsat_parent_row_suggests_enabling_an_installed_parents_off_flag() {
    let dir = dir_163("parent-installed");
    let repos = repo_pkgs_163_ru(
        &dir,
        &[
            ("dev-libs/parent", "1.0", "0", "flip", ""),
            ("dev-libs/child", "1.0", "0", "+flip", ""),
        ],
    );
    let config = cfg_163();
    let vdb = dir.join("var/db/pkg/dev-libs/parent-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("USE"), "\n").unwrap();
    std::fs::write(vdb.join("IUSE"), "flip\n").unwrap();
    let entries = vec![entry_163("dev-libs", "parent", installed_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[flip=]",
            owner: Some(&owner_163("dev-libs", "parent")),
            config: &config
        }),
        Some((
            "dev-libs/parent-1.0::testrepo".to_string(),
            vec!["Change USE: +flip".to_string()],
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A merge-bound chain node whose dep on the child sits behind
/// `qml? ( ... )` renders real's `[qml]` qualifier (the r135 S0
/// `(b)` shape: `extract_affecting_use` finds qml gating the edge,
/// qml enabled in the node's effective USE). Kills the
/// always-bare-row behaviour.
#[test]
fn chain_suffix_qualifies_a_flag_gated_merge_bound_edge() {
    let dir = dir_163("chain-qml");
    let repos = repo_pkgs_163(
        &dir,
        &[(
            "dev-libs/consumer",
            "1.0",
            "0",
            "+qml",
            "qml? ( dev-libs/leaf )",
            "amd64",
        )],
    );
    let config = cfg_163();
    let entries = vec![
        entry_163(
            "dev-libs",
            "leaf",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "consumer")],
        ),
        entry_163("dev-libs", "consumer", new_163("1.0"), &[]),
    ];
    let atoms: Vec<String> = Vec::new();
    assert_eq!(
        masked_dep_chain(MaskedDepChainArgs {
            repos: &repos,
            config: &config,
            entries: &entries,
            category: "dev-libs",
            package: "leaf",
            atoms: &atoms,
            root: dir.as_path(),
            atom: ""
        }),
        vec![(
            "dev-libs/consumer-1.0::testrepo[qml]".to_string(),
            "ebuild".to_string(),
        ),]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same edge ungated renders the bare row (no token names the
/// child behind a conditional, so there is nothing to qualify).
#[test]
fn chain_suffix_stays_bare_on_an_ungated_edge() {
    let dir = dir_163("chain-bare");
    let repos = repo_pkgs_163(
        &dir,
        &[(
            "dev-libs/consumer",
            "1.0",
            "0",
            "+qml",
            "dev-libs/leaf",
            "amd64",
        )],
    );
    let config = cfg_163();
    let entries = vec![
        entry_163(
            "dev-libs",
            "leaf",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "consumer")],
        ),
        entry_163("dev-libs", "consumer", new_163("1.0"), &[]),
    ];
    let atoms: Vec<String> = Vec::new();
    assert_eq!(
        masked_dep_chain(MaskedDepChainArgs {
            repos: &repos,
            config: &config,
            entries: &entries,
            category: "dev-libs",
            package: "leaf",
            atoms: &atoms,
            root: dir.as_path(),
            atom: ""
        }),
        vec![(
            "dev-libs/consumer-1.0::testrepo".to_string(),
            "ebuild".to_string(),
        ),]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An installed chain node qualifies from its vdb `RDEPEND` against
/// its recorded `USE` (the live `[installed]` `[qml]`/`[kde]` rows):
/// the gate is read from the vdb, the repo candidate at the same
/// version only grounds keywords/repo. Kills the installed-blindness
/// (bare rows for installed nodes).
#[test]
fn chain_suffix_qualifies_a_gated_installed_edge_from_the_vdb() {
    let dir = dir_163("chain-installed");
    let repos = repo_pkgs_163(
        &dir,
        &[(
            "dev-libs/parent",
            "1.0",
            "0",
            "+qml",
            "qml? ( dev-libs/leaf )",
            "amd64",
        )],
    );
    let config = cfg_163();
    install_163(
        &dir,
        "dev-libs",
        "parent-1.0",
        "0",
        &[
            ("USE", "qml\n"),
            ("IUSE", "+qml\n"),
            ("RDEPEND", "qml? ( dev-libs/leaf )\n"),
            ("repository", "testrepo\n"),
        ],
    );
    let entries = vec![
        entry_163(
            "dev-libs",
            "leaf",
            PretendOutcome::NoVisibleCandidate,
            &[("dev-libs", "parent")],
        ),
        entry_163("dev-libs", "parent", installed_163("1.0"), &[]),
    ];
    let atoms: Vec<String> = Vec::new();
    assert_eq!(
        masked_dep_chain(MaskedDepChainArgs {
            repos: &repos,
            config: &config,
            entries: &entries,
            category: "dev-libs",
            package: "leaf",
            atoms: &atoms,
            root: dir.as_path(),
            atom: ""
        }),
        vec![(
            "dev-libs/parent-1.0::testrepo[qml]".to_string(),
            "installed".to_string(),
        ),]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// No owner, and a display atom with no conditional use-deps, both
/// return `None` (the `owner?` early return and the empty-flags
/// early return).
#[test]
fn use_unsat_parent_row_returns_none_without_an_owner_or_conditional_flags() {
    let dir = dir_163("parent-none");
    let repos = repo_pkgs_163_ru(&dir, &[("dev-libs/parent", "1.0", "0", "flipme", "")]);
    let config = cfg_163();
    let entries = vec![entry_163("dev-libs", "parent", new_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[flipme?]",
            owner: None,
            config: &config
        }),
        None
    );
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[flipme]",
            owner: Some(&owner_163("dev-libs", "parent")),
            config: &config
        }),
        None
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A flip that would newly violate the parent's `REQUIRED_USE`
/// appends the violation note. Kills the `!new_sat` deletion
/// (which drops the note here) and the `!is_empty` deletion of
/// the `REQUIRED_USE` gate (which skips the check for a
/// non-empty constraint and drops the note too).
#[test]
fn use_unsat_parent_row_notes_a_newly_violated_required_use() {
    let dir = dir_163("parent-req");
    let repos = repo_pkgs_163_ru(
        &dir,
        &[
            (
                "dev-libs/parent",
                "1.0",
                "0",
                "flipme other",
                "flipme? ( other )",
            ),
            ("dev-libs/child", "1.0", "0", "+flipme", ""),
        ],
    );
    let config = cfg_163();
    let entries = vec![entry_163("dev-libs", "parent", new_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs { repos: &repos, root: dir.as_path(), entries: &entries, display_atom: "dev-libs/child[flipme=]", owner: Some(&owner_163("dev-libs", "parent")), config: &config }),
        Some((
            "dev-libs/parent-1.0::testrepo".to_string(),
            vec!["Change USE: +flipme, this change violates use flag constraints defined by dev-libs/parent-1.0: 'flipme? ( other )'".to_string()],
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A flip that keeps `REQUIRED_USE` satisfied carries no note: the
/// `&&` conjunction must not fire on `old_sat` alone. Kills the
/// `&&` -> `||` flip of the violation gate (which appends the note
/// here).
#[test]
fn use_unsat_parent_row_omits_the_note_when_required_use_stays_satisfied() {
    let dir = dir_163("parent-req-ok");
    let repos = repo_pkgs_163_ru(
        &dir,
        &[
            (
                "dev-libs/parent",
                "1.0",
                "0",
                "flipme",
                "flipme? ( flipme )",
            ),
            ("dev-libs/child", "1.0", "0", "+flipme", ""),
        ],
    );
    let config = cfg_163();
    let entries = vec![entry_163("dev-libs", "parent", new_163("1.0"), &[])];
    assert_eq!(
        use_unsat_parent_row(UseUnsatParentRowArgs {
            repos: &repos,
            root: dir.as_path(),
            entries: &entries,
            display_atom: "dev-libs/child[flipme=]",
            owner: Some(&owner_163("dev-libs", "parent")),
            config: &config
        }),
        Some((
            "dev-libs/parent-1.0::testrepo".to_string(),
            vec!["Change USE: +flipme".to_string()],
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
/// Arguments of [`write_pkg_163`].
struct WritePkg163Args<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    iuse: &'a str,
    rdepend: &'a str,
    keywords: &'a str,
}

/// One scratch ebuild plus its real md5-cache entry (the md5-cache
/// validation guard fails otherwise), in the #161/#162 shape:
/// `SLOT`, `KEYWORDS`, `IUSE`, `RDEPEND` lines in both the ebuild
/// and the cache entry.
fn write_pkg_163(args: WritePkg163Args<'_>) {
    let WritePkg163Args {
        repo,
        cp,
        pv,
        slot,
        iuse,
        rdepend,
        keywords,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"163 result assembly\"\nSLOT=\"{slot}\"\nKEYWORDS=\"{keywords}\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=163 result assembly\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(entry, "RDEPEND={rdepend}").unwrap();
    }
    writeln!(entry, "KEYWORDS={keywords}\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Single scratch repo (name `testrepo`, priority 0, main) holding
/// `(cp, version, slot, iuse, rdepend, keywords)` ebuilds.
fn repo_pkgs_163(dir: &Path, pkgs: &[(&str, &str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse, rdepend, keywords) in pkgs {
        write_pkg_163(WritePkg163Args {
            repo: &repo,
            cp,
            pv,
            slot,
            iuse,
            rdepend,
            keywords,
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

/// One installed instance under `<root>/var/db/pkg` with the given
/// `SLOT` plus caller-supplied field files (`USE`, `IUSE`,
/// `RDEPEND`, `repository`, ...). Mirrors `tests_162::install_162`,
/// which must stay untouched.
fn install_163(root: &Path, cat: &str, pf: &str, slot: &str, files: &[(&str, &str)]) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    for (name, contents) in files {
        std::fs::write(d.join(name), contents.as_bytes()).unwrap();
    }
}

// ---- S3: `refresh_entry_use_display` (the post-walk USE-line
// re-render: real `_pkg_use_enabled` consults the flip tier for the
// display regardless of the walked graph). ----

/// A `New` entry's display comes from the resolved candidate's own
/// IUSE with the profile-effective USE. Kills the whole-body `()`
/// row and the `New`-arm deletion (both leave the display empty).
#[test]
fn refresh_entry_use_display_renders_a_new_entry_from_the_candidate() {
    let dir = dir_163("refresh-new");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rdisp", "2.0", "0", "alpha beta", "", "amd64")],
    );
    let config = cfg_163();
    let mut entries = vec![entry_163("dev-libs", "rdisp", new_163("2.0"), &[])];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("alpha".to_string(), false), ("beta".to_string(), false),]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An `Upgrade` entry renders the target candidate with the
/// installed instance as the diff base; a `Downgrade` the same in
/// the other direction. Kills the combined
/// `Upgrade|Downgrade`-arm deletion (both stay empty).
#[test]
fn refresh_entry_use_display_renders_upgrade_and_downgrade_against_the_installed_base() {
    let dir = dir_163("refresh-updown");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64"),
            ("dev-libs/rdisp", "2.0", "0", "alpha beta", "", "amd64"),
        ],
    );
    let config = cfg_163();
    install_163(
        &dir,
        "dev-libs",
        "rdisp-1.0",
        "0",
        &[("IUSE", "alpha"), ("USE", "alpha")],
    );
    let mut entries = vec![entry_163(
        "dev-libs",
        "rdisp",
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        },
        &[],
    )];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("alpha".to_string(), false), ("beta".to_string(), false),]
    );
    // The installed base shows in the `-pv` line: `alpha` was on
    // (`-alpha*`), `beta` is new to IUSE (`-beta%`).
    assert_eq!(
        entries[0].use_expand_display,
        vec![("USE".to_string(), "-alpha* -beta%".to_string())]
    );
    let _ = std::fs::remove_dir_all(&dir);

    let dir = dir_163("refresh-down");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64"),
            ("dev-libs/rdisp", "2.0", "0", "alpha beta", "", "amd64"),
        ],
    );
    install_163(
        &dir,
        "dev-libs",
        "rdisp-2.0",
        "0",
        &[("IUSE", "alpha beta"), ("USE", "alpha beta")],
    );
    let mut entries = vec![entry_163(
        "dev-libs",
        "rdisp",
        PretendOutcome::Downgrade {
            from: "2.0".to_string(),
            to: "1.0".to_string(),
        },
        &[],
    )];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("alpha".to_string(), false)]
    );
    // The installed base shows in the `-pv` line: `alpha` was on
    // (`-alpha*`), and `beta` -- dropped from IUSE by the
    // downgrade target -- renders as removed (`(-beta%*)`).
    assert_eq!(
        entries[0].use_expand_display,
        vec![("USE".to_string(), "-alpha* (-beta%*)".to_string())]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `Reinstall` entry whose trigger flag the new ebuild dropped
/// from IUSE still shows the `(-flag%*)` removed row at plain `-p`
/// through the trigger set. Kills the `Reinstall`-arm deletion of
/// the version lookup and the `Reinstall{changed_flags}`-arm
/// deletion of the trigger set (both render an empty line here).
#[test]
fn refresh_entry_use_display_force_shows_a_dropped_reinstall_trigger_flag() {
    let dir = dir_163("refresh-reinst");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rdisp", "1.0", "0", "keep", "", "amd64")]);
    let config = cfg_163();
    install_163(
        &dir,
        "dev-libs",
        "rdisp-1.0",
        "0",
        &[("IUSE", "gone keep"), ("USE", "gone")],
    );
    let mut entries = vec![entry_163(
        "dev-libs",
        "rdisp",
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec!["gone".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        &[],
    )];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("keep".to_string(), false)]
    );
    assert_eq!(
        entries[0].use_expand_display_p,
        vec![("USE".to_string(), "(-gone%*)".to_string())]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Entries for another cp are skipped, even when their version
/// matches a candidate of the refreshed cp: the `||` conjunction
/// must skip on either mismatch. Kills the `||` -> `&&` flip, and
/// both `!=` -> `==` flips (which skip the refreshed entry
/// itself).
#[test]
fn refresh_entry_use_display_skips_entries_for_another_cp() {
    let dir = dir_163("refresh-skip");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64")],
    );
    let config = cfg_163();
    let mut entries = vec![
        entry_163("dev-libs", "rdisp", new_163("1.0"), &[]),
        entry_163("dev-libs", "other", new_163("1.0"), &[]),
        entry_163("sys-libs", "rdisp", new_163("1.0"), &[]),
    ];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("alpha".to_string(), false)]
    );
    assert!(entries[1].use_flags_display.is_empty());
    assert!(entries[2].use_flags_display.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The candidate is picked by exact version: with two versions in
/// the tree the display comes from the entry's own, not the first
/// non-matching one. Kills the `==` -> `!=` flip of the version
/// lookup.
#[test]
fn refresh_entry_use_display_picks_the_candidate_at_the_entry_version() {
    let dir = dir_163("refresh-ver");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rdisp", "1.0", "0", "oldflag", "", "amd64"),
            ("dev-libs/rdisp", "2.0", "0", "newflag", "", "amd64"),
        ],
    );
    let config = cfg_163();
    let mut entries = vec![entry_163("dev-libs", "rdisp", new_163("2.0"), &[])];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert_eq!(
        entries[0].use_flags_display,
        vec![("newflag".to_string(), false)]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Settled entries (`AlreadyInstalled`) keep their empty display:
/// the `_` arm skips them.
#[test]
fn refresh_entry_use_display_skips_settled_entries() {
    let dir = dir_163("refresh-settled");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64")],
    );
    let config = cfg_163();
    let mut entries = vec![entry_163("dev-libs", "rdisp", installed_163("1.0"), &[])];
    refresh_entry_use_display(
        &mut entries,
        &repos,
        &dir,
        &owner_163("dev-libs", "rdisp"),
        &config,
    );
    assert!(entries[0].use_flags_display.is_empty());
    assert!(entries[0].use_expand_display.is_empty());
    assert!(entries[0].use_expand_display_p.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
// ---- shared resolve_pretend harness (S4) ----

/// Option bundle for `resolve_163`, mirroring `resolve_pretend`'s
/// trailing flags positionally (see the `resolve` helper in the
/// main test module for the same shape). Defaults are the plain
/// top-level selective resolve.
#[derive(Default)]
struct RpOpts163 {
    newuse: bool,
    changed_use: bool,
    update: bool,
    excluded: Vec<String>,
    changed_deps: bool,
    with_bdeps: bool,
    changed_slot: bool,
    selective: bool,
    is_top_level: bool,
    usepkg: bool,
    usepkgonly: bool,
    binpkg_respect_use: bool,
    usepkg_exclude: Vec<String>,
    usepkg_include: Vec<String>,
    rebuilt_binaries: bool,
    rebuilt_binaries_timestamp: Option<u64>,
    newrepo: bool,
    empty: bool,
    getbinpkg: bool,
    autounmask_keywords: bool,
    autounmask_use: bool,
    autounmask_license: bool,
    autounmask_masks: bool,
    extra_constraints: Vec<String>,
}

impl RpOpts163 {
    fn top() -> Self {
        RpOpts163 {
            selective: true,
            is_top_level: true,
            with_bdeps: true,
            ..Default::default()
        }
    }
    fn dep() -> Self {
        RpOpts163 {
            with_bdeps: true,
            ..Default::default()
        }
    }
}

/// Direct `resolve_pretend` call over a scratch repo + scratch vdb.
/// `binpkgs` stages `config.scanned_binpkgs` records (the S6 binary
/// legs' shape); the binary pool derives from the same config, like
/// production's `ResolveCtx::new`.
fn resolve_163(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    atom: &str,
    o: &RpOpts163,
) -> PretendOutcome {
    resolve_pretend(
        &SelectSource {
            repos,
            root,
            config,
            local_binpkg: &build_local_binpkg_index(config),
        },
        atom,
        &SelectOptions {
            newuse: o.newuse,
            changed_use: o.changed_use,
            update: o.update,
            excluded: &o.excluded,
            changed_deps: o.changed_deps,
            with_bdeps: o.with_bdeps,
            changed_slot: o.changed_slot,
            selective: o.selective,
            is_top_level: o.is_top_level,
            usepkg: o.usepkg,
            usepkgonly: o.usepkgonly,
            binpkg_respect_use: o.binpkg_respect_use,
            usepkg_exclude: &o.usepkg_exclude,
            usepkg_include: &o.usepkg_include,
            rebuilt_binaries: o.rebuilt_binaries,
            rebuilt_binaries_timestamp: o.rebuilt_binaries_timestamp,
            newrepo: o.newrepo,
            empty: o.empty,
            getbinpkg: o.getbinpkg,
            autounmask_keywords: o.autounmask_keywords,
            autounmask_use: o.autounmask_use,
            autounmask_license: o.autounmask_license,
            autounmask_masks: o.autounmask_masks,
            extra_constraints: &o.extra_constraints,
        },
    )
    .unwrap_or_else(|e| panic!("resolve_163({atom}) failed: {e}"))
}

fn binpkg_163(cpv: &str, slot: &str, extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m = HashMap::from([
        ("CPV".to_string(), cpv.to_string()),
        ("SLOT".to_string(), slot.to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ]);
    for (k, v) in extra {
        m.insert(k.to_string(), v.to_string());
    }
    m
}

// ---- S4: `resolve_pretend` (the single-atom v1 pretend decision:
// best visible candidate vs installed, real
// `_wrapped_select_pkg_highest_available_imp` + the `avoid_update`
// / fallback / autounmask levels around it). ----

/// A fresh top-level atom resolves `New` at the best visible
/// version. Pins the baseline the S0 run already covers (its rows
/// are caught or unviable there).
#[test]
fn resolve_pretend_fresh_top_level_atom_resolves_new_at_the_best_version() {
    let dir = dir_163("rp-new");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    let config = cfg_163();
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::top()),
        new_163("2.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--emptytree` rebuilds everything: an installed same-version
/// top-level atom with a newer tree version resolves `Upgrade`
/// while a non-empty selective resolve keeps the installed one.
/// Pins the `selective && !empty` conjunction the S0 run already
/// covers.
#[test]
fn resolve_pretend_emptytree_upgrades_past_the_installed_version() {
    for (tag, empty, selective, expected) in [
        (
            "empty",
            true,
            true,
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
        ),
        ("selective", false, true, installed_163("1.0")),
    ] {
        let dir = dir_163(&format!("rp-empty-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
            ],
        );
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
        let config = cfg_163();
        let mut o = RpOpts163::top();
        o.empty = empty;
        o.selective = selective;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A non-empty selective top-level resolve over an installed
/// version with a newer tree version keeps the installed one.
/// Pins the shape the S0 run already covers.
#[test]
fn resolve_pretend_selective_top_level_keeps_the_installed_version() {
    let dir = dir_163("rp-sel");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::top()),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--usepkgonly` pulls a binary-only package from the staged
/// index. Kills the `usepkg || usepkgonly` -> `&&` flip (which
/// leaves the binary pool empty and reports `NoVisibleCandidate`).
#[test]
fn resolve_pretend_usepkgonly_pulls_a_binary_only_package() {
    let dir = dir_163("rp-binonly");
    let repos = repo_pkgs_163(&dir, &[]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/rpbin-1.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.usepkgonly = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rpbin", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The installed fallback under `--usepkg`/`--usepkgonly` covers a
/// dependency whose only satisfier is installed (real restricts
/// the *merge* pool, not installed-satisfaction). Pins the shape
/// the S0 run already covers.
#[test]
fn resolve_pretend_usepkg_dependency_falls_back_to_the_installed_version() {
    let dir = dir_163("rp-usefallback");
    let repos = repo_pkgs_163(&dir, &[]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dependency whose installed version still satisfies the atom
/// is kept without searching for a newer one (real's
/// `avoid_update` early return). Pins the early return all four
/// `!`s guard.
#[test]
fn resolve_pretend_dependency_avoids_updating_a_satisfying_installed_version() {
    let dir = dir_163("rp-avoid");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::dep()),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each widened early-return guard changes the outcome on its own:
/// `--update`, `--emptytree`, and a top-level atom each bypass the
/// early return and upgrade past the installed version (the dep
/// baseline keeps it). Pins the gate the S0 run already covers.
#[test]
fn resolve_pretend_early_return_bypass_flags_each_upgrade_past_installed() {
    for (tag, o) in [
        (
            "update",
            RpOpts163 {
                update: true,
                ..RpOpts163::dep()
            },
        ),
        (
            "empty",
            RpOpts163 {
                empty: true,
                ..RpOpts163::dep()
            },
        ),
        (
            "top",
            RpOpts163 {
                is_top_level: true,
                selective: false,
                ..RpOpts163::dep()
            },
        ),
    ] {
        let dir = dir_163(&format!("rp-bypass-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
            ],
        );
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
        let config = cfg_163();
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
/// A plain matching candidate is usable: no autounmask fallback
/// runs. Pins the probe the S0 run already covers.
#[test]
fn resolve_pretend_plain_matching_candidate_needs_no_fallback() {
    let dir = dir_163("rp-usable");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    let config = cfg_163();
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::top()),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A keyword-masked-only candidate resolves through the
/// `--autounmask` keyword level. Pins the level loop the S0 run
/// already covers.
#[test]
fn resolve_pretend_autounmask_keywords_relaxes_a_masked_candidate() {
    let dir = dir_163("rp-aukw");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "~amd64")]);
    let config = cfg_163();
    let mut o = RpOpts163::top();
    o.autounmask_keywords = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("1.0")
    );
    // Without the flag the same candidate is invisible.
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::top()),
        PretendOutcome::NoVisibleCandidate
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A positive extra constraint narrows the match; a negative one
/// hides its version. Pins the filters the S0 run already
/// covers.
#[test]
fn resolve_pretend_extra_constraints_narrow_and_hide_versions() {
    for (tag, extra, expected) in [
        (
            "positive",
            vec!["=dev-libs/rp-1.0".to_string()],
            new_163("1.0"),
        ),
        (
            "negative",
            vec!["!=dev-libs/rp-2.0".to_string()],
            new_163("1.0"),
        ),
    ] {
        let dir = dir_163(&format!("rp-extra-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
            ],
        );
        let config = cfg_163();
        let mut o = RpOpts163::top();
        o.extra_constraints = extra;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A vacuous negative extra constraint changes nothing. Pins
/// the probe the S0 run already covers.
#[test]
fn resolve_pretend_vacuous_negative_extra_constraint_resolves() {
    let dir = dir_163("rp-extravac");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    let config = cfg_163();
    let mut o = RpOpts163::top();
    o.extra_constraints = vec!["!=dev-libs/rp-9.9".to_string()];
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `[flag]` atom against a flippable candidate resolves through
/// `--autounmask-use`, and without the flag it is
/// `NoVisibleCandidate`. Pins the filter the S0 run already
/// covers (its reachable rows are caught there; the `true`
/// guard is unreachable -- see the report).
#[test]
fn resolve_pretend_use_dep_atom_resolves_only_through_autounmask_use() {
    for (tag, autounmask_use, expected) in [
        ("flip", true, new_163("1.0")),
        ("noflip", false, PretendOutcome::NoVisibleCandidate),
    ] {
        let dir = dir_163(&format!("rp-usedep-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "feat", "", "amd64")]);
        let config = cfg_163();
        let mut o = RpOpts163::top();
        o.autounmask_use = autounmask_use;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp[feat]", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A `[flag]` atom for a flag no candidate has is unfixable even
/// with `--autounmask-use`. Pins the shape the S0 run already
/// covers.
#[test]
fn resolve_pretend_unfixable_use_dep_atom_stays_unresolved() {
    let dir = dir_163("rp-usebogus");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "feat", "", "amd64")]);
    let config = cfg_163();
    let mut o = RpOpts163::top();
    o.autounmask_use = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp[bogus]", &o),
        PretendOutcome::NoVisibleCandidate
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--useoldpkg-atoms` restricts the match to built candidates,
/// and an atoms-matching binary is exempt from the
/// ebuild-visibility check. One test, one global set/reset pair
/// on a dedicated `dev-libs/opkg` cp: the atoms are
/// process-global, so they must match no other test's cp, and two
/// setters would race each other's reset under parallel runs.
/// Kills the `source == Binary` -> `!=` flip and the `&&` ->
/// `||` flip of the oldpkg filter (both resolve the newer
/// ebuild), and the `!uev` deletion of the exemption (which drops
/// the exempt binary).
#[test]
fn resolve_pretend_useoldpkg_prefers_and_exempts_built_candidates() {
    let dir = dir_163("rp-oldpkg");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/opkg", "2.0", "0", "", "", "amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/opkg-1.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.usepkg = true;
    // Without the atoms the newer ebuild wins.
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/opkg", &o),
        new_163("2.0")
    );
    set_useoldpkg_atoms(vec!["dev-libs/opkg".to_string()]);
    // With the atoms the older built candidate wins.
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/opkg", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Same atoms: a binary with no ebuild at its version survives
    // the visibility check through the exemption.
    let dir = dir_163("rp-oldexempt");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/opkg", "1.0", "0", "", "", "amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/opkg-2.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/opkg", &o),
        new_163("2.0")
    );
    set_useoldpkg_atoms(Vec::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An excluded installed version is left as-is before any
/// reinstall check runs. Pins the shape the S0 run already
/// covers.
#[test]
fn resolve_pretend_excluded_installed_version_stays_installed_under_newuse() {
    let dir = dir_163("rp-excl");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "a b", "", "amd64")]);
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "a"), ("USE", "a")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.newuse = true;
    o.excluded = vec!["dev-libs/rp".to_string()];
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A same-version install in another slot is a `New` into a fresh
/// slot, not an installed match: `candidate_is_installed` needs
/// both version and slot. Pins the shape the S0 run already
/// covers.
#[test]
fn resolve_pretend_same_version_in_another_slot_is_a_new_slot_install() {
    let dir = dir_163("rp-newslot");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "1", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::top();
    o.selective = false;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An installed same-version same-slot dependency atom is
/// `AlreadyInstalled`: both `==`s of `candidate_is_installed`
/// must hold. Pins the predicate the S0 run already covers.
#[test]
fn resolve_pretend_installed_same_version_is_already_installed() {
    let dir = dir_163("rp-ai");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `avoid_update` early return honours a hiding negative
/// extra constraint: the installed version it rejects falls
/// through to the ordinary search. Kills both `!` deletions of
/// the negative arm (each keeps the installed version and
/// reports `AlreadyInstalled`).
#[test]
fn resolve_pretend_early_return_honours_a_hiding_negative_constraint() {
    let dir = dir_163("rp-extrahide");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.extra_constraints = vec!["!=dev-libs/rp-1.0".to_string()];
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `avoid_update` early return honours a pinning positive
/// extra constraint even when the tree candidate is masked: the
/// installed version it keeps is reported as-is (the masked tree
/// alone would be `NoVisibleCandidate`). Kills the positive-arm
/// `!` deletion (which drops the installed version and reports
/// the miss).
#[test]
fn resolve_pretend_early_return_honours_a_pinning_positive_constraint() {
    let dir = dir_163("rp-extrapin");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "~amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.extra_constraints = vec!["=dev-libs/rp-1.0".to_string()];
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A visible ebuild matching the atom disqualifies a binary with
/// no ebuild at its version (`_equiv_ebuild_visible`). Pins the
/// drop the S0 run already covers (its rows are caught
/// there).
#[test]
fn resolve_pretend_visible_ebuild_disqualifies_a_version_skewed_binary() {
    let dir = dir_163("rp-equiv");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/rp-2.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
/// A top-level `[flag]` atom the tree cannot currently satisfy
/// upgrades past a satisfying installed version (real keeps the
/// installed one only for dependencies). `--autounmask-use` keeps
/// the flippable candidate in the pool so the gate is reached.
/// Kills the `!is_top_level` deletion (which keeps the installed
/// version; the neighbouring `&&` -> `||` is unviable).
#[test]
fn resolve_pretend_top_level_use_dep_atom_upgrades_past_a_satisfying_installed_version() {
    let dir = dir_163("rp-74top");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rp", "2.0", "0", "wantflag", "", "amd64")],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "wantflag"), ("USE", "wantflag")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::top();
    o.update = true;
    o.autounmask_use = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp[wantflag]", &o),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dependency `[flag]` atom the tree satisfies upgrades past a
/// satisfying installed version under `--update`. Kills the
/// `!atom_currently_satisfiable` deletion (which keeps the
/// installed version; the neighbouring `&&` -> `||` is
/// unviable).
#[test]
fn resolve_pretend_satisfiable_use_dep_atom_upgrades_under_update() {
    let dir = dir_163("rp-74sat");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rp", "2.0", "0", "+wantflag", "", "amd64")],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "wantflag"), ("USE", "wantflag")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp[wantflag]", &o),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dependency `[flag]` atom the tree cannot currently satisfy
/// keeps the satisfying installed version under `--update`
/// (real's `_want_installed_pkg` keep). `--autounmask-use` keeps
/// the flippable candidate in the pool so the gate is reached.
/// Kills the `!d.is_empty()` deletion (which skips the keep and
/// upgrades).
#[test]
fn resolve_pretend_unsatisfiable_use_dep_atom_keeps_the_installed_version() {
    let dir = dir_163("rp-74keep");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rp", "2.0", "0", "wantflag", "", "amd64")],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "wantflag"), ("USE", "wantflag")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    o.autounmask_use = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp[wantflag]", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `!update` shortcut's inner disjunction: a non-selective
/// top-level atom skips it, an `--emptytree` dependency skips it,
/// and `--update` skips it -- each upgrades past the installed
/// version where the plain dependency keeps it. Pins the shortcut
/// the S0 run already covers.
#[test]
fn resolve_pretend_update_shortcut_bypass_shapes_upgrade_past_installed() {
    // Baseline: plain dependency keeps the installed version.
    let dir = dir_163("rp-upd-base");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &RpOpts163::dep()),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Each bypass upgrades.
    for (tag, o) in [
        (
            "top",
            RpOpts163 {
                is_top_level: true,
                selective: false,
                ..RpOpts163::dep()
            },
        ),
        (
            "empty",
            RpOpts163 {
                empty: true,
                ..RpOpts163::dep()
            },
        ),
        (
            "update",
            RpOpts163 {
                update: true,
                ..RpOpts163::dep()
            },
        ),
    ] {
        let dir = dir_163(&format!("rp-upd-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
            ],
        );
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
        let config = cfg_163();
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `--newuse` with a gained flag reinstalls the installed version
/// with the trigger set: the gained flag (`b`) plus the dropped
/// effective flag (`a`, on in the vdb but default-off in the new
/// ebuild). Pins the trigger the S0 run already covers.
#[test]
fn resolve_pretend_newuse_with_a_gained_flag_reinstalls() {
    let dir = dir_163("rp-newuse");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "a b", "", "amd64")]);
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "a"), ("USE", "a")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.newuse = true;
    o.selective = false;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec!["a".to_string(), "b".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dependency atom whose installed version lacks the requested
/// `[flag]` reinstalls it (real never calls that "already
/// installed"). Pins the flip check the S0 run already covers.
#[test]
fn resolve_pretend_installed_version_missing_the_flag_reinstalls() {
    let dir = dir_163("rp-flip");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "a b", "", "amd64")]);
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("IUSE", "a b"), ("USE", "a")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.selective = false;
    o.autounmask_use = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp[b]", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each reinstall trigger also fires through the best-version
/// path (past `--update`, which bypasses the `avoid_update`
/// shortcut): the gate and its disjunction arm must agree.
/// Kills the `&&` -> `||` flip of the `changed_slot` gate
/// (13711) and the `||` -> `&&` flips of the `changed_deps`,
/// `changed_slot`, and `new_repo` disjunction arms
/// (13741/13742/13744).
#[test]
fn resolve_pretend_triggers_fire_through_the_best_version_path() {
    // Changed RDEPEND under --update.
    let dir = dir_163("rp-bdeps");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rp", "1.0", "0", "", "dev-libs/other", "amd64")],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-1.0",
        "0",
        &[("RDEPEND", "dev-libs/different")],
    );
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    o.selective = false;
    o.changed_deps = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Changed sub-slot under --update, option on and off.
    for (tag, changed_slot, expected) in [
        (
            "on",
            true,
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: true,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
        ),
        ("off", false, installed_163("1.0")),
    ] {
        let dir = dir_163(&format!("rp-bslot-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0/1", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0/0", &[]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.update = true;
        o.selective = false;
        o.changed_slot = changed_slot;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed repo under --update.
    let dir = dir_163("rp-brepo");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[("repository", "other")]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.update = true;
    o.selective = false;
    o.newrepo = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: true,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each reinstall trigger fires on its own through the
/// `avoid_update` shortcut as well: the option-on shapes
/// reinstall with only their flag, the option-off shapes keep
/// the installed version. Pins the shortcut shapes the S0 run
/// already covers (the best-path arms live in the next test).
#[test]
fn resolve_pretend_each_reinstall_trigger_fires_on_its_own() {
    // Changed RDEPEND, option on and off.
    for (tag, changed_deps, deps_changed) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-tdeps-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[("dev-libs/rp", "1.0", "0", "", "dev-libs/other", "amd64")],
        );
        install_163(
            &dir,
            "dev-libs",
            "rp-1.0",
            "0",
            &[("RDEPEND", "dev-libs/different")],
        );
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.changed_deps = changed_deps;
        o.selective = false;
        let expected = if deps_changed {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: true,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed sub-slot (same main slot, so the installed instance
    // still matches): the `slot_changed` probe compares the full
    // slot, and a main-slot move is a new-slot install instead.
    // Option on and off.
    for (tag, changed_slot, slot_changed) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-tslot-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0/1", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0/0", &[]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.changed_slot = changed_slot;
        o.selective = false;
        let expected = if slot_changed {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: true,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    // Changed repo, option on and off.
    for (tag, newrepo, new_repo) in [("on", true, true), ("off", false, false)] {
        let dir = dir_163(&format!("rp-trepo-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[("repository", "other")]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.newrepo = newrepo;
        o.selective = false;
        let expected = if new_repo {
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: true,
                slot_operator_rebuild: false,
            }
        } else {
            installed_163("1.0")
        };
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The `Reinstall` disjunction fires on each arm alone: `--emptytree`
/// and a non-selective top-level atom each reinstall a changeless
/// installed version where a plain dependency keeps it. Pins the
/// disjunction the S0 run already covers.
#[test]
fn resolve_pretend_reinstall_disjunction_fires_on_each_arm_alone() {
    // Baseline: plain dependency keeps the installed version.
    let dir = dir_163("rp-rcond-base");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.selective = false;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
    // Each arm reinstalls bare.
    let bare = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    for (tag, o) in [
        (
            "empty",
            RpOpts163 {
                empty: true,
                ..RpOpts163::dep()
            },
        ),
        (
            "top",
            RpOpts163 {
                is_top_level: true,
                selective: false,
                ..RpOpts163::dep()
            },
        ),
    ] {
        let dir = dir_163(&format!("rp-rcond-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
        let config = cfg_163();
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            bare.clone(),
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A selective top-level atom under `--update` keeps the
/// installed version: the top-level arm needs `!selective`. Kills
/// the `&&` -> `||` flip of the top-level arm (which reinstalls
/// bare; the `!selective` deletion is already caught).
#[test]
fn resolve_pretend_top_level_arm_needs_non_selective() {
    let dir = dir_163("rp-rcond-upd");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.is_top_level = true;
    o.selective = true;
    o.update = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `!update` shortcut is reached past a non-empty exclude
/// list: an excluded-but-unmatched dependency still keeps its
/// installed version through the shortcut. Pins the shape the S0
/// run already covers.
#[test]
fn resolve_pretend_update_shortcut_holds_past_an_unmatched_exclude() {
    let dir = dir_163("rp-upd-excl");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-1.0", "0", &[]);
    let config = cfg_163();
    let mut o = RpOpts163::dep();
    o.selective = false;
    o.excluded = vec!["dev-libs/other".to_string()];
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        installed_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Upgrade and downgrade are decided against the installed
/// version in the candidate's own slot: a newer same-slot tree
/// version upgrades, an older one downgrades, and an other-slot
/// install is ignored. Pins the slot/version comparison the S0 run
/// already covers.
#[test]
fn resolve_pretend_upgrade_and_downgrade_follow_the_own_slot_version() {
    for (tag, tree, vdb, expected) in [
        (
            "upgrade",
            vec![
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
            ],
            ("rp-1.0", "0"),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
        ),
        (
            "downgrade",
            vec![("dev-libs/rp", "1.0", "0", "", "", "amd64")],
            ("rp-2.0", "0"),
            PretendOutcome::Downgrade {
                from: "2.0".to_string(),
                to: "1.0".to_string(),
            },
        ),
        (
            "other-slot",
            vec![
                ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
                ("dev-libs/rp", "1.5", "1", "", "", "amd64"),
            ],
            ("rp-1.0", "0"),
            new_163("1.5"),
        ),
    ] {
        let dir = dir_163(&format!("rp-slotver-{tag}"));
        let repos = repo_pkgs_163(&dir, &tree);
        install_163(&dir, "dev-libs", vdb.0, vdb.1, &[]);
        let config = cfg_163();
        let mut o = RpOpts163::dep();
        o.update = true;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
/// `--binpkg-respect-use` (and `--newuse`, which opens the same
/// gate) drops a binary built against different USE. With no
/// ebuild matching the atom the visibility filter stays out, so
/// the gate alone decides: the divergent binary must not win.
/// Kills both `||` -> `&&` flips of the gate (each keeps the
/// binary and resolves it).
#[test]
fn resolve_pretend_respect_use_drops_a_use_divergent_binary() {
    let dir = dir_163("rp-respect");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "~amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163(
        "dev-libs/rp-2.0",
        "0",
        &[("IUSE", "feat"), ("USE", "feat")],
    )]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    o.newuse = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::NoVisibleCandidate
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The respect-use comparison is against the ebuild at the
/// binary's own version: a same-version ebuild whose IUSE covers
/// the baked USE keeps the binary in a skewed-slot shape. Kills
/// the `==` -> `!=` flip of the ebuild lookup (which compares
/// against no ebuild, drops the binary, and reports the
/// installed ebuild instead of the new slot), the `==` -> `!=`
/// flip of the version probe (which blinds the retain the same
/// way), and the `||` -> `&&` flip of the retain predicate
/// (which drops the binary the same way).
#[test]
fn resolve_pretend_respect_use_compares_against_the_same_version_ebuild() {
    let dir = dir_163("rp-respectver");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "+a", "", "amd64"),
        ],
    );
    install_163(
        &dir,
        "dev-libs",
        "rp-2.0",
        "0",
        &[("IUSE", "a"), ("USE", "a")],
    );
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163(
        "dev-libs/rp-2.0",
        "1",
        &[("IUSE", "a"), ("USE", "a")],
    )]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    o.newuse = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("2.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A binary with no ebuild at its version is compared without one:
/// with no visible ebuild at all the visibility filter stays out,
/// so a real USE divergence still drops it. Kills the `&&` ->
/// `||` and `==` -> `!=` flips of the ebuild lookup (each
/// compares against the masked ebuild, accepts, and resolves the
/// binary).
#[test]
fn resolve_pretend_lonely_binary_with_divergent_use_is_dropped() {
    let dir = dir_163("rp-lonely");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "+a", "", "~amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163(
        "dev-libs/rp-2.0",
        "0",
        &[("IUSE", "a"), ("USE", "a")],
    )]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    o.newuse = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::NoVisibleCandidate
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--binpkg-changed-deps` drops a binary built against different
/// dependencies: the stale binary in its own slot must not win
/// over the installed ebuild. Kills the `!binary_deps_changed`
/// deletion (which keeps the binary and resolves its slot).
#[test]
fn resolve_pretend_changed_deps_drops_a_stale_binary() {
    let dir = dir_163("rp-bindeps");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-2.0", "0", &[]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163(
        "dev-libs/rp-2.0",
        "1",
        &[("RDEPEND", "dev-libs/other")],
    )]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        PretendOutcome::Reinstall {
            version: "2.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A keyword-masked ebuild does not make `some_ebuild_matches`:
/// the visible newer binary still resolves. Kills the first
/// `&&` -> `||` flip of the match probe (which counts the masked
/// ebuild, runs the visibility filter, and drops the binary).
#[test]
fn resolve_pretend_masked_ebuild_does_not_match_for_visibility() {
    let dir = dir_163("rp-masketouch");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "~amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/rp-2.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("2.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A version-pinned atom does not match a newer ebuild for the
/// visibility probe: the matching older binary still resolves.
/// Kills the second `&&` -> `||` flip of the match probe (which
/// counts the newer ebuild, runs the filter, and drops the
/// binary).
#[test]
fn resolve_pretend_pinned_atom_does_not_match_a_newer_ebuild() {
    let dir = dir_163("rp-pintouch");
    let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "2.0", "0", "", "", "amd64")]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/rp-1.0", "0", &[])]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "=dev-libs/rp-1.0", &o),
        new_163("1.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A slot-skewed binary kept by the visibility check resolves
/// into its own slot. Kills the `||` -> `&&` flip of the retain
/// predicate (which drops the binary and reports the installed
/// ebuild instead).
#[test]
fn resolve_pretend_kept_slot_skewed_binary_resolves_into_its_slot() {
    let dir = dir_163("rp-binslot");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/rp", "1.0", "0", "", "", "amd64"),
            ("dev-libs/rp", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(&dir, "dev-libs", "rp-2.0", "0", &[]);
    let mut config = cfg_163();
    config.scanned_binpkgs = Some(vec![binpkg_163("dev-libs/rp-2.0", "1", &[])]);
    let mut o = RpOpts163::top();
    o.selective = false;
    o.usepkg = true;
    assert_eq!(
        resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
        new_163("2.0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--rebuilt-binaries` with a newer binary build reinstalls
/// the installed version carrying the rebuild flag; without the
/// flag the same shape reinstalls bare (the `--emptytree` arm).
/// The `rebuilt` arm runs top-level selective under `--update`
/// ( past the update block with the top-level arm shut) so the
/// rebuild flag alone decides. Kills the `usepkg || usepkgonly`
/// -> `&&` flip of the rebuild gate and the `||` -> `&&` flip
/// of its disjunction arm (each reports the bare reinstall),
/// and the `&&` -> `||` flip of the gate (which reports the
/// rebuild reinstall without the flag).
#[test]
fn resolve_pretend_rebuilt_binaries_reinstalls_on_newer_build() {
    let bare = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let rebuilt = PretendOutcome::Reinstall {
        version: "1.0".to_string(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: true,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    for (tag, rebuilt_binaries, expected) in [("rebuilt", true, rebuilt), ("bare", false, bare)] {
        let dir = dir_163(&format!("rp-rebuilt-{tag}"));
        let repos = repo_pkgs_163(&dir, &[("dev-libs/rp", "1.0", "0", "", "", "amd64")]);
        install_163(&dir, "dev-libs", "rp-1.0", "0", &[("BUILD_TIME", "1000")]);
        let mut config = cfg_163();
        config.scanned_binpkgs = Some(vec![binpkg_163(
            "dev-libs/rp-1.0",
            "0",
            &[("BUILD_TIME", "2000")],
        )]);
        // The `rebuilt` arm resolves top-level selective under
        // `--update` (no shortcut, no bare arms) so the rebuild
        // flag alone decides; the `bare` arm goes through
        // `--emptytree` (the bare arm fires, the flag stays
        // false).
        let mut o = RpOpts163::top();
        if tag == "bare" {
            o.selective = false;
            o.is_top_level = false;
            o.empty = true;
        }
        o.update = true;
        o.usepkg = true;
        o.rebuilt_binaries = rebuilt_binaries;
        assert_eq!(
            resolve_163(&repos, &dir, &config, "dev-libs/rp", &o),
            expected,
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
// ---- shared assemble_result harness (S5) ----

fn pass_163() -> PassResult {
    PassResult {
        entries: Vec::new(),
        slot_conflicts: Vec::new(),
        skipped_updates: Vec::new(),
        orphan_blockers: Vec::new(),
        slot_want: HashMap::new(),
        slot_pullers: HashMap::new(),
        masked_deps: Vec::new(),
        use_unsat_deps: Vec::new(),
        plain_miss_deps: Vec::new(),
        nvc_dep_atoms: HashMap::new(),
        missing_dep_trigger: None,
        missing_args: Vec::new(),
        autounmask_grew: false,
        parent_flip_rescued: false,
        edge_kind_map: HashMap::new(),
        changed_deps_report_entries: Vec::new(),
        pprovided_atoms: Vec::new(),
        autounmask_keyword_changes: Vec::new(),
        autounmask_use_changes: Vec::new(),
        autounmask_license_changes: Vec::new(),
        autounmask_mask_changes: Vec::new(),
        use_overlay: HashMap::new(),
        use_change_overlay: Vec::new(),
        use_broke: false,
        abi_rebuilds: None,
        suppressed_nvc: false,
        parent_atoms: Vec::new(),
    }
}

/// Literal `ResolveCtx` in the #161 `ctx_161` shape (same field
/// list, so it tracks production), with the knobs each leg needs
/// as parameters.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
fn ctx_163<'a>(
    root: &'a Path,
    config: &'a portage_profile::Config,
    repos: Vec<RepoConfig>,
    atoms: &'a [String],
    no_strings: &'a [String],
    buildpkgonly: bool,
    backtrack_enabled: bool,
    ignore_slot_op: bool,
    rebuild_new_slot: bool,
    reachable: HashSet<(String, String)>,
) -> ResolveCtx<'a> {
    ResolveCtx {
        root,
        atoms,
        update: false,
        config,
        newuse: false,
        changed_use: false,
        nodeps: false,
        onlydeps: false,
        onlydeps_with_rdeps: true,
        onlydeps_with_ideps: false,
        deep: Deep::NotRequested,
        excluded: no_strings,
        with_bdeps: false,
        changed_deps: false,
        changed_slot: false,
        with_test_deps: false,
        changed_deps_report: false,
        selective: true,
        autounmask_backtrack_enabled: backtrack_enabled,
        usepkg: false,
        usepkgonly: false,
        binpkg_respect_use: false,
        usepkg_exclude: no_strings,
        usepkg_include: no_strings,
        rebuilt_binaries: false,
        rebuilt_binaries_timestamp: None,
        newrepo: false,
        buildpkgonly,
        root_deps_running_root: None,
        distdir: root,
        empty: false,
        getbinpkg: false,
        ignore_built_slot_operator_deps: ignore_slot_op,
        backtrack_max: 10,
        reinstall_atoms: no_strings,
        rebuild_if_new_slot: rebuild_new_slot,
        rebuild_if_unbuilt: false,
        rebuild_if_new_rev: false,
        rebuild_if_new_ver: false,
        rebuild_exclude: no_strings,
        rebuild_ignore: no_strings,
        dynamic_deps: true,
        implicit_system_deps: false,
        complete: false,
        set_args: HashSet::new(),
        repos,
        slot_op_reachable: reachable.clone(),
        world_reachable: reachable,
        blocker_retry_closure: HashSet::new(),
        complete_locked_merges: HashSet::new(),
        top_level: HashSet::new(),
        top_level_cps: HashSet::new(),
        local_binpkg: build_local_binpkg_index(config),
    }
}

fn change_163(atom: &str, token: &str, chain: &[&str]) -> AutounmaskChange {
    AutounmaskChange {
        atom: atom.to_string(),
        token: token.to_string(),
        dep_chain: chain.iter().map(|s| s.to_string()).collect(),
        trigger: None,
    }
}

fn use_unsat_163(cat: &str, pkg: &str, atom: &str) -> UseUnsatDepReport {
    UseUnsatDepReport {
        category: cat.to_string(),
        package: pkg.to_string(),
        atom: atom.to_string(),
        rows: vec![("x".to_string(), vec!["y".to_string()])],
        chain: Vec::new(),
    }
}

fn plain_163(cat: &str, pkg: &str, atom: &str) -> PlainMissDepReport {
    PlainMissDepReport {
        category: cat.to_string(),
        package: pkg.to_string(),
        atom: atom.to_string(),
        chain: Vec::new(),
    }
}

// ---- S5: `assemble_result` (the settled-graph assembly: merge
// order, buildpkgonly gate, cycle report, autounmask coalescing,
// disclosure chains, and the abort classification). ----

/// A non-buildpkgonly resolve never reports unsatisfied
/// buildpkgonly deps, even with merge edges present. Pins the
/// gate the S0 run already covers.
#[test]
fn assemble_result_no_buildpkgonly_flag_means_no_buildpkgonly_refusal() {
    let dir = dir_163("asm-nobpo");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "a", new_163("1.0"), &[("dev-libs", "b")]),
        entry_163("dev-libs", "b", new_163("1.0"), &[]),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert!(!result.buildpkgonly_deps_unsatisfied);
    assert_eq!(result.backtrack_restarts, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A lone merge with no owners is no buildpkgonly edge, and a
/// lone uninstall with no owners is no blocker edge either.
/// The uninstall arm kills the `&&` -> `||` flip of the action
/// filter (which reports an edge here); the merge arm pins the
/// already-caught edge predicate.
#[test]
fn assemble_result_lone_entries_are_no_buildpkgonly_edge() {
    for (tag, outcome) in [
        ("merge", new_163("1.0")),
        (
            "uninstall",
            PretendOutcome::Uninstall {
                version: "1.0".to_string(),
            },
        ),
    ] {
        let dir = dir_163(&format!("asm-lone-{tag}"));
        let repos = repo_pkgs_163(&dir, &[]);
        let config = cfg_163();
        let no_strings: Vec<String> = Vec::new();
        let atoms: Vec<String> = Vec::new();
        let ctx = ctx_163(
            &dir,
            &config,
            repos,
            &atoms,
            &no_strings,
            true,
            false,
            false,
            true,
            HashSet::new(),
        );
        let mut pass = pass_163();
        pass.entries = vec![entry_163("dev-libs", "solo", outcome, &[])];
        let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
        assert!(!result.buildpkgonly_deps_unsatisfied, "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A merge required by another merge is a buildpkgonly edge.
/// Pins the disjunction the S0 run already covers.
#[test]
fn assemble_result_merge_requiring_a_merge_is_a_buildpkgonly_edge() {
    let dir = dir_163("asm-edge");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        true,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "a", new_163("1.0"), &[("dev-libs", "b")]),
        entry_163("dev-libs", "b", new_163("1.0"), &[]),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert!(result.buildpkgonly_deps_unsatisfied);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two chained uninstalls with no merge anywhere are still no
/// buildpkgonly edge: uninstalls are not merge actions. Pins the
/// filter the S0 run already covers.
#[test]
fn assemble_result_chained_uninstalls_are_no_buildpkgonly_edge() {
    let dir = dir_163("asm-uninst");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        true,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163(
            "dev-libs",
            "u1",
            PretendOutcome::Uninstall {
                version: "1.0".to_string(),
            },
            &[("dev-libs", "u2")],
        ),
        entry_163(
            "dev-libs",
            "u2",
            PretendOutcome::Uninstall {
                version: "2.0".to_string(),
            },
            &[],
        ),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert!(!result.buildpkgonly_deps_unsatisfied);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A recorded autounmask change is not duplicated by the
/// backtracking slice's records, while a record for another atom
/// is appended. Kills the first-`==` -> `!=` flip of the dedup
/// comparison (which duplicates the known atom and drops the new
/// one); pins the already-caught gate and the second flip (whose
/// duplicate the coalescing below repairs).
#[test]
fn assemble_result_recorded_autounmask_change_is_not_duplicated() {
    let dir = dir_163("asm-dedup");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.autounmask_use_changes = vec![change_163(">=dev-libs/a-1.0", "x", &[])];
    let params = BacktrackParams {
        autounmask_use_change_records: vec![
            change_163(">=dev-libs/a-1.0", "x", &[]),
            change_163(">=dev-libs/b-1.0", "x", &[]),
        ],
        ..Default::default()
    };
    let result = assemble_result(&ctx, &params, pass, &config, 0);
    assert_eq!(
        result
            .autounmask_use_changes
            .iter()
            .map(|c| (c.atom.clone(), c.token.clone()))
            .collect::<Vec<_>>(),
        vec![
            (">=dev-libs/a-1.0".to_string(), "x".to_string()),
            (">=dev-libs/b-1.0".to_string(), "x".to_string()),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A record with a new token for a known atom is appended, then
/// coalesced into the atom's line. Kills the `&&` -> `||` flip of
/// the dedup comparison (which drops the new token).
#[test]
fn assemble_result_new_token_for_a_known_atom_is_coalesced() {
    let dir = dir_163("asm-coal");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.autounmask_use_changes = vec![change_163(">=dev-libs/a-1.0", "x", &[])];
    let params = BacktrackParams {
        autounmask_use_change_records: vec![change_163(">=dev-libs/a-1.0", "y", &[])],
        ..Default::default()
    };
    let result = assemble_result(&ctx, &params, pass, &config, 0);
    assert_eq!(
        result
            .autounmask_use_changes
            .iter()
            .map(|c| (c.atom.clone(), c.token.clone()))
            .collect::<Vec<_>>(),
        vec![(">=dev-libs/a-1.0".to_string(), "x y".to_string())]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Changes for different atoms stay on separate lines, and a
/// repeated flag is not duplicated within a line. Kills the
/// `==` -> `!=` flip of the coalescing lookup (which merges
/// different atoms) and the `!any` deletion of the flag dedup
/// (which doubles the flag).
#[test]
fn assemble_result_coalescing_keeps_atoms_apart_and_flags_singular() {
    let dir = dir_163("asm-coal2");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.autounmask_use_changes = vec![
        change_163(">=dev-libs/a-1.0", "x", &[]),
        change_163(">=dev-libs/b-1.0", "y", &[]),
        change_163(">=dev-libs/a-1.0", "x", &[]),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(
        result
            .autounmask_use_changes
            .iter()
            .map(|c| (c.atom.clone(), c.token.clone()))
            .collect::<Vec<_>>(),
        vec![
            (">=dev-libs/a-1.0".to_string(), "x".to_string()),
            (">=dev-libs/b-1.0".to_string(), "y".to_string()),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An uncovered dependency `NoVisibleCandidate` gains a
/// plain-miss disclosure naming the queued atom. Kills the
/// `!matches!(NoVisibleCandidate)` deletion (which files none).
#[test]
fn assemble_result_uncovered_nvc_gains_a_plain_miss_disclosure() {
    let dir = dir_163("asm-plain");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![entry_163(
        "dev-libs",
        "pm",
        PretendOutcome::NoVisibleCandidate,
        &[],
    )];
    pass.nvc_dep_atoms.insert(
        ("dev-libs".to_string(), "pm".to_string()),
        ">=dev-libs/pm-2.0".to_string(),
    );
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(result.plain_miss_deps.len(), 1);
    assert_eq!(result.plain_miss_deps[0].atom, ">=dev-libs/pm-2.0");
    assert_eq!(result.outcome, ResolveOutcome::Complete);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `NoVisibleCandidate` claimed by each sibling disclosure gains
/// no plain-miss row: every `||` arm of the coverage check must
/// stay an `||`, and the `==`s inside each arm a `==`. Kills the
/// three `||` -> `&&` flips (each files a duplicate row) and the
/// first-`==` -> `!=` flip inside each arm predicate (each misses
/// its arm's exact claim and files a row).
#[test]
fn assemble_result_claimed_nvc_gains_no_plain_miss_row() {
    for (tag, setup, expected) in [
        ("masked", 0u8, 0usize),
        ("use", 1u8, 0usize),
        ("plain", 2u8, 1usize),
    ] {
        let dir = dir_163(&format!("asm-claimed-{tag}"));
        let repos = repo_pkgs_163(&dir, &[]);
        let config = cfg_163();
        let no_strings: Vec<String> = Vec::new();
        let atoms: Vec<String> = Vec::new();
        let ctx = ctx_163(
            &dir,
            &config,
            repos,
            &atoms,
            &no_strings,
            false,
            false,
            false,
            true,
            HashSet::new(),
        );
        let mut pass = pass_163();
        pass.entries = vec![entry_163(
            "dev-libs",
            "pm",
            PretendOutcome::NoVisibleCandidate,
            &[],
        )];
        match setup {
            0 => {
                pass.masked_deps
                    .push(masked_163("dev-libs", "pm", "dev-libs/pm"));
            }
            1 => {
                pass.use_unsat_deps
                    .push(use_unsat_163("dev-libs", "pm", "dev-libs/pm[flag]"));
            }
            _ => {
                pass.plain_miss_deps
                    .push(plain_163("dev-libs", "pm", "dev-libs/pm"));
            }
        }
        let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
        assert_eq!(result.plain_miss_deps.len(), expected, "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A neighbour's claim does not cover the entry: sibling reports
/// for another package leave the plain-miss row in place. Kills
/// the `&&` -> `||` flips and the second-`==` -> `!=` flips inside
/// each arm predicate (each treats the neighbour's claim as
/// covering).
#[test]
fn assemble_result_neighbour_claim_does_not_cover_the_entry() {
    for (tag, setup, expected) in [
        ("masked", 0u8, 1usize),
        ("use", 1u8, 1usize),
        ("plain", 2u8, 2usize),
    ] {
        let dir = dir_163(&format!("asm-neighbour-{tag}"));
        let repos = repo_pkgs_163(&dir, &[]);
        let config = cfg_163();
        let no_strings: Vec<String> = Vec::new();
        let atoms: Vec<String> = Vec::new();
        let ctx = ctx_163(
            &dir,
            &config,
            repos,
            &atoms,
            &no_strings,
            false,
            false,
            false,
            true,
            HashSet::new(),
        );
        let mut pass = pass_163();
        pass.entries = vec![entry_163(
            "dev-libs",
            "pm",
            PretendOutcome::NoVisibleCandidate,
            &[],
        )];
        match setup {
            0 => {
                pass.masked_deps
                    .push(masked_163("dev-libs", "other", "dev-libs/other"));
            }
            1 => {
                pass.use_unsat_deps.push(use_unsat_163(
                    "dev-libs",
                    "other",
                    "dev-libs/other[flag]",
                ));
            }
            _ => {
                pass.plain_miss_deps
                    .push(plain_163("dev-libs", "other", "dev-libs/other"));
            }
        }
        let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
        assert_eq!(result.plain_miss_deps.len(), expected, "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The in-progress extra list covers only its own cp: a second
/// uncovered entry in the same category is still filed. Kills the
/// `&&` -> `||` flip and the second-`==` -> `!=` flip of the
/// extra-list predicate (each treats the first row as covering
/// the second).
#[test]
fn assemble_result_extra_list_covers_only_its_own_cp() {
    let dir = dir_163("asm-extra");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "other", PretendOutcome::NoVisibleCandidate, &[]),
        entry_163("dev-libs", "pm", PretendOutcome::NoVisibleCandidate, &[]),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(result.plain_miss_deps.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The in-progress extra list does not cover across categories
/// either: same package, other category is still filed. Kills the
/// first-`==` -> `!=` flip of the extra-list predicate (which
/// treats the cross-category row as covering).
#[test]
fn assemble_result_extra_list_does_not_cover_across_categories() {
    let dir = dir_163("asm-extra-cat");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("sys-libs", "pm", PretendOutcome::NoVisibleCandidate, &[]),
        entry_163("dev-libs", "pm", PretendOutcome::NoVisibleCandidate, &[]),
    ];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(result.plain_miss_deps.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
/// With `--autounmask-backtrack` off, a recorded use-config
/// change refreshes just that package's USE line. Kills the
/// `!autounmask_backtrack_enabled` deletion and the
/// `!config.is_empty()` deletion (each skips the refresh and
/// leaves the line empty).
#[test]
fn assemble_result_use_config_change_refreshes_the_package_line() {
    let dir = dir_163("asm-refresh");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64")],
    );
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![entry_163("dev-libs", "rdisp", new_163("1.0"), &[])];
    let mut params = BacktrackParams::default();
    params.autounmask_use_config.insert(
        ("dev-libs".to_string(), "rdisp".to_string()),
        HashMap::from([("alpha".to_string(), true)]),
    );
    let result = assemble_result(&ctx, &params, pass, &config, 0);
    let entry = result
        .entries
        .iter()
        .find(|e| e.package == "rdisp")
        .expect("entry kept");
    assert_eq!(entry.use_flags_display, vec![("alpha".to_string(), true)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `--autounmask-backtrack` on, the same recorded change
/// leaves the walked line alone. Kills the `&&` -> `||` flip of
/// the refresh gate (which refreshes anyway).
#[test]
fn assemble_result_backtrack_on_leaves_the_walked_line_alone() {
    let dir = dir_163("asm-norefresh");
    let repos = repo_pkgs_163(
        &dir,
        &[("dev-libs/rdisp", "1.0", "0", "alpha", "", "amd64")],
    );
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        true,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![entry_163("dev-libs", "rdisp", new_163("1.0"), &[])];
    let mut params = BacktrackParams::default();
    params.autounmask_use_config.insert(
        ("dev-libs".to_string(), "rdisp".to_string()),
        HashMap::from([("alpha".to_string(), true)]),
    );
    let result = assemble_result(&ctx, &params, pass, &config, 0);
    let entry = result
        .entries
        .iter()
        .find(|e| e.package == "rdisp")
        .expect("entry kept");
    assert!(entry.use_flags_display.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An autounmask change with an empty chain gains its `#required
/// by` chain from the change's own atom through the entries.
/// Pins the derivation the `&&` -> `||` flip would also run on a
/// preset chain.
#[test]
fn assemble_result_change_gains_its_required_by_chain() {
    let dir = dir_163("asm-chain");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms = vec!["dev-libs/top".to_string()];
    let mut top_level = HashSet::new();
    top_level.insert("dev-libs/top");
    let mut ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    ctx.top_level = top_level;
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "dc", new_163("1.0"), &[("dev-libs", "top")]),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    pass.autounmask_keyword_changes = vec![change_163(">=dev-libs/dc-1.0", "~amd64", &[])];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec![
            "required by dev-libs/top-1.0::testrepo".to_string(),
            "required by dev-libs/top (argument)".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A preset chain is kept as-is. Pins the keep (the `&&` ->
/// `||` flip is unviable, so no mutant reaches this shape).
#[test]
fn assemble_result_preset_chain_is_kept() {
    let dir = dir_163("asm-chainkeep");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms = vec!["dev-libs/top".to_string()];
    let mut top_level = HashSet::new();
    top_level.insert("dev-libs/top");
    let mut ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    ctx.top_level = top_level;
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "dc", new_163("1.0"), &[("dev-libs", "top")]),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    pass.autounmask_keyword_changes = vec![change_163(">=dev-libs/dc-1.0", "~amd64", &["keep"])];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec!["keep".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The change owner is the entry matching the change's own
/// package: a same-category decoy listed first must not steal
/// it. Kills the `&&` -> `||` flip of the owner lookup (which
/// derives the chain from the decoy); pins the already-caught
/// `==` flips.
#[test]
fn assemble_result_change_owner_is_the_matching_entry() {
    let dir = dir_163("asm-owner");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms = vec!["dev-libs/top".to_string()];
    let mut top_level = HashSet::new();
    top_level.insert("dev-libs/top");
    let mut ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    ctx.top_level = top_level;
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "zz", new_163("1.0"), &[]),
        entry_163("dev-libs", "dc", new_163("1.0"), &[("dev-libs", "top")]),
        entry_163("dev-libs", "top", new_163("1.0"), &[]),
    ];
    pass.autounmask_keyword_changes = vec![change_163(">=dev-libs/dc-1.0", "~amd64", &[])];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec![
            "required by dev-libs/top-1.0::testrepo".to_string(),
            "required by dev-libs/top (argument)".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hard two-ring with no wired deps reports the cycle with an
/// empty display and no large-count. Pins the count the S0 run
/// already covers.
#[test]
fn assemble_result_small_cycle_reports_no_large_count() {
    let dir = dir_163("asm-cycle2");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms = vec!["dev-libs/ca".to_string()];
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let mut pass = pass_163();
    pass.entries = vec![
        entry_163("dev-libs", "ca", new_163("1.0"), &[]),
        entry_163("dev-libs", "cb", new_163("1.0"), &[]),
    ];
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());
    pass.edge_kind_map
        .insert((cp("cb"), cp("ca")), (true, false));
    pass.edge_kind_map
        .insert((cp("ca"), cp("cb")), (true, false));
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(result.circular_deps.len(), 1);
    assert!(!result.large_cycle_count);
    assert!(result.cycle_display.is_empty());
    match &result.outcome {
        ResolveOutcome::Aborted {
            reason: AbortReason::UnserializableCycle { members },
            partial,
        } => {
            assert!(members.is_empty());
            assert!(partial.is_empty());
        }
        other => panic!("expected UnserializableCycle, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hard triangle with wired deps reports three elementary
/// cycles: a large count only above three. Kills the `>` ->
/// `>=` flip (which reports a large cycle here); pins the
/// already-caught `==` flip.
#[test]
fn assemble_result_triangle_reports_three_cycles_without_large_count() {
    let dir = dir_163("asm-cycle3");
    let repos = repo_pkgs_163(&dir, &[]);
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms = vec![
        "dev-libs/ta".to_string(),
        "dev-libs/tb".to_string(),
        "dev-libs/tc".to_string(),
    ];
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::new(),
    );
    let edge = |next: &str| DepEdge {
        atom: format!("dev-libs/{next}"),
        evaluated: format!("dev-libs/{next}"),
        category: "dev-libs".to_string(),
        package: next.to_string(),
        priority: DepPriority {
            runtime: true,
            ..Default::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let mut ea = entry_163("dev-libs", "ta", new_163("1.0"), &[]);
    ea.deps = vec![edge("tb")];
    let mut eb = entry_163("dev-libs", "tb", new_163("1.0"), &[]);
    eb.deps = vec![edge("tc")];
    let mut ec = entry_163("dev-libs", "tc", new_163("1.0"), &[]);
    ec.deps = vec![edge("ta")];
    let mut pass = pass_163();
    pass.entries = vec![ea, eb, ec];
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());
    pass.edge_kind_map
        .insert((cp("tb"), cp("ta")), (true, false));
    pass.edge_kind_map
        .insert((cp("tc"), cp("tb")), (true, false));
    pass.edge_kind_map
        .insert((cp("ta"), cp("tc")), (true, false));
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(result.circular_deps.len(), 1);
    assert!(!result.large_cycle_count);
    assert_eq!(result.cycle_display.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}
/// The slot-operator vdb scan reports a consumer bound to a
/// rebuilt sub-slot: real's `_forced_rebuilds` display pairs. The
/// scan runs exactly when neither the ignore flag nor a disabled
/// rebuild gate suppresses it. Kills the match-guard `true`
/// mutant (which reports no pairs here).
#[test]
fn assemble_result_slot_operator_scan_reports_the_rebuilt_consumer() {
    let dir = dir_163("asm-slotop");
    let repos = repo_pkgs_163(
        &dir,
        &[
            ("dev-libs/prov", "1.0", "0", "", "", "amd64"),
            ("dev-libs/prov", "2.0", "0", "", "", "amd64"),
        ],
    );
    install_163(
        &dir,
        "dev-libs",
        "cons-1.0",
        "0",
        &[("RDEPEND", "dev-libs/prov:0/0=")],
    );
    let config = cfg_163();
    let no_strings: Vec<String> = Vec::new();
    let atoms: Vec<String> = Vec::new();
    let ctx = ctx_163(
        &dir,
        &config,
        repos,
        &atoms,
        &no_strings,
        false,
        false,
        false,
        true,
        HashSet::from([("dev-libs".to_string(), "cons".to_string())]),
    );
    let mut pass = pass_163();
    let mut prov = entry_163(
        "dev-libs",
        "prov",
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        },
        &[],
    );
    prov.slot = Some("0".to_string());
    prov.sub_slot = Some("1".to_string());
    pass.entries = vec![prov];
    let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
    assert_eq!(
        result.abi_rebuilds,
        vec![(
            "dev-libs/prov-2.0".to_string(),
            "dev-libs/cons-1.0".to_string()
        )]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The ignore flag and a disabled rebuild gate each suppress the
/// scan. Kills the match-guard `false` mutant, the `||` -> `&&`
/// flip, and the `!rebuild_if_new_slot` deletion (each runs the
/// scan and reports the pair here).
#[test]
fn assemble_result_slot_operator_scan_gates_suppress_the_pairs() {
    for (tag, ignore, rebuild) in [("ignore", true, true), ("disabled", false, false)] {
        let dir = dir_163(&format!("asm-slotgate-{tag}"));
        let repos = repo_pkgs_163(
            &dir,
            &[
                ("dev-libs/prov", "1.0", "0", "", "", "amd64"),
                ("dev-libs/prov", "2.0", "0", "", "", "amd64"),
            ],
        );
        install_163(
            &dir,
            "dev-libs",
            "cons-1.0",
            "0",
            &[("RDEPEND", "dev-libs/prov:0/0=")],
        );
        let config = cfg_163();
        let no_strings: Vec<String> = Vec::new();
        let atoms: Vec<String> = Vec::new();
        let ctx = ctx_163(
            &dir,
            &config,
            repos,
            &atoms,
            &no_strings,
            false,
            false,
            ignore,
            rebuild,
            HashSet::from([("dev-libs".to_string(), "cons".to_string())]),
        );
        let mut pass = pass_163();
        let mut prov = entry_163(
            "dev-libs",
            "prov",
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            &[],
        );
        prov.slot = Some("0".to_string());
        prov.sub_slot = Some("1".to_string());
        pass.entries = vec![prov];
        let result = assemble_result(&ctx, &BacktrackParams::default(), pass, &config, 0);
        assert!(result.abi_rebuilds.is_empty(), "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
