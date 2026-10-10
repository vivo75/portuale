/// Track U #165: `lib.rs` graph-input unit tests (mutation cluster 5) --
/// walk/installed/vdb, merge-order, blockers, use.
///
/// Scratch-vdb / scratch-repo legs in the Phase 8 S2 style (own
/// `dir_165` / `install_165` / `write_pkg_165` / `repo_165` / `entry_165`
/// helpers copied from `tests_161`/`tests_162` -- not shared, so the
/// other Track U branches' blocks rebase mechanically). Every test
/// observes the function's own return value (returned vectors, the vdb
/// fingerprint, filed conflicts, queued atoms) -- no end-to-end
/// contract reproduction. Real-source grounding is real portage 3.0.82.2
/// (`3rdparty/portage`): `vartree.dbapi` for the installed-set readers,
/// `lib/_emerge/depgraph.py:8891 _validate_blockers` for the blocker arms,
/// `repository/config.py` for `find_repos_impl`,
/// `lib/_emerge/actions.py`'s unmerge ordering (`UnmergeDepPriority`,
/// `ignore_priority_range` ~:1709) for `topological_removal_order`.
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

// ---- S1: `installed_reverse_dependents` (real
// `vartree.dbapi`'s reverse-dependency scan: every installed
// package whose flattened runtime deps match the consumer's own
// `cat/pkg-ver:slot/sub-slot`, excluding the consumer itself). ----

