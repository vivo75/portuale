use super::*;

#[test]
fn edge_satisfied_with_pins_the_slot_operator_narrowing() {
    let map = installed_by_cp(&[(
        "dev-libs",
        "slotted",
        &["dev-libs/slotted-1.0:0/0", "dev-libs/slotted-2.0:1/1"],
    )]);
    let plain = runtime_edge("dev-libs/slotted", "dev-libs", "slotted");
    assert!(edge_satisfied_with(&map, &plain, None));
    // No candidates, no atom match, no match at all.
    assert!(!edge_satisfied_with(&installed_by_cp(&[]), &plain, None));
    assert!(!edge_satisfied_with(
        &map,
        &runtime_edge(">=dev-libs/slotted-3", "dev-libs", "slotted"),
        None
    ));
    assert!(!edge_satisfied_with(
        &map,
        &runtime_edge("dev-libs/absent", "dev-libs", "absent"),
        None
    ));
    // A slot-operator edge with no resolved child falls back to the
    // plain atom check.
    let slotop = plain_edge(
        "dev-libs/slotted",
        "dev-libs",
        "slotted",
        DepPriority {
            runtime: true,
            runtime_slot_op: true,
            ..DepPriority::default()
        },
    );
    assert!(edge_satisfied_with(&map, &slotop, None));
    // With a resolved child it must carry the child's own slot/sub.
    assert!(edge_satisfied_with(&map, &slotop, Some(("1", "1"))));
    assert!(edge_satisfied_with(&map, &slotop, Some(("0", "0"))));
    assert!(
        !edge_satisfied_with(&map, &slotop, Some(("0", "1"))),
        "a half-matching slot/sub-slot is not satisfied"
    );
    assert!(
        !edge_satisfied_with(&map, &slotop, Some(("2", "2"))),
        "no candidate carries the child's slot"
    );
    // The build-time slot-operator variant narrows identically.
    let bslotop = plain_edge(
        "dev-libs/slotted",
        "dev-libs",
        "slotted",
        DepPriority {
            buildtime: true,
            buildtime_slot_op: true,
            ..DepPriority::default()
        },
    );
    assert!(edge_satisfied_with(&map, &bslotop, Some(("0", "0"))));
    assert!(!edge_satisfied_with(&map, &bslotop, Some(("2", "2"))));
}

#[test]
fn installed_candidates_by_cp_reads_the_fixture_vdb() {
    let root = fixtures_vdb_root();
    let map = installed_candidates_by_cp(&root);
    let mut got = map
        .get(&("dev-libs".to_string(), "dualslotpkg".to_string()))
        .cloned()
        .expect("the fixture vdb carries two dualslotpkg slots");
    got.sort();
    assert_eq!(
        got,
        vec![
            "dev-libs/dualslotpkg-1.0:1/1".to_string(),
            "dev-libs/dualslotpkg-2.0:2/2".to_string()
        ]
    );
    assert!(!map.contains_key(&("dev-libs".to_string(), "definitely-absent".to_string())));
}

#[test]
fn dep_edge_satisfied_by_installed_reads_the_vdb() {
    let root = mo_tmp_vdb(
        "satisfied",
        &[(
            "dev-libs",
            "inst-1.0",
            &[("SLOT", "0/1"), ("RDEPEND", "dev-libs/other")],
        )],
    );
    assert!(dep_edge_satisfied_by_installed(
        &root,
        &runtime_edge("dev-libs/inst", "dev-libs", "inst"),
        None
    ));
    assert!(!dep_edge_satisfied_by_installed(
        &root,
        &runtime_edge(">=dev-libs/inst-2", "dev-libs", "inst"),
        None
    ));
    // A slot-operator edge follows the resolved child's own slot.
    let slotop = plain_edge(
        "dev-libs/inst",
        "dev-libs",
        "inst",
        DepPriority {
            runtime: true,
            runtime_slot_op: true,
            ..DepPriority::default()
        },
    );
    let mut child = new_entry("dev-libs", "inst", "2.0", Vec::new());
    child.slot = Some("1".into());
    child.sub_slot = Some("1".into());
    assert!(!dep_edge_satisfied_by_installed(
        &root,
        &slotop,
        Some(&child)
    ));
    child.slot = Some("0".into());
    child.sub_slot = Some("1".into());
    assert!(dep_edge_satisfied_by_installed(
        &root,
        &slotop,
        Some(&child)
    ));
}

