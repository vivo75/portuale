use super::*;

/// Backlog #194 (review round 1): under `--onlydeps
/// --onlydeps-with-rdeps=n --onlydeps-with-ideps=y` on an EAPI-8
/// root, real keeps walking `IDEPEND` (`depgraph.py:4186-4193`;
/// oracle cell `[B, E, F]`), so an atom present in both a
/// build-time key and `IDEPEND` is a runtime edge -- not
/// `buildtime_hard` (real `DepPriority`: an unbreakable
/// merge-order edge). Pinned at the classification level: the key
/// selection still carries `IDEPEND`, and one shared atom enqueued
/// with those sets lands soft.
#[test]
fn onlydeps_ideps_atom_shared_with_buildtime_key_is_not_buildtime_hard() {
    // rdeps blanked, ideps kept, EAPI 8: IDEPEND still classifies;
    // with ideps blanked (or below EAPI 8) nothing does.
    assert_eq!(onlydeps_runtime_keys(true, false, true), &["IDEPEND"][..]);
    assert!(onlydeps_runtime_keys(true, true, true).is_empty());
    assert!(onlydeps_runtime_keys(true, false, false).is_empty());
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
    // One atom in both a build-time key and the walked IDEPEND.
    let buildtime_atoms = flatten("dev-libs/odw0e");
    let runtime_atoms = flatten("dev-libs/odw0e");
    assert!(buildtime_atoms.contains("dev-libs/odw0e"));
    assert!(runtime_atoms.contains("dev-libs/odw0e"));
    let mut queue = VecDeque::new();
    let mut blockers = Vec::new();
    enqueue_flat_deps(
        vec!["dev-libs/odw0e".to_string()],
        &("dev-libs".to_string(), "odw0a".to_string()),
        "1",
        1,
        &HashSet::new(),
        &mut queue,
        &mut blockers,
        &buildtime_atoms,
        &runtime_atoms,
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
        false,
    );
    assert_eq!(queue.len(), 1);
    assert!(
        !queue[0].buildtime_hard,
        "an IDEPEND-walked atom is a runtime edge even when rdeps are blanked"
    );
}

/// Backlog #194 (review round 1): the read-time EAPI gate drops
/// `IDEPEND` from an EAPI-7 cache entry that still carries it.
/// Real `egencache` could never emit such an entry (real
/// `bin/ebuild.sh`'s `depend` phase unsets `IDEPEND` below EAPI 8),
/// so no fixture pins this path -- the adversarial entry is fed to
/// the gate directly.
#[test]
fn eapi7_cache_entry_carrying_idepend_is_gated_at_read_time() {
    // EAPI 7 + IDEPEND present: dropped even with no onlydeps
    // blanking.
    assert!(!onlydeps_walk_dep_key("IDEPEND", false, false, false));
    // EAPI 8 keeps it under the default flags ...
    assert!(onlydeps_walk_dep_key("IDEPEND", true, false, false));
    // ... but rdeps=n + ideps=n drops it there too.
    assert!(!onlydeps_walk_dep_key("IDEPEND", true, true, true));
    // Build-time keys are never gated; RDEPEND/PDEPEND fall to the
    // rdeps blank only.
    assert!(onlydeps_walk_dep_key("DEPEND", false, true, true));
    assert!(onlydeps_walk_dep_key("BDEPEND", false, true, true));
    assert!(!onlydeps_walk_dep_key("RDEPEND", false, true, true));
    assert!(onlydeps_walk_dep_key("RDEPEND", true, false, false));
}

/// #236 B: real `_minimize_children` (`_emerge/depgraph.py:4751-4854`)
/// on its two observed shapes. (1) An installed parent's dynamic deps
/// (the r25 shape, real's own probe in
/// `LLM/evidence/2026-09-29-236/playground/r25-debug.log`): the live
/// `minlib:=` picks the tree's `2.0` (`0/2`), the appended built
/// `>=minlib-1.0:0/1=` the installed `1.0`; installed-first elimination
/// drops `2.0` and both atoms bind the installed instance. (2) An
/// ebuild parent's version range (the fixture's `libgit2-glib`,
/// `libgit2-glib.log` there): `<minlib-3` picks `2.0`, `>=minlib-1.5`
/// the tree's `3.0`; ascending elimination keeps `2.0` (the only
/// match of `<3`) and drops `3.0`. A single selection, or atoms whose
/// selections match nothing else, pass through unchanged.
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
    // (3) #107's mesa shape: both atoms carry use-deps. The installed
    // instance's recorded `USE` satisfies `[abi]`, so real's
    // `findAtomForPackage(pkg, modified_use=_pkg_use_enabled(pkg))`
    // matches it and the tree pick is eliminated.
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
    // Disjoint selections (each atom matches only its own pick) are
    // left alone; so is a use-dep atom the installed instance's
    // recorded USE does not satisfy (`foo` is not in its IUSE), and
    // one no ebuild candidate satisfies either (`foo` is in no
    // candidate's IUSE here).
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

/// #266: real `findAtomForPackage(pkg,
/// modified_use=self._pkg_use_enabled(pkg))` evaluates a use-dep atom
/// against a not-yet-built package's own candidate effective USE, so
/// `>=minlib2-1[bar]` alongside `<minlib2-3` collapses onto the lower
/// pick when that pick builds `bar`, and stays split when it does
/// not. Grounded on real 3.0.82.2's ResolverPlayground
/// (`LLM/evidence/2026-10-01-266/r266_pg.py`).
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
