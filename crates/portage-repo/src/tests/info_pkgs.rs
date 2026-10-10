use super::*;

#[test]
fn info_pkgs_table_expands_a_virtual_and_renders_versions_with_repo() {
    let root = masters_test_root("info-pkgs");
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    // profiles/info_pkgs adds one extra atom.
    std::fs::write(repo.join("profiles/info_pkgs"), "# hdr\nsys-libs/glibc\n").unwrap();
    let mkpkg = |cat: &str, pf: &str, rdepend: &str, repo_name: &str| {
        let d = root.join("var/db/pkg").join(cat).join(pf);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        std::fs::write(d.join("USE"), "\n").unwrap();
        std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        std::fs::write(d.join("repository"), format!("{repo_name}\n")).unwrap();
    };
    mkpkg(
        "virtual",
        "os-headers-1",
        "sys-kernel/linux-headers",
        "gentoo",
    );
    mkpkg("sys-kernel", "linux-headers-7.1", "", "gentoo");
    mkpkg("dev-lang", "python-3.14.7", "", "gentoo");
    mkpkg("sys-libs", "glibc-2.43-r4", "", "gentoo");

    let rows = info_pkgs_table(&root, &repo);
    let by_cp: std::collections::HashMap<&str, &Vec<String>> =
        rows.iter().map(|r| (r.cp.as_str(), &r.versions)).collect();
    // virtual/os-headers -> sys-kernel/linux-headers, with the
    // ` (virtual/os-headers)` provide-suffix.
    assert_eq!(
        by_cp.get("sys-kernel/linux-headers").map(|v| v.as_slice()),
        Some(["7.1::gentoo (virtual/os-headers)".to_string()].as_slice())
    );
    // A plain (non-virtual) atom: no suffix.
    assert_eq!(
        by_cp.get("dev-lang/python").map(|v| v.as_slice()),
        Some(["3.14.7::gentoo".to_string()].as_slice())
    );
    // The info_pkgs-file atom is included.
    assert_eq!(
        by_cp.get("sys-libs/glibc").map(|v| v.as_slice()),
        Some(["2.43-r4::gentoo".to_string()].as_slice())
    );
}

#[test]
fn resolve_info_candidate_reports_the_best_visible_ebuild_and_its_use_display() {
    let repos = find_repos(&fixtures_root()).expect("fixture repos.conf must resolve");
    // dev-libs/pkginfopkg: IUSE="+alpha beta", DEFINED_PHASES=info.
    let info = resolve_info_candidate(&repos, "dev-libs/pkginfopkg", &test_config())
        .expect("resolves")
        .expect("has a visible candidate");
    assert_eq!(info.cpv, "dev-libs/pkginfopkg-1.0");
    assert_eq!(info.repo_name, "testrepo");
    assert!(info.defines_pkg_info);
    assert_eq!(
        info.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );

    // dev-libs/newpkg has no pkg_info phase.
    let plain = resolve_info_candidate(&repos, "dev-libs/newpkg", &test_config())
        .expect("resolves")
        .expect("has a visible candidate");
    assert!(!plain.defines_pkg_info);

    // A cat/pkg with no ebuild -> None.
    assert!(
        resolve_info_candidate(&repos, "dev-libs/nonexistent", &test_config())
            .expect("resolves")
            .is_none()
    );
}

