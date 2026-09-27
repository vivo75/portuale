use super::*;

/// An atom in both a build-time key and the walked `IDEPEND` is a
/// runtime edge, not `buildtime_hard`.
#[test]
fn onlydeps_ideps_atom_shared_with_buildtime_key_is_not_buildtime_hard() {
    // EAPI 8 with ideps kept classifies; blanked ideps (or EAPI 7) classifies nothing.
    assert_eq!(
        onlydeps_runtime_keys(OnlydepsRuntimeKeysArgs {
            onlydeps_blanks_runtime: true,
            onlydeps_blanks_idepend: false,
            eapi_has_idepend: true
        }),
        &["IDEPEND"][..]
    );
    assert!(
        onlydeps_runtime_keys(OnlydepsRuntimeKeysArgs {
            onlydeps_blanks_runtime: true,
            onlydeps_blanks_idepend: true,
            eapi_has_idepend: true
        })
        .is_empty()
    );
    assert!(
        onlydeps_runtime_keys(OnlydepsRuntimeKeysArgs {
            onlydeps_blanks_runtime: true,
            onlydeps_blanks_idepend: false,
            eapi_has_idepend: false
        })
        .is_empty()
    );
    let flatten = |text: &str| -> HashSet<String> {
        let toks: Vec<String> = text.split_whitespace().map(String::from).collect();
        let use_flags: HashSet<String> = HashSet::new();
        portage_use_reduce::use_reduce_flat(
            &toks,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map(|v| v.into_iter().filter(|t| t != "||").collect())
        .unwrap_or_default()
    };
    let buildtime_atoms = flatten("dev-libs/odw0e");
    let runtime_atoms = flatten("dev-libs/odw0e");
    assert!(buildtime_atoms.contains("dev-libs/odw0e"));
    assert!(runtime_atoms.contains("dev-libs/odw0e"));
    let mut queue = VecDeque::new();
    let mut blockers = Vec::new();
    enqueue_flat_deps(EnqueueFlatDepsArgs {
        flat_deps: vec!["dev-libs/odw0e".to_string()],
        key: &("dev-libs".to_string(), "odw0a".to_string()),
        version: "1",
        depth: 1,
        parent_use: &HashSet::new(),
        queue: &mut queue,
        pending_blockers: &mut blockers,
        buildtime_atoms: &buildtime_atoms,
        runtime_atoms: &runtime_atoms,
        bdepend_atoms: &HashSet::new(),
        or_universe: &HashSet::new(),
        running_atoms: &HashSet::new(),
        owner_targets_running_root: false,
    });
    assert_eq!(queue.len(), 1);
    assert!(
        !queue[0].buildtime_hard,
        "an IDEPEND-walked atom is a runtime edge even when rdeps are blanked"
    );
}

/// Collapses co-selected atoms onto one pick like Portage's
/// `_minimize_children`: installed-first for dynamic deps,
/// ascending for version ranges.
#[test]
fn minimize_children_collapses_like_portage() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-236-minimize").keep();
    let d = base.join("var/db/pkg/dev-libs/minlib-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("IUSE"), "abi\n").unwrap();
    fs::write(d.join("USE"), "abi\n").unwrap();
    let config = test_config();
    let repo = base.join("repo");
    for (pv, slot) in [("1.0", "0/1"), ("2.0", "0/2"), ("3.0", "0/3")] {
        let dir = repo.join("dev-libs/minlib");
        fs::create_dir_all(&dir).unwrap();
        let body = format!("EAPI=8\nDESCRIPTION=\"236\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        fs::write(dir.join(format!("minlib-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=236\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        fs::create_dir_all(&cachedir).unwrap();
        fs::write(cachedir.join(format!("minlib-{pv}")), entry).unwrap();
    }
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
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
    }];
    let key = ("dev-libs".to_string(), "minlib".to_string());
    let installed = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let up = |to: &str| PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: to.into(),
    };
    // (1) dynamic deps of an installed parent.
    let out = minimize_children(
        &base,
        &repos,
        &config,
        &key,
        vec![
            ("dev-libs/minlib:=".to_string(), up("2.0")),
            (">=dev-libs/minlib-1.0:0/1=".to_string(), installed.clone()),
        ],
    );
    assert_eq!(
        out,
        vec![
            ("dev-libs/minlib:=".to_string(), installed.clone()),
            (">=dev-libs/minlib-1.0:0/1=".to_string(), installed.clone()),
        ]
    );
    // (2) an ebuild parent's version range: ascending, the lower pick
    // is the only match of `<3` and stays; `3.0` is eliminated.
    let out = minimize_children(
        &base,
        &repos,
        &config,
        &key,
        vec![
            ("<dev-libs/minlib-3".to_string(), up("2.0")),
            (">=dev-libs/minlib-1.5".to_string(), up("3.0")),
        ],
    );
    assert_eq!(
        out,
        vec![
            ("<dev-libs/minlib-3".to_string(), up("2.0")),
            (">=dev-libs/minlib-1.5".to_string(), up("2.0")),
        ]
    );
    // (3) both atoms carry use-deps the installed USE satisfies, so
    // the tree pick is eliminated.
    // Portage: depgraph.py, findAtomForPackage
    let out = minimize_children(
        &base,
        &repos,
        &config,
        &key,
        vec![
            ("dev-libs/minlib:=[abi]".to_string(), up("2.0")),
            ("dev-libs/minlib:0/1=[abi]".to_string(), installed.clone()),
        ],
    );
    assert_eq!(
        out,
        vec![
            ("dev-libs/minlib:=[abi]".to_string(), installed.clone()),
            ("dev-libs/minlib:0/1=[abi]".to_string(), installed.clone()),
        ]
    );
    // Disjoint selections pass through, as do use-deps neither the
    // installed USE nor any ebuild candidate satisfies.
    let disjoint = vec![
        ("<dev-libs/minlib-2".to_string(), installed.clone()),
        (">=dev-libs/minlib-3".to_string(), up("3.0")),
    ];
    assert_eq!(
        minimize_children(&base, &repos, &config, &key, disjoint.clone()),
        disjoint
    );
    let used = vec![
        ("dev-libs/minlib[foo]".to_string(), up("2.0")),
        (">=dev-libs/minlib-1.0:0/1=".to_string(), installed.clone()),
    ];
    assert_eq!(
        minimize_children(&base, &repos, &config, &key, used.clone()),
        used
    );
    let _ = fs::remove_dir_all(&base);
}

