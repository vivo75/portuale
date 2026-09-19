use super::*;

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