#[test]
fn resolve_installed_info_reads_the_vdb_use_line_and_the_differing_vars() {
    let root = fixtures_root();
    // dev-libs/infoinstpkg-1.0: vdb IUSE="alpha beta" USE="alpha",
    // CFLAGS/CHOST recorded, test_config's make.conf sets neither.
    let infos = resolve_installed_info(&root, "dev-libs/infoinstpkg", &test_config());
    assert_eq!(infos.len(), 1);
    let i = &infos[0];
    assert_eq!(i.cpv, "dev-libs/infoinstpkg-1.0");
    assert_eq!(i.repo_name, "testrepo");
    assert_eq!(
        i.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );
    assert_eq!(
        i.differing_vars,
        vec![
            ("CHOST".to_string(), "x86_64-pc-linux-gnu".to_string()),
            ("CFLAGS".to_string(), "-O2 -march=native".to_string()),
        ]
    );
    assert_eq!(
        i.unset_vars,
        vec![
            "CXXFLAGS".to_string(),
            "FEATURES".to_string(),
            "LDFLAGS".to_string()
        ]
    );

    // Not installed -> empty.
    assert!(resolve_installed_info(&root, "dev-libs/newpkg", &test_config()).is_empty());
}

#[test]
fn parse_env_assignments_matches_portage_aux_env_search_forms() {
    // Portage: dbapi/vartree.py, _aux_env_search.
    let wanted = &[
        "PLAIN", "DECL", "EXP", "MULTI", "EMPTY", "ESC", "DUP", "TAB",
    ];
    let text = concat!(
        "PLAIN=x86_64-pc-linux-gnu\n",
        "declare -x DECL=\"-O2 -march=native\"\n",
        "export EXP='single'\n",
        "MULTI=\"line1\n",
        "line2\n",
        "line3\"\n",
        "EMPTY=\"\"\n",
        "ESC=\"v\\\"\n",
        "DUP=1\n",
        "DUP=2\n",
        "export\tTAB=tabbed\n",
        "NOEQUALS\n",
        "=nokey\n",
        "UNWANTED=zzz\n",
    );
    let got = parse_env_assignments(text, wanted);
    assert_eq!(
        got.get("PLAIN").map(String::as_str),
        Some("x86_64-pc-linux-gnu")
    );
    assert_eq!(
        got.get("DECL").map(String::as_str),
        Some("-O2 -march=native")
    );
    assert_eq!(got.get("EXP").map(String::as_str), Some("single"));
    assert_eq!(
        got.get("MULTI").map(String::as_str),
        Some("line1line2\nline3")
    );
    assert_eq!(got.get("EMPTY").map(String::as_str), Some(""));
    // The escaped `\"` does NOT close: following lines are swallowed through EOF
    // (Portage's own EOF-without-closer path).
    assert_eq!(
        got.get("ESC").map(String::as_str),
        Some("v\\\"DUP=1\nDUP=2\nexport\tTAB=tabbed\nNOEQUALS\n=nokey\nUNWANTED=zz")
    );
    assert!(!got.contains_key("DUP"), "swallowed by ESC's continuation");
    assert!(!got.contains_key("TAB"), "swallowed by ESC's continuation");
    assert!(!got.contains_key("UNWANTED"));
}

#[test]
fn parse_env_assignments_skips_continuations_of_unwanted_keys() {
    // Portage builds the value before its key check, so a skipped multi-line
    // value must not leak its body lines as fresh assignments.
    let text = "SKIP=\"a\nB=1\n\"\nB=real\n";
    let got = parse_env_assignments(text, &["B"]);
    assert_eq!(got.get("B").map(String::as_str), Some("real"));
}