/// A plain RDEPEND consumer is reported; a package depending on
/// something else is not; the whole-body `vec![]` /
/// `vec![String::new()]` / `vec!["xyzzy"]` rows all fail here.
#[test]
fn installed_reverse_dependents_reports_a_plain_rdepend_consumer() {
    let root = dir_165("revdep-basic");
    install_165(&root, "dev-libs", "consumer-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "user-1.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    install_165(
        &root,
        "dev-libs",
        "bystander-1.0",
        "0",
        &[("RDEPEND", "dev-libs/other")],
    );
    assert_eq!(
        installed_reverse_dependents(&root, "dev-libs", "consumer", "1.0"),
        vec!["dev-libs/user-1.0".to_string()]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The `&&`-chain self-skip is exact: a same-category sibling and a
/// same-cp other version are *not* the consumer and stay reported.
/// Both `&&` -> `||` flips (which wrongly skip on a bare category
/// or category+package match) fail here.
#[test]
fn installed_reverse_dependents_keeps_siblings_and_other_versions() {
    let root = dir_165("revdep-siblings");
    install_165(&root, "dev-libs", "consumer-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "other-1.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    install_165(
        &root,
        "dev-libs",
        "consumer-2.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    assert_eq!(
        installed_reverse_dependents(&root, "dev-libs", "consumer", "1.0"),
        vec![
            "dev-libs/consumer-2.0".to_string(),
            "dev-libs/other-1.0".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A consumer depending on itself is still excluded: each `==` ->
/// `!=` flip in the self-skip lets the consumer's own record
/// through and reports it.
#[test]
fn installed_reverse_dependents_excludes_a_self_dependent_consumer() {
    let root = dir_165("revdep-self");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    assert!(installed_reverse_dependents(&root, "dev-libs", "consumer", "1.0").is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// Blocker atoms never count as reverse dependencies (the
/// blocker `!=` -> `==` flip would skip every plain dep instead),
/// while a genuinely matching consumer still files (the match
/// `delete !` flip would only file empty matches): both pins in
/// one leg.
#[test]
fn installed_reverse_dependents_ignores_blockers_but_keeps_matches() {
    let root = dir_165("revdep-blocker");
    install_165(&root, "dev-libs", "consumer-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "blocker-1.0",
        "0",
        &[("RDEPEND", "!dev-libs/consumer")],
    );
    install_165(
        &root,
        "dev-libs",
        "user-1.0",
        "0",
        &[("RDEPEND", "dev-libs/consumer")],
    );
    assert_eq!(
        installed_reverse_dependents(&root, "dev-libs", "consumer", "1.0"),
        vec!["dev-libs/user-1.0".to_string()]
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---- S3: `find_repos_impl` survivors (real
// `repository/config.py::RepoConfigLoader`): S0 already catches
// everything except the default-priority `==` and the
// no-`volatile`-key heuristic, which no existing leg observes. ----

/// The main repo defaults to priority -1000, every other repo to 0
/// (real `config.py`'s own default): the `*name == main_repo` ->
/// `!=` flip swaps them and fails here.
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

/// The no-`volatile`-key heuristic (real `config.py:423`): volatile
/// unless the tree is under `/var/db/repos` *and* owned by
/// root/portage. An absolute location under `/var/db/repos` (which
/// need not exist -- only the lexical prefix and the failed
/// ownership stat matter) is still volatile for a non-root owner,
/// pinning both the `||` -> `&&` flip and the `!owned` deletion.
/// A second repo at a root-owned absolute location pins the
/// `!starts_with` deletion; where the suite has no root-owned
/// system dir that leg skips loudly instead of passing silently.
#[test]
fn find_repos_impl_computes_volatile_from_location_and_owner() {
    let root = dir_165("repos-volatile");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    let sys = ["/usr", "/etc", "/bin", "/"]
        .iter()
        .map(PathBuf::from)
        .find(|p| path_owned_by_root_or_portage(p))
        .map(|p| p.to_string_lossy().into_owned());
    let conf = match &sys {
        Some(s) => format!(
            "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = /var/db/repos/foo\n\n[second]\nlocation = {s}\n"
        ),
        None => "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = /var/db/repos/foo\n".to_string(),
    };
    std::fs::write(root.join("etc/portage/repos.conf"), conf).unwrap();
    let repos = find_repos(&root).expect("repos.conf resolves");
    // `/var/db/repos/foo` is unowned (it does not exist): real
    // `!starts_with || !owned` is true; `&&` and the `!owned`
    // deletion both yield false.
    assert!(repos.iter().find(|r| r.name == "main").unwrap().volatile);
    match sys {
        Some(_) => {
            // A non-`/var/db/repos` tree is volatile either way;
            // with a genuinely root-owned location this also pins
            // the `!starts_with` deletion (which would yield false).
            assert!(repos.iter().find(|r| r.name == "second").unwrap().volatile);
        }
        None => {
            eprintln!(
                "SKIP tests_165 second-repo volatile leg: no root-owned /usr, /etc, /bin or / on this host"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// One scratch ebuild plus its real md5-cache entry (the md5-cache
/// validation guard fails otherwise), with optional dependency keys.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn write_pkg_165(
    repo: &Path,
    cp: &str,
    pv: &str,
    slot: &str,
    iuse: &str,
    depend: &str,
    rdepend: &str,
    bdepend: &str,
) {
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
        write_pkg_165(&repo, cp, pv, slot, iuse, depend, rdepend, bdepend);
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

// ---- S4: `parent_use_state` (the requesting parent's own resolved
// USE state behind a conditional use-dep: the owner's merge-bound
// entry version re-looked-up in the tree). ----

/// A `New` parent resolves to its own tree version with the
/// ebuild's IUSE visible: the whole-body `-> None` row and the
/// `New`-arm deletion both fail here.
#[test]
fn parent_use_state_resolves_a_new_parent_to_its_tree_version() {
    let dir = dir_165("parent-new");
    let repos = repo_165(&dir, &[("test/pkg", "1.0", "0", "foo", "", "", "")]);
    let entries = vec![new_165("test", "pkg", "1.0")];
    let (resolved, full_iuse, _use_flags, required_use) = parent_use_state(
        &repos,
        &entries,
        &("test".to_string(), "pkg".to_string()),
        &cfg_165(),
    )
    .expect("a merge-bound parent with a tree version resolves");
    assert_eq!(resolved.version, "1.0");
    assert!(full_iuse.contains("foo"));
    assert_eq!(required_use, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Upgrade`, `Downgrade` and `Reinstall` parents resolve through
/// their own `to`/`version` fields: each deleted outcome arm
/// returns `None` instead and fails here.
#[test]
fn parent_use_state_resolves_upgrade_downgrade_and_reinstall_parents() {
    let dir = dir_165("parent-outcomes");
    let repos = repo_165(
        &dir,
        &[
            ("test/pkg", "1.0", "0", "foo", "", "", ""),
            ("test/pkg", "2.0", "0", "foo", "", "", ""),
        ],
    );
    let config = cfg_165();
    let owner = ("test".to_string(), "pkg".to_string());
    for outcome in [
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        },
        PretendOutcome::Downgrade {
            from: "2.0".to_string(),
            to: "1.0".to_string(),
        },
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
    ] {
        let entries = vec![entry_165("test", "pkg", outcome.clone(), Some("0"))];
        let (resolved, _, _, _) = parent_use_state(&repos, &entries, &owner, &config)
            .expect("every merge-bound outcome resolves");
        let want = match &outcome {
            PretendOutcome::Upgrade { to, .. } => to,
            PretendOutcome::Downgrade { to, .. } => to,
            PretendOutcome::Reinstall { version, .. } => version,
            _ => unreachable!(),
        };
        assert_eq!(&resolved.version, want);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The owner lookup is an exact `(category, package)` match:
/// same-category and same-package decoys do not resolve. The `&&`
/// -> `||` widening (which accepts either decoy) and each `==` ->
/// `!=` flip (which accepts its own decoy) fail here.
#[test]
fn parent_use_state_ignores_same_category_and_same_package_decoys() {
    let dir = dir_165("parent-decoys");
    let repos = repo_165(&dir, &[("test/pkg", "1.0", "0", "foo", "", "", "")]);
    let entries = vec![
        new_165("test", "other", "1.0"),
        new_165("other", "pkg", "1.0"),
    ];
    assert!(
        parent_use_state(
            &repos,
            &entries,
            &("test".to_string(), "pkg".to_string()),
            &cfg_165(),
        )
        .is_none()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The tree re-lookup filters on the parent's own version: with two
/// tree versions the `New 1.0` parent resolves 1.0, pinning the
/// `c.version == version` -> `!=` flip (which would resolve 2.0).
#[test]
fn parent_use_state_picks_the_parents_own_version_from_the_tree() {
    let dir = dir_165("parent-version");
    let repos = repo_165(
        &dir,
        &[
            ("test/pkg", "1.0", "0", "foo", "", "", ""),
            ("test/pkg", "2.0", "0", "foo", "", "", ""),
        ],
    );
    let entries = vec![new_165("test", "pkg", "1.0")];
    let (resolved, _, _, _) = parent_use_state(
        &repos,
        &entries,
        &("test".to_string(), "pkg".to_string()),
        &cfg_165(),
    )
    .expect("the parent version resolves");
    assert_eq!(resolved.version, "1.0");
    let _ = std::fs::remove_dir_all(&dir);
}

fn ipkg_165(cat: &str, pkg: &str, ver: &str, slot: &str) -> InstalledPackage {
    InstalledPackage {
        category: cat.to_string(),
        package: pkg.to_string(),
        version: ver.to_string(),
        slot: slot.to_string(),
    }
}

// ---- S5: `topological_removal_order` (real unmerge ordering:
// dependers before dependencies, cpv-descending ties, genuine
// cycles broken by real's `ignore_priority_range` scan). ----

/// Installs `<name>-1.0` (slot 0) with the given dep keys.
fn order_vdb_165(root: &Path, pkgs: &[(&str, &[(&str, &str)])]) {
    for (name, keys) in pkgs {
        let files: Vec<(&str, &str)> = keys.to_vec();
        install_165(root, "dev-libs", &format!("{name}-1.0"), "0", &files);
    }
}

fn order_list_165(names: &[&str]) -> Vec<InstalledPackage> {
    names
        .iter()
        .map(|n| ipkg_165("dev-libs", n, "1.0", "0"))
        .collect()
}

fn order_names_165(out: &[InstalledPackage]) -> Vec<String> {
    out.iter().map(|p| p.package.clone()).collect()
}

/// A three-deep DEPEND chain unmerges depender-first: the
/// whole-body rows, the `n < 2` widenings, the no-edge
/// (`is_some_and`/`contains`) deletions, the `indeg` narrowings,
/// the ready-batch mutants, and -- crucially -- the `!ready`
/// deletion all fail here. That mutant only diverges (spins the
/// `ready.is_empty()` branch forever) on *cyclic* input; on this
/// DAG it terminates and merely pops in cycle-break order, so this
/// leg kills it quickly with no timeout involved.
#[test]
fn topological_removal_order_unmerges_a_depend_chain_dependers_first() {
    let root = dir_165("order-chain");
    order_vdb_165(
        &root,
        &[
            ("aa", &[("DEPEND", "dev-libs/bb")]),
            ("bb", &[("DEPEND", "dev-libs/cc")]),
            ("cc", &[]),
        ],
    );
    let (ordered, out) = topological_removal_order(&root, order_list_165(&["cc", "aa", "bb"]));
    assert!(ordered);
    assert_eq!(order_names_165(&out), vec!["aa", "bb", "cc"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A self-dependency is not an edge (`i == j` always skips): with
/// no edges the input order is kept and `ordered` is false. Both
/// `||` -> `&&` narrowings (which admit the self-edge, pushing the
/// package through the cycle-break path instead) fail here.
#[test]
fn topological_removal_order_ignores_a_self_dependency() {
    let root = dir_165("order-self");
    order_vdb_165(&root, &[("aa", &[("RDEPEND", "dev-libs/aa")]), ("bb", &[])]);
    let (ordered, out) = topological_removal_order(&root, order_list_165(&["aa", "bb"]));
    assert!(!ordered);
    assert_eq!(order_names_165(&out), vec!["aa", "bb"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A cross-category edge orders too: the `i == j` -> `!=` flip
/// (which drops every edge) and both category/package `!=` ->
/// `==` flips (which drop every cross-cp edge) fail here.
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

/// Priority arms: an IDEPEND edge (0) loses to an RDEPEND edge
/// (-2), an RDEPEND edge loses to a DEPEND edge (-4), and a PDEPEND
/// edge (-3) loses to a DEPEND edge. Each deleted `match` arm
/// demotes its key to -4 and pops the wrong node first here.
#[test]
fn topological_removal_order_breaks_cycles_by_dependency_key_priority() {
    // `aa` IDEPENDs `zz` (0), `zz` RDEPENDs `aa` (-2): `aa` pops at
    // the -2 level (IDEPEND-arm deletion pops `zz` at -4 instead).
    let root = dir_165("order-prio-idepend");
    order_vdb_165(
        &root,
        &[
            ("aa", &[("IDEPEND", "dev-libs/zz")]),
            ("zz", &[("RDEPEND", "dev-libs/aa")]),
        ],
    );
    let (ordered, out) = topological_removal_order(&root, order_list_165(&["zz", "aa"]));
    assert!(ordered);
    assert_eq!(order_names_165(&out), vec!["aa", "zz"]);

    // `aa` RDEPENDs `zz` (-2), `zz` DEPENDs `aa` (-4): `aa` pops at
    // -4 (RDEPEND-arm deletion ties both at -4 and pops cpv-max
    // `zz` instead).
    let root2 = dir_165("order-prio-rdepend");
    order_vdb_165(
        &root2,
        &[
            ("aa", &[("RDEPEND", "dev-libs/zz")]),
            ("zz", &[("DEPEND", "dev-libs/aa")]),
        ],
    );
    let (_, out2) = topological_removal_order(&root2, order_list_165(&["zz", "aa"]));
    assert_eq!(order_names_165(&out2), vec!["aa", "zz"]);

    // `aa` PDEPENDs `zz` (-3), `zz` DEPENDs `aa` (-4): `aa` pops at
    // -4 (PDEPEND-arm deletion ties both at -4 and pops `zz`).
    let root3 = dir_165("order-prio-pdepend");
    order_vdb_165(
        &root3,
        &[
            ("aa", &[("PDEPEND", "dev-libs/zz")]),
            ("zz", &[("DEPEND", "dev-libs/aa")]),
        ],
    );
    let (_, out3) = topological_removal_order(&root3, order_list_165(&["zz", "aa"]));
    assert_eq!(order_names_165(&out3), vec!["aa", "zz"]);

    // Mutual PDEPEND (-3, -3): both pop at -3, cpv-max first; the
    // PDEPEND `-3` -> `3` flip strands both (no level ever
    // eligible) and returns nothing.
    let root4 = dir_165("order-prio-pdepend-pair");
    order_vdb_165(
        &root4,
        &[
            ("aa", &[("PDEPEND", "dev-libs/zz")]),
            ("zz", &[("PDEPEND", "dev-libs/aa")]),
        ],
    );
    let (_, out4) = topological_removal_order(&root4, order_list_165(&["aa", "zz"]));
    assert_eq!(order_names_165(&out4), vec!["zz", "aa"]);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&root2);
    let _ = std::fs::remove_dir_all(&root3);
    let _ = std::fs::remove_dir_all(&root4);
}

/// A slot-operator RDEPEND without a sub-slot (`dev-libs/zz:0=`)
/// still matches (real `_match_slot` only checks the sub-slot when
/// the atom carries one) at the plain -2 priority: both
/// `slot_op_built` narrowings (`&&` -> `||`), which would promote
/// it to -1 and pop `aa` instead of `zz`, fail here.
#[test]
fn topological_removal_order_keeps_a_sub_slot_less_slot_operator_at_rdepend_priority() {
    let root = dir_165("order-slotop");
    order_vdb_165(
        &root,
        &[
            ("aa", &[("RDEPEND", "dev-libs/zz:0=")]),
            ("zz", &[("RDEPEND", "dev-libs/aa")]),
        ],
    );
    let (_, out) = topological_removal_order(&root, order_list_165(&["aa", "zz"]));
    assert_eq!(order_names_165(&out), vec!["zz", "aa"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// The `ignore_priority_range` scan: with edges at 0 (IDEPEND) and
/// -2 (RDEPEND) the -4 and -3 levels yield nothing and `aa` (whose
/// incoming edge is -2) pops at -2. Each `delete -` in the -4/-3/-2
/// scan levels turns its level positive, so both nodes are eligible
/// early and cpv-max `zz` pops instead. (The -1 level itself can
/// never pop a matchable edge -- every matchable priority is
/// -4..-2 or 0, and a node eligible at -1 was already eligible at
/// -2 -- so its own `delete -` is equivalent; classified in the
/// closeout, not pinned here.)
#[test]
fn topological_removal_order_scans_ignore_levels_before_popping() {
    let root = dir_165("order-ignore");
    order_vdb_165(
        &root,
        &[
            ("aa", &[("IDEPEND", "dev-libs/zz")]),
            ("zz", &[("RDEPEND", "dev-libs/aa")]),
        ],
    );
    let (_, out) = topological_removal_order(&root, order_list_165(&["zz", "aa"]));
    assert_eq!(order_names_165(&out), vec!["aa", "zz"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A three-cycle pops cpv-max first, then re-scans: the
/// cycle-break candidate mutants (stale-`done` inclusion or
/// exclusion), the `<=` -> `>` flip, and the early-`break`
/// deletion all return a short or duplicated order and fail the
/// exact assertion here.
#[test]
fn topological_removal_order_pops_a_three_cycle_cpv_max_first() {
    let root = dir_165("order-3cycle");
    order_vdb_165(
        &root,
        &[
            ("aa", &[("DEPEND", "dev-libs/bb")]),
            ("bb", &[("DEPEND", "dev-libs/cc")]),
            ("cc", &[("DEPEND", "dev-libs/aa")]),
        ],
    );
    let (ordered, out) = topological_removal_order(&root, order_list_165(&["aa", "bb", "cc"]));
    assert!(ordered);
    assert_eq!(order_names_165(&out), vec!["cc", "aa", "bb"]);
    let _ = std::fs::remove_dir_all(&root);
}

// ---- S6: `prune_cleanlist` (real `emerge --prune`'s selection:
// every non-highest version its args-set matches, minus whatever
// the dependency closure reaches). ----

/// Two versions, no dependents: the older is pruned, the result is
/// unordered, and nothing is kept or unresolved. (The whole-body
/// `Default` row does not compile -- tool-reported unviable.) The
/// `is_highest`/`is_candidate`/seed/cleanlist mutants
/// (which empty, swap, or flood the cleanlist), and the
/// `counts`/`multi_version` narrowings all fail here, as does the
/// highest-version guard's `!= Greater` -> `==` flip (which would
/// protect the lowest and prune 2.0 instead).
#[test]
fn prune_cleanlist_prunes_the_older_of_two_versions() {
    let root = dir_165("prune-basic");
    install_165(&root, "dev-libs", "aa-1.0", "0", &[]);
    install_165(&root, "dev-libs", "aa-2.0", "0", &[]);
    install_165(&root, "dev-libs", "bb-1.0", "0", &[]);
    let result = prune_cleanlist(&root, &[], &[], &[]);
    assert_eq!(
        result.cleanlist.iter().map(|p| p.cpv()).collect::<Vec<_>>(),
        vec!["dev-libs/aa-1.0"]
    );
    assert!(!result.ordered);
    assert_eq!(result.required_count, 2);
    assert!(result.kept_parents.is_empty());
    assert!(result.unresolved.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// The highest-version guard does not depend on scan order: with
/// versions whose lexicographic and version orders disagree (9.0
/// sorts after 10.0 but compares lower), the guard's `-> false`
/// flip -- which protects whatever version the directory scan
/// happened to yield last -- protects 9.0 and prunes 10.0 here,
/// under either scan discipline (sorted readdir, which this host's
/// tmpfs yields, or creation order). The real comparison is
/// order-independent, so this leg is green either way.
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

/// Explicit versioned args: `=dev-libs/aa-2.0` matches only 2.0
/// (which is highest, so nothing is pruned). The args
/// `category`/`package` flips and the match `delete !` (which
/// unmatch everything) fail the companion assertion; the outer
/// `&&` -> `||` (which also matches 1.0 by its bare cp, pruning
/// it) fails here.
#[test]
fn prune_cleanlist_honours_versioned_atoms_in_args() {
    let root = dir_165("prune-args");
    install_165(&root, "dev-libs", "aa-1.0", "0", &[]);
    install_165(&root, "dev-libs", "aa-2.0", "0", &[]);
    let matched = prune_cleanlist(&root, &["dev-libs/aa".to_string()], &[], &[]);
    assert_eq!(
        matched
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/aa-1.0"]
    );
    let pinned = prune_cleanlist(&root, &["=dev-libs/aa-2.0".to_string()], &[], &[]);
    assert!(pinned.cleanlist.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// A keeper pins the old version through the dependency walk: with
/// `keeper` RDEPENDing the unversioned cp, both versions are
/// reached and nothing is pruned. The `matches_atom` narrowings
/// (which blind the walk) and the cleanlist `||`/`&&` flips fail
/// here.
#[test]
fn prune_cleanlist_keeps_a_version_reached_through_the_walk() {
    let root = dir_165("prune-keeper");
    install_165(&root, "dev-libs", "aa-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "aa-2.0",
        "0",
        &[("RDEPEND", "dev-libs/keeper")],
    );
    install_165(
        &root,
        "dev-libs",
        "keeper-1.0",
        "0",
        &[("RDEPEND", "dev-libs/aa")],
    );
    let result = prune_cleanlist(&root, &[], &[], &[]);
    assert!(result.cleanlist.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// `kept_parents` names the keeper for every kept version it reaches:
/// `keeper` pins the whole `aa` cp, so both `aa-1.0` and `aa-2.0`
/// are kept with the same parent line, while single-version `bb`
/// (unmatched by the empty-args set) stays out even though the
/// walk records an edge into it. The `kept_parents` `delete !`
/// (which reports nothing) fails here, and the `multi_version`
/// `>=` widening (which would additionally keep `bb` with its own
/// parent line) fails on the exact row count.
#[test]
fn prune_cleanlist_reports_the_keeper_of_a_kept_version() {
    let root = dir_165("prune-kept-parents");
    install_165(&root, "dev-libs", "aa-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "aa-2.0",
        "0",
        &[("RDEPEND", "dev-libs/bb")],
    );
    install_165(&root, "dev-libs", "bb-1.0", "0", &[]);
    install_165(
        &root,
        "dev-libs",
        "keeper-1.0",
        "0",
        &[("RDEPEND", "dev-libs/aa")],
    );
    let result = prune_cleanlist(&root, &[], &[], &[]);
    assert!(result.cleanlist.is_empty());
    assert_eq!(result.kept_parents.len(), 2);
    for (i, ver) in ["dev-libs/aa-1.0", "dev-libs/aa-2.0"].iter().enumerate() {
        assert_eq!(result.kept_parents[i].0.cpv(), ver.to_string());
        assert_eq!(
            result.kept_parents[i].1,
            vec!["dev-libs/keeper-1.0 requires dev-libs/aa".to_string()]
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ---- S7: `resolve_blockers` (real `_validate_blockers`: pending
// blocker atoms matched against installed + merge-bound targets,
// then filed as satisfied-uninstall, replacement, or unsolvable
// rows). The existing legs pin the `New` merge-bound arm, the
// use-dep filter, and the buildpkgonly gate; these legs pin the
// remaining outcome arms, the installed/graph dedup, the
// same-slot and installed-parent skips, and the nomerge
// unsolvable arms. ----

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

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
fn pending_165(
    atom: &str,
    strong: bool,
    buildtime: bool,
    target: (&str, &str),
    owner: (&str, &str),
    owner_version: &str,
    owner_merging: bool,
    owner_installed: bool,
) -> PendingBlocker {
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
/// the candidate set exactly like `New` ones: each deleted outcome
/// arm drops its version from the match and files nothing here.
#[test]
fn resolve_blockers_matches_upgrade_downgrade_and_reinstall_targets() {
    let mk = |outcome| {
        resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &[pending_165(
                "!!dev-libs/target",
                true,
                true,
                ("dev-libs", "target"),
                ("dev-libs", "owner"),
                "1.0",
                true,
                false,
            )],
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
/// the installed one: the `!candidates.any` deletion (which drops
/// the graph version) and the slot `==` -> `!=` flip (which drops
/// it too) both miss the `:1` blocker here.
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
        &[pending_165(
            "!!dev-libs/target:1",
            true,
            true,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict.matched_version, "1.0");
    assert!(conflicts[0].conflict.unsolvable);
    let _ = std::fs::remove_dir_all(&root);
}

/// `installed_match` needs slot *and* version: installed
/// `target-1.0:0` beside the merge-bound twin `target-1.0:1` leaves
/// the `:1` row an unsolvable uninstall (the matched instance is
/// not the installed one). The `&&` -> `||` flip (16930:49)
/// self-matches on the version alone and misfiles it as a slot
/// `Replacement` once a second same-slot entry (`target-2.0:1`)
/// satisfies the replaced-in-slot arm -- so this leg kills it.
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
        &[pending_165(
            "!!dev-libs/target:1",
            true,
            true,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
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
/// the soft same-slot skip: the `!strong` deletion and the slot
/// `&&`/`==` narrowings (which would all skip it) fail here.
#[test]
fn resolve_blockers_keeps_a_strong_blocker_against_its_own_slot() {
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[pending_165(
            "!!dev-libs/owner",
            true,
            true,
            ("dev-libs", "owner"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
        &[new_165("dev-libs", "owner", "1.0")],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].conflict.unsolvable);
}

/// Cell f: an installed-only match for a nomerge owner is ignored
/// outright. Every `&&` -> `||` widening and `!` deletion in the
/// cell files a row instead and fails here.
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
        &[pending_165(
            "!dev-libs/target",
            false,
            false,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            false,
            false,
        )],
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// A nomerge owner in the required-set closure still blocks on a
/// merge-bound match (real's "digraph node with parents"): without
/// the closure the same row is solvable. The `owner_set_parent`
/// `||` -> `&&` narrowing (which drops the set term) fails the
/// first leg; the `merge_bound_match &&` deletion... the second
/// leg pins the solvable side.
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
    let pending = vec![pending_165(
        "!dev-libs/target",
        false,
        false,
        ("dev-libs", "target"),
        ("dev-libs", "owner"),
        "1.0",
        false,
        false,
    )];
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

/// A use-dep blocker is evaluated against the *matched version's*
/// own entry: `targetA` (New 1.0, flag off) does not satisfy
/// `[wantblock]`, `targetB` (Upgrade to 2.0, flag on) does, so
/// only the 2.0 row files. Every `&&` -> `||` widening and `==` ->
/// `!=` flip in the graphed-USE entry lookup files the wrong set
/// (nothing, or the 1.0 row too) and fails here.
#[test]
fn resolve_blockers_evaluates_use_deps_against_the_matched_versions_entry() {
    let mut target_a = new_165("dev-libs", "target", "1.0");
    target_a.use_flags_display = vec![("wantblock".to_string(), false)];
    let mut target_b = upgrade_165("dev-libs", "target", "1.0", "2.0");
    target_b.use_flags_display = vec![("wantblock".to_string(), true)];
    let entries = vec![target_a, target_b, new_165("dev-libs", "owner", "1.0")];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[pending_165(
            "!!dev-libs/target[wantblock]",
            true,
            true,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict.matched_version, "2.0");
    assert!(conflicts[0].conflict.unsolvable);
}

/// `merge_bound_match` needs the entry's slot *and* version: an
/// installed 1.0 beside a slot-moved merge-bound 9.9 leaves the
/// 1.0 row solvable (removed by uninstall). The
/// `installed_match`/`merge_bound_match` `&&` -> `||` widening
/// marks it merge-bound instead and fails here.
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
        &[pending_165(
            "!!dev-libs/target",
            true,
            true,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
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
/// entry at all, another cp's `AlreadyInstalled` row must not
/// stand in (the `&&` -> `||` widening flips the owner to
/// nomerge and solves the merge-bound row instead of blocking
/// on it).
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
        &[pending_165(
            "!!dev-libs/target",
            true,
            true,
            ("dev-libs", "target"),
            ("dev-libs", "owner"),
            "1.0",
            true,
            false,
        )],
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

// ---- S8: `file_blocker_conflicts` (hanging resolved rows on
// their owner entry, or -- with no owner entry -- on the
// `Uninstall` removal the row resolves by) and
// `collect_unwalked_installed_blockers` (real `_validate_blockers`'
// all-installed-packages scan for blockers the walk never
// reached). ----

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn filed_165(
    owner: (&str, &str),
    owner_version: &str,
    atom: &str,
    strong: bool,
    target: (&str, &str, &str),
    unsolvable: bool,
    satisfied_by: Option<BlockerSatisfiedBy>,
) -> FiledBlocker {
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
/// its `Uninstall` tag names: the whole-body row (which files
/// nothing anywhere) and the owner `==` flip (which misses the
/// entry) both fail here. (The deleted `Uninstall` match arm does
/// not compile -- tool-reported unviable.)
#[test]
fn file_blocker_conflicts_files_an_owned_row_on_its_entry_and_removal() {
    let root = dir_165("file-owned");
    install_165(&root, "dev-libs", "victim-1.0", "0", &[]);
    let mut entries = vec![new_165("dev-libs", "owner", "1.0")];
    let mut orphans = Vec::new();
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![filed_165(
            ("dev-libs", "owner"),
            "1.0",
            "!!dev-libs/victim",
            true,
            ("dev-libs", "victim", "1.0"),
            false,
            Some(uninstall_165("dev-libs/victim-1.0", ("dev-libs", "owner"))),
        )],
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
/// up, the anchor is recorded once): each removal-lookup `==` ->
/// `!=` flip spawns a duplicate removal, and the anchor `delete !`
/// duplicates the anchor -- all fail the exact assertions here.
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
            filed_165(
                ("dev-libs", "owner"),
                "1.0",
                "!!dev-libs/victim",
                true,
                ("dev-libs", "victim", "1.0"),
                false,
                Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            ),
            filed_165(
                ("dev-libs", "owner"),
                "1.0",
                "!dev-libs/victim",
                false,
                ("dev-libs", "victim", "1.0"),
                false,
                Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            ),
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

/// Conflicts against different removals stay separate: both
/// removal-lookup `&&` -> `||` widenings (which merge on a bare
/// category or category+package match) fail here.
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
            filed_165(
                ("dev-libs", "owner"),
                "1.0",
                "!!dev-libs/victim",
                true,
                ("dev-libs", "victim", "1.0"),
                false,
                Some(uninstall_165("dev-libs/victim-1.0", anchor)),
            ),
            filed_165(
                ("dev-libs", "owner"),
                "1.0",
                "!!dev-libs/other",
                true,
                ("dev-libs", "other", "1.0"),
                false,
                Some(uninstall_165("dev-libs/other-1.0", anchor)),
            ),
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
/// removal (the vdb-scan nomerge shape): two rows against one
/// removal share it, a same-version other-package row stays
/// separate. The no-owner lookup `==` -> `!=` flips (which split
/// the shared removal) and its `&&` -> `||` widenings (which fuse
/// the separate one) fail here.
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
            filed_165(
                ("dev-libs", "scanner"),
                "1.0",
                "!!dev-libs/victim",
                true,
                ("dev-libs", "victim", "1.0"),
                false,
                Some(uninstall_165(
                    "dev-libs/victim-1.0",
                    ("dev-libs", "scanner"),
                )),
            ),
            filed_165(
                ("dev-libs", "scanner"),
                "1.0",
                "!dev-libs/victim",
                false,
                ("dev-libs", "victim", "1.0"),
                false,
                Some(uninstall_165(
                    "dev-libs/victim-1.0",
                    ("dev-libs", "scanner2"),
                )),
            ),
            filed_165(
                ("dev-libs", "scanner"),
                "1.0",
                "!!dev-libs/other",
                true,
                ("dev-libs", "other", "1.0"),
                false,
                Some(uninstall_165("dev-libs/other-1.0", ("dev-libs", "scanner"))),
            ),
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
/// dropped: it rides the orphan list real still reports.
#[test]
fn file_blocker_conflicts_orphans_an_unsolvable_ownerless_row() {
    let root = dir_165("file-orphan");
    let mut entries: Vec<GraphEntry> = Vec::new();
    let mut orphans = Vec::new();
    file_blocker_conflicts(
        &mut entries,
        &root,
        vec![filed_165(
            ("dev-libs", "scanner"),
            "1.0",
            "!!dev-libs/gone",
            true,
            ("dev-libs", "gone", "9.9"),
            true,
            None,
        )],
        &mut orphans,
    );
    assert!(entries.is_empty());
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].owner_package, "scanner");
    assert_eq!(orphans[0].conflict.atom_str, "!!dev-libs/gone");
    let _ = std::fs::remove_dir_all(&root);
}

/// The scan files an unwalked installed package's strong blocker:
/// every whole-body / early-return / token-filter mutant empties
/// the pending list and fails here.
#[test]
fn collect_unwalked_installed_blockers_files_an_unwalked_strong_blocker() {
    let root = dir_165("scan-basic");
    install_165(
        &root,
        "dev-libs",
        "blk-1.0",
        "0",
        &[("RDEPEND", "!!dev-libs/victim")],
    );
    let entries = vec![new_165("dev-libs", "victim", "2.0")];
    let mut pending = Vec::new();
    let mut memo = HashMap::new();
    collect_unwalked_installed_blockers(
        &[],
        &root,
        &cfg_165(),
        false,
        false,
        &HashMap::new(),
        None,
        &entries,
        &mut pending,
        &mut memo,
    );
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].atom_str, "!!dev-libs/victim");
    assert!(pending[0].strong);
    assert!(!pending[0].buildtime);
    assert_eq!(pending[0].target_category, "dev-libs");
    assert_eq!(pending[0].target_package, "victim");
    assert_eq!(
        pending[0].owner_key,
        ("dev-libs".to_string(), "blk".to_string())
    );
    assert_eq!(pending[0].owner_version, "1.0");
    assert!(!pending[0].owner_merging);
    assert!(pending[0].owner_installed);
    let _ = std::fs::remove_dir_all(&root);
}

/// An already-walked cp is never scanned, and with nothing
/// merge-bound the scan is a no-op: both legs pin the early
/// returns.
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
    collect_unwalked_installed_blockers(
        &[],
        &root,
        &cfg_165(),
        false,
        false,
        &HashMap::new(),
        None,
        &entries,
        &mut pending,
        &mut memo,
    );
    assert!(pending.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// The duplicate guard is exact: a pre-seeded identical row is not
/// filed twice (each `==` -> `!=` flip re-files it), while a
/// same-owner different-atom row still files (each `&&` -> `||`
/// widening swallows it).
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
    collect_unwalked_installed_blockers(
        &[],
        &root,
        &cfg_165(),
        false,
        false,
        &HashMap::new(),
        None,
        &entries,
        &mut pending,
        &mut memo,
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].atom_str, "!!dev-libs/other");
    let _ = std::fs::remove_dir_all(&root);
}

// ---- S9: `enqueue_dependencies` (the `--deep`
// AlreadyInstalled recursion: the installed package's own
// flattened deps go to the queue, blockers to pending). Called
// directly with `dynamic_deps = false`, so dep strings come from
// the vdb snapshot and only the version lookup needs the tree. ----

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn enqueue_165(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    cat: &str,
    pkg: &str,
    ver: &str,
    with_bdeps: bool,
    entries: &mut Vec<GraphEntry>,
    queue: &mut std::collections::VecDeque<QueueItem>,
    pending: &mut Vec<PendingBlocker>,
) -> SlotPullers {
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

/// A plain RDEPEND on an installed package is queued (not skipped):
/// the whole-body row, the `||`-token skip, and every
/// satisfied-self widening (which would drop it) fail here.
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
    let pullers = enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        false,
        &mut entries,
        &mut queue,
        &mut pending,
    );
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
/// self-edge real drops: the `version == owner_version` -> `!=`
/// flip re-queues it and fails here.
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
    enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        false,
        &mut entries,
        &mut queue,
        &mut pending,
    );
    assert!(queue.is_empty());
    assert!(pending.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// A strong runtime blocker files into pending (never the queue)
/// with its provenance: the blocker `!=` -> `==` flip (which
/// queues it as a dep instead) and the `Strong` -> inverted flip
/// fail here.
#[test]
fn enqueue_dependencies_files_a_strong_blocker_into_pending() {
    let root = dir_165("enqueue-blocker");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("RDEPEND", "!!dev-libs/victim")],
    );
    let repos = repo_165(&root, &[("dev-libs/consumer", "1.0", "0", "", "", "", "")]);
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        false,
        &mut entries,
        &mut queue,
        &mut pending,
    );
    assert!(queue.is_empty());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].atom_str, "!!dev-libs/victim");
    assert!(pending[0].strong);
    assert!(!pending[0].buildtime);
    assert!(!pending[0].owner_merging);
    assert!(!pending[0].owner_installed);
    let _ = std::fs::remove_dir_all(&root);
}

/// A build-time blocker carries its provenance: with `with_bdeps`
/// the DEPEND `!!` row is `buildtime`, pinning the
/// buildtime-flatten `!=` -> `==` flip (which would record it as
/// runtime).
#[test]
fn enqueue_dependencies_marks_a_depend_blocker_as_buildtime() {
    let root = dir_165("enqueue-buildtime");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("DEPEND", "!!dev-libs/victim")],
    );
    let repos = repo_165(&root, &[("dev-libs/consumer", "1.0", "0", "", "", "", "")]);
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        true,
        &mut entries,
        &mut queue,
        &mut pending,
    );
    assert!(queue.is_empty());
    assert_eq!(pending.len(), 1);
    assert!(pending[0].strong);
    assert!(pending[0].buildtime);
    let _ = std::fs::remove_dir_all(&root);
}

/// A conditional use-dep is evaluated against the installed USE
/// before queueing, and the queue remembers the raw form: the
/// `evaluated != raw` -> `==` flip swaps which rows carry
/// `unevaluated` and fails here.
#[test]
fn enqueue_dependencies_records_the_unevaluated_form_of_a_conditional_dep() {
    let root = dir_165("enqueue-unevaluated");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[
            ("USE", "flag"),
            ("RDEPEND", "dev-libs/need[flag=] dev-libs/plain"),
        ],
    );
    let repos = repo_165(
        &root,
        &[
            ("dev-libs/consumer", "1.0", "0", "", "", "", ""),
            ("dev-libs/need", "1.0", "0", "flag", "", "", ""),
            ("dev-libs/plain", "1.0", "0", "", "", "", ""),
        ],
    );
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        false,
        &mut entries,
        &mut queue,
        &mut pending,
    );
    assert!(pending.is_empty());
    assert_eq!(queue.len(), 2);
    let cond = queue
        .iter()
        .find(|q| q.atom.starts_with("dev-libs/need"))
        .expect("conditional dep queued");
    assert_eq!(cond.unevaluated.as_deref(), Some("dev-libs/need[flag=]"));
    let plain = queue
        .iter()
        .find(|q| q.atom == "dev-libs/plain")
        .expect("plain dep queued");
    assert_eq!(plain.unevaluated, None);
    let _ = std::fs::remove_dir_all(&root);
}

/// Plains queue before `||` choices (the main walk's
/// plains-reversed + `||`-last split): both partition mutants flip
/// the order here.
#[test]
fn enqueue_dependencies_queues_plains_before_disjunction_choices() {
    let root = dir_165("enqueue-order");
    install_165(
        &root,
        "dev-libs",
        "consumer-1.0",
        "0",
        &[("RDEPEND", "dev-libs/plain || ( dev-libs/only )")],
    );
    let repos = repo_165(
        &root,
        &[
            ("dev-libs/consumer", "1.0", "0", "", "", "", ""),
            ("dev-libs/plain", "1.0", "0", "", "", "", ""),
            ("dev-libs/only", "1.0", "0", "", "", "", ""),
        ],
    );
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut pending = Vec::new();
    enqueue_165(
        &repos,
        &root,
        &cfg_165(),
        "dev-libs",
        "consumer",
        "1.0",
        false,
        &mut entries,
        &mut queue,
        &mut pending,
    );
    assert!(pending.is_empty());
    let atoms: Vec<String> = queue.iter().map(|q| q.atom.clone()).collect();
    assert_eq!(atoms, vec!["dev-libs/plain", "dev-libs/only"]);
    let _ = std::fs::remove_dir_all(&root);
}