#[test]
fn add_installed_dependency_closure_virtuals_only_expands_virtuals() {
    let root = mo_tmp_vdb(
        "closure-virtuals",
        &[
            (
                "virtual",
                "libc-1.0",
                &[("SLOT", "0"), ("RDEPEND", "sys-libs/glibc-mine")],
            ),
            (
                "sys-libs",
                "glibc-mine-2.38",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/glibc-dep")],
            ),
            ("dev-libs", "glibc-dep-1.0", &[("SLOT", "0")]),
            (
                "dev-libs",
                "plain-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/plain-dep")],
            ),
            ("dev-libs", "plain-dep-1.0", &[("SLOT", "0")]),
            ("dev-libs", "systemseed-1.0", &[("SLOT", "0")]),
        ],
    );
    let top = new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![
            runtime_edge("virtual/libc", "virtual", "libc"),
            runtime_edge("dev-libs/plain", "dev-libs", "plain"),
        ],
    );
    let mut entries = vec![top];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &[],
        system_atoms: &["dev-libs/systemseed".to_string()],
        virtuals_only: true,
        dynamic_deps: true,
    });
    let packages: Vec<&str> = entries.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(
        packages,
        vec!["top", "libc", "plain", "glibc-mine"],
        "{packages:?}"
    );
    assert_eq!(
        entries[1]
            .deps
            .iter()
            .map(|e| e.atom.as_str())
            .collect::<Vec<_>>(),
        vec!["sys-libs/glibc-mine"],
        "the virtual expands to its provider"
    );
    assert!(
        entries[2].deps.is_empty(),
        "a non-virtual installed node is a leaf under virtuals_only"
    );
    assert!(
        entries[3].deps.is_empty(),
        "the provider is added as a leaf, not queued: {:?}",
        entries[3].deps
    );
    assert!(
        !packages.contains(&"glibc-dep") && !packages.contains(&"plain-dep"),
        "non-virtual nodes are never expanded under virtuals_only: {packages:?}"
    );
    assert!(
        !packages.contains(&"systemseed"),
        "virtuals_only does not seed @system: {packages:?}"
    );
}