/// A use-dep atom is matched against a not-yet-built package's own
/// candidate effective USE: collapses when the lower pick builds the
/// flag, stays split otherwise.
/// Portage: depgraph.py, findAtomForPackage
#[test]
fn minimize_children_matches_use_deps_against_candidate_use() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-266-minimize").keep();
    let repo = base.join("repo");
    for (pv, slot, iuse) in [
        ("1.0", "1", "-bar"),
        ("2.0", "2", "-bar"),
        ("3.0", "3", "+bar"),
    ] {
        let dir = repo.join("dev-libs/minlib2");
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "EAPI=8\nDESCRIPTION=\"266\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\nIUSE=\"{iuse}\"\n"
        );
        fs::write(dir.join(format!("minlib2-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=266\nEAPI=8\n".to_string();
        writeln!(
            entry,
            "IUSE={iuse}\nKEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}"
        )
        .unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        fs::create_dir_all(&cachedir).unwrap();
        fs::write(cachedir.join(format!("minlib2-{pv}")), entry).unwrap();
    }
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
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
    }];
    let key = ("dev-libs".to_string(), "minlib2".to_string());
    let up = |to: &str| PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: to.into(),
    };
    let picks = vec![
        (">=dev-libs/minlib2-1[bar]".to_string(), up("3.0")),
        ("<dev-libs/minlib2-3".to_string(), up("2.0")),
    ];
    // Default flags: 2.0 builds `-bar`, so the use-dep atom matches
    // only 3.0 and the pair stays split.
    let config = test_config();
    assert_eq!(
        minimize_children(&base, &repos, &config, &key, picks.clone()),
        picks
    );
    // With `bar` enabled the lower pick satisfies both atoms, so the
    // pair collapses onto 2.0.
    let mut bar_config = test_config();
    bar_config.use_tokens.push("bar".to_string());
    assert_eq!(
        minimize_children(&base, &repos, &bar_config, &key, picks.clone()),
        vec![
            (">=dev-libs/minlib2-1[bar]".to_string(), up("2.0")),
            ("<dev-libs/minlib2-3".to_string(), up("2.0")),
        ]
    );
    let _ = fs::remove_dir_all(&base);
}