#[test]
fn resolve_installed_info_unsets_everything_without_environment_bz2() {
    // With no `environment.bz2` Portage returns {}: all five vars are `Unset:`,
    // even when a same-named individual vdb file exists.
    let root = tmp_vdb(
        "dev-libs",
        "tmpenvpkg-1.0",
        &[("CFLAGS", b"-O2 -stray\n"), ("USE", b""), ("IUSE", b"")],
    );
    let infos = resolve_installed_info(&root, "dev-libs/tmpenvpkg", &test_config());
    assert_eq!(infos.len(), 1);
    assert!(infos[0].differing_vars.is_empty());
    assert_eq!(
        infos[0].unset_vars,
        vec![
            "CHOST".to_string(),
            "CFLAGS".to_string(),
            "CXXFLAGS".to_string(),
            "FEATURES".to_string(),
            "LDFLAGS".to_string()
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_installed_info_reads_mydesiredvars_only_from_the_saved_env() {
    // Only `mydesiredvars` come from the saved env; stray individual files are
    // ignored (Portage's `_aux_env_search`).
    let env = bz2(b"declare -x CHOST=\"x86_64-pc-linux-gnu\"\n");
    let root = tmp_vdb(
        "dev-libs",
        "tmpenvpkg-1.0",
        &[
            ("CFLAGS", b"-O2 -stray\n"),
            ("USE", b""),
            ("IUSE", b""),
            ("environment.bz2", &env),
        ],
    );
    let infos = resolve_installed_info(&root, "dev-libs/tmpenvpkg", &test_config());
    assert_eq!(infos.len(), 1);
    assert_eq!(
        infos[0].differing_vars,
        vec![("CHOST".to_string(), "x86_64-pc-linux-gnu".to_string())]
    );
    assert_eq!(
        infos[0].unset_vars,
        vec![
            "CFLAGS".to_string(),
            "CXXFLAGS".to_string(),
            "FEATURES".to_string(),
            "LDFLAGS".to_string()
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_installed_info_wraps_a_globally_forced_flag() {
    // Portage's `pkg_use_display` wraps a forced flag regardless of its state.
    let root = fixtures_root();
    let config = portage_profile::Config {
        use_force: HashSet::from(["globalforceflag".to_string()]),
        use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
            use_force: vec!["globalforceflag".to_string()],
            ..Default::default()
        }],
        ..test_config()
    };
    let infos = resolve_installed_info(&root, "dev-libs/infoforcedpkg", &config);
    assert_eq!(infos.len(), 1);
    assert_eq!(
        infos[0].use_expand_display,
        vec![(
            "USE".to_string(),
            "(globalforceflag) -otherflag".to_string()
        )]
    );
}

#[test]
fn resolve_installed_info_flags_a_vdb_recorded_pkg_info_phase() {
    let root = fixtures_root();
    // No DEFINED_PHASES file at all is falsy, so `pkg_info()` IS attempted.
    let plain = resolve_installed_info(&root, "dev-libs/infoinstpkg", &test_config());
    assert_eq!(plain.len(), 1);
    assert!(plain[0].defines_pkg_info);
    assert_eq!(plain[0].category, "dev-libs");
    assert_eq!(plain[0].package, "infoinstpkg");
    assert_eq!(plain[0].version, "1.0");

    // `DEFINED_PHASES="-"` is truthy but names no `info` phase -> NOT attempted.
    let dash = resolve_installed_info(&root, "dev-libs/infodashphases", &test_config());
    assert_eq!(dash.len(), 1);
    assert!(!dash[0].defines_pkg_info);
}

#[test]
fn resolve_info_binary_candidate_picks_a_pkg_info_defining_local_binpkg() {
    // A binary-only pkg with DEFINED_PHASES=info is a `--info` candidate.
    let config = portage_profile::Config {
        pkgdir: fixtures_root()
            .join("pkgdir")
            .to_string_lossy()
            .into_owned(),
        ..test_config()
    };
    let local_binpkg = build_local_binpkg_index(&config);
    let got = resolve_info_binary_candidate(&config, "dev-libs/binaryinfopkg", &local_binpkg)
        .expect("no error")
        .expect("a candidate");
    assert_eq!(got.cpv, "dev-libs/binaryinfopkg-1.0");
    assert_eq!(got.repo_name, "testrepo");
    assert_eq!(got.pf, "binaryinfopkg-1.0");
    assert_eq!(
        got.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );

    // A binary with DEFINED_PHASES="-" is not a pkg_info() candidate.
    assert!(
        resolve_info_binary_candidate(&config, "dev-libs/binaryonlypkg", &local_binpkg)
            .expect("no error")
            .is_none()
    );
    assert!(has_local_binary_candidate(
        &local_binpkg,
        "dev-libs/binaryonlypkg"
    ));
}
