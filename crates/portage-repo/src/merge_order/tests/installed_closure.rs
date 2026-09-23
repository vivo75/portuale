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

#[test]
fn add_installed_dependency_closure_strips_the_injected_libc_dep() {
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
    let root = mo_tmp_vdb(
        "closure-libc",
        &[
            (
                "virtual",
                "libc-1.0",
                &[("SLOT", "0"), ("RDEPEND", "sys-libs/glibc-mine")],
            ),
            ("dev-libs", "inj-1.0", files),
        ],
    );
    let mut entries = vec![new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
    )];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &[],
        system_atoms: &[],
        virtuals_only: false,
        dynamic_deps: true,
    });
    let inj = entries
        .iter()
        .find(|e| e.package == "inj")
        .expect("the installed dep is seeded");
    let atoms: HashSet<&str> = inj.deps.iter().map(|e| e.atom.as_str()).collect();
    assert!(
        !atoms.contains(">=sys-libs/glibc-mine-2.38"),
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
    // Without an installed libc provider there is no injection to
    // strip: the same atom stays.
    let root = mo_tmp_vdb("closure-libc-none", &[("dev-libs", "inj-1.0", files)]);
    let mut entries = vec![new_entry(
        "app-misc",
        "top",
        "1.0",
        vec![runtime_edge("dev-libs/inj", "dev-libs", "inj")],
    )];
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut entries,
        root: &root,
        repos: &[],
        system_atoms: &[],
        virtuals_only: false,
        dynamic_deps: true,
    });
    let inj = entries.iter().find(|e| e.package == "inj").unwrap();
    assert!(
        inj.deps
            .iter()
            .any(|e| e.atom == ">=sys-libs/glibc-mine-2.38"),
        "no libc provider means nothing is stripped: {:?}",
        inj.deps.iter().map(|e| e.atom.as_str()).collect::<Vec<_>>()
    );
}