/// A scratch repo carrying `dev-libs/inj-1.0`'s md5-cache entry, so the
/// installed package has a live ebuild.
fn libc_inj_repo(tag: &str) -> (TempDir, Vec<RepoConfig>) {
    use md5::Digest as _;
    let base = TempDir::new(&format!("portuale-mo-repo-{}-{}", std::process::id(), tag));
    let dir = base.join("r/dev-libs/inj");
    std::fs::create_dir_all(&dir).unwrap();
    let ebuild =
        "EAPI=8\nDESCRIPTION=\"x\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"dev-libs/keep\"\n";
    std::fs::write(dir.join("inj-1.0.ebuild"), ebuild).unwrap();
    let md5 = portage_util::hex_lower(&md5::Md5::digest(ebuild.as_bytes()));
    let cache = base.join("r/metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("inj-1.0"),
        format!(
            "DEFINED_PHASES=-\nDESCRIPTION=x\nEAPI=8\nKEYWORDS=amd64\nRDEPEND=dev-libs/keep\nSLOT=0\n_md5_={md5}\n"
        ),
    )
    .unwrap();
    let repos = vec![RepoConfig {
        name: "r".to_string(),
        location: base.join("r"),
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
    (base, repos)
}

/// Portage reads an installed package's deps from its ebuild when the tree
/// still has it (the injected libc atom is then never seen) and from the vdb
/// when the ebuild is gone (the atom is a real runtime edge).
// Portage: _emerge/depgraph.py, _add_pkg_deps (dynamic deps fallback)
#[test]
fn add_installed_dependency_closure_strips_the_injected_libc_dep_only_with_an_ebuild() {
    let files: &[(&str, &str)] = &[
        ("SLOT", "0"),
        (
            "RDEPEND",
            ">=sys-libs/glibc-mine-2.38 dev-libs/keep \
             >sys-libs/glibc-mine-2.38 \
             >=sys-libs/glibc-mine-2.38:2.38 >=sys-libs/glibc-mine-2.38[foo] \
             =sys-libs/glibc-mine-2.38 >=sys-libs/other-2.38",
        ),
    ];
    let vdb = &[
        (
            "virtual",
            "libc-1.0",
            &[("SLOT", "0"), ("RDEPEND", "sys-libs/glibc-mine")][..],
        ),
        ("dev-libs", "inj-1.0", files),
    ];
    let run = |tag: &str, repos: &[RepoConfig]| -> HashSet<String> {
        let root = mo_tmp_vdb(tag, vdb);
        let mut entries = vec![new_entry(
            "app-misc",
            "top",
            "1.0",
            vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
        )];
        add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
            entries: &mut entries,
            root: &root,
            repos,
            system_atoms: &[],
            virtuals_only: false,
            dynamic_deps: true,
        });
        let inj = entries
            .iter()
            .find(|e| e.package == "inj")
            .expect("the installed dep is seeded");
        inj.deps.iter().map(|e| e.atom.clone()).collect()
    };
    let injected = ">=sys-libs/glibc-mine-2.38";

    // The ebuild is in the tree: the injected bare atom is stripped.
    let (_keep, repos) = libc_inj_repo("with-ebuild");
    let atoms = run("closure-libc", &repos);
    assert!(
        !atoms.contains(injected),
        "the injected bare `>=libc-provider-version` atom is stripped: {atoms:?}"
    );
    for kept in [
        "dev-libs/keep",
        ">sys-libs/glibc-mine-2.38",
        ">=sys-libs/glibc-mine-2.38:2.38",
        ">=sys-libs/glibc-mine-2.38[foo]",
        "=sys-libs/glibc-mine-2.38",
        ">=sys-libs/other-2.38",
    ] {
        assert!(
            atoms.contains(kept),
            "only the exact injected shape is stripped; {kept} must stay: {atoms:?}"
        );
    }

    // The ebuild is gone from the tree: Portage walks the vdb string, so the
    // atom stays a runtime edge of the installed package.
    let atoms = run("closure-libc-no-ebuild", &[]);
    assert!(
        atoms.contains(injected),
        "no ebuild means the vdb's libc atom is kept: {atoms:?}"
    );

    // Without an installed libc provider there is no injection to strip.
    let root = mo_tmp_vdb("closure-libc-none", &[("dev-libs", "inj-1.0", files)]);
    let (_keep, repos) = libc_inj_repo("none");
    let mut entries = vec![new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
    )];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &repos,
        system_atoms: &[],
        virtuals_only: false,
        dynamic_deps: true,
    });
    let inj = entries.iter().find(|e| e.package == "inj").unwrap();
    assert!(
        inj.deps.iter().any(|e| e.atom == injected),
        "no libc provider means nothing is stripped: {:?}",
        inj.deps.iter().map(|e| e.atom.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn usable_installed_by_cp_drops_a_package_whose_slot_operator_binding_the_plan_breaks() {
    // #680: the installed XS module is bound to perl:0/1.0; the plan merges
    // perl at sub-slot 2.0, so the installed module cannot stand in for its
    // rebuild. The unbound tool stays usable.
    let root = mo_tmp_vdb(
        "usable",
        &[
            ("dev-lang", "perl-1.0", &[("SLOT", "0/1.0")]),
            (
                "dev-perl",
                "xs-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-lang/perl:0/1.0=")],
            ),
            (
                "dev-util",
                "tool-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-perl/xs")],
            ),
        ],
    );
    let mut perl = new_entry("dev-lang", "perl", "2.0", Vec::new());
    perl.slot = Some("0".into());
    perl.sub_slot = Some("2.0".into());
    let usable = usable_installed_by_cp(&root, std::slice::from_ref(&perl));
    let cands = |cat: &str, pkg: &str| usable[&(cat.to_string(), pkg.to_string())].clone();
    assert!(cands("dev-perl", "xs").is_empty());
    assert_eq!(cands("dev-util", "tool"), ["dev-util/tool-1.0:0/0"]);
    // The same sub-slot breaks nothing.
    perl.sub_slot = Some("1.0".into());
    let usable = usable_installed_by_cp(&root, std::slice::from_ref(&perl));
    assert_eq!(
        usable[&("dev-perl".into(), "xs".into())],
        ["dev-perl/xs-1.0:0/0"]
    );
}
