use super::*;

#[test]
fn depclean_cleanlist_keeps_the_required_closure_and_lists_the_rest() {
    // dcworld (world) -> dcdep -> dcsub, dcworld -[bar?]-> dccond; dcorphan chain
    // orphaned; systempkg required as a root.
    let root = masters_test_root("depclean");
    let install = |name: &str, rdepend: &str, use_str: &str, depend: &str, bdepend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
        if !depend.is_empty() {
            std::fs::write(d.join("DEPEND"), format!("{depend}\n")).unwrap();
        }
        if !bdepend.is_empty() {
            std::fs::write(d.join("BDEPEND"), format!("{bdepend}\n")).unwrap();
        }
        if !use_str.is_empty() {
            std::fs::write(d.join("USE"), format!("{use_str}\n")).unwrap();
        }
    };
    install(
        "dcworld",
        "dev-libs/dcdep bar? ( dev-libs/dccond )",
        "bar",
        "dev-libs/dcbuilddep",
        "",
    );
    install("dcdep", "dev-libs/dcsub", "", "", "dev-libs/dcbdep");
    install("dcsub", "", "", "", "");
    install("dccond", "", "", "", "");
    install("dcbuilddep", "", "", "", "");
    install("dcbdep", "", "", "", "");
    install("dcorphan", "dev-libs/dcorphandep", "", "", "");
    install("dcorphandep", "", "", "", "");
    install("systempkg", "", "", "", "");

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        system_atoms: &["dev-libs/systempkg".to_string()],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(
        clean,
        vec![
            "dev-libs/dcorphan-1.0".to_string(),
            "dev-libs/dcorphandep-1.0".to_string(),
        ]
    );
    // Seven required packages (world closure plus DEPEND/BDEPEND/system).
    assert_eq!(result.required_count, 7);

    // `args` mode narrows to the named package; its private dep stays protected.
    let narrowed = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        system_atoms: &["dev-libs/systempkg".to_string()],
        args: &["dev-libs/dcorphan".to_string()],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert_eq!(
        narrowed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/dcorphan-1.0".to_string()]
    );
    // Still needed by dcdep: nothing listed.
    let needed = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        system_atoms: &["dev-libs/systempkg".to_string()],
        args: &["dev-libs/dcsub".to_string()],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(needed.cleanlist.is_empty());

    // Protected provider keeps its own RDEPEND too: empty cleanlist, nine required.
    let protected = InstalledPackage {
        category: "dev-libs".to_string(),
        package: "dcorphan".to_string(),
        version: "1.0".to_string(),
        slot: "0".to_string(),
    };
    let lib_kept = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        system_atoms: &["dev-libs/systempkg".to_string()],
        args: &[],
        deselect: true,
        lib_protected_providers: std::slice::from_ref(&protected),
        package_provided: &[],
    });
    assert!(lib_kept.cleanlist.is_empty(), "{:?}", lib_kept.cleanlist);
    assert_eq!(lib_kept.required_count, 9);

    // No unsatisfiable hard runtime dep here.
    assert!(result.unresolved.is_empty(), "{:?}", result.unresolved);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_flags_a_kept_packages_unsatisfiable_runtime_dep() {
    let root = masters_test_root("depclean-unresolved");
    let install = |name: &str, rdepend: &str, depend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
        if !depend.is_empty() {
            std::fs::write(d.join("DEPEND"), format!("{depend}\n")).unwrap();
        }
    };
    // uk RDEPENDs a missing package plus a satisfied `||` group; its missing
    // DEPEND is build-time (soft), so ignored.
    install("uw", "dev-libs/uk", "");
    install(
        "uk",
        "dev-libs/umissing || ( dev-libs/uany dev-libs/ualtmissing )",
        "dev-libs/ubuildmissing",
    );
    install("uany", "", "");

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/uw".to_string(), "@selected".to_string())],
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert_eq!(
        result.unresolved,
        vec![(
            "dev-libs/umissing".to_string(),
            "dev-libs/uk-1.0".to_string()
        )]
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_deselect_false_keeps_a_world_member_named_as_an_arg() {
    // dw (world) has no reverse deps.
    let root = masters_test_root("depclean-deselect");
    let d = root.join("var/db/pkg/dev-libs/dw-1.0");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    std::fs::write(d.join("SLOT"), "0\n").unwrap();
    let world = &[("dev-libs/dw".to_string(), "@selected".to_string())];
    let args = &["dev-libs/dw".to_string()];

    // Deselect drops the world seed in args mode: dw becomes unreachable.
    let removed = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args,
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert_eq!(
        removed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/dw-1.0".to_string()]
    );

    // `--deselect=n` keeps the seed: @selected still reaches dw.
    let kept = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args,
        deselect: false,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(kept.cleanlist.is_empty(), "{:?}", kept.cleanlist);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_records_reverse_dep_parents_for_kept_packages() {
    let root = masters_test_root("depclean-revdep");
    let install = |name: &str, rdepend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    install("rw", "dev-libs/rdep dev-libs/rshared");
    install("rdep", "dev-libs/rshared");
    install("rshared", "");
    install("rorphan", "");

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/rw".to_string(), "@selected".to_string())],
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    // rorphan is the cleanlist; kept parents render in cpv order.
    assert_eq!(
        result.cleanlist.iter().map(|p| p.cpv()).collect::<Vec<_>>(),
        vec!["dev-libs/rorphan-1.0".to_string()]
    );
    let parents: Vec<(String, Vec<String>)> = result
        .kept_parents
        .iter()
        .map(|(p, lines)| (p.cpv(), lines.clone()))
        .collect();
    assert_eq!(
        parents,
        vec![
            (
                "dev-libs/rdep-1.0".to_string(),
                vec!["dev-libs/rw-1.0 requires dev-libs/rdep".to_string()],
            ),
            (
                "dev-libs/rshared-1.0".to_string(),
                vec![
                    "dev-libs/rdep-1.0 requires dev-libs/rshared".to_string(),
                    "dev-libs/rw-1.0 requires dev-libs/rshared".to_string(),
                ],
            ),
            (
                "dev-libs/rw-1.0".to_string(),
                vec!["@selected requires dev-libs/rw".to_string()],
            ),
        ]
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_package_provided_intercepts_both_a_world_root_and_a_dependency_edge() {
    // Portage intercepts `package.provided` before the root forms and on the
    // edge, so world membership does not protect either package.
    let root = masters_test_root("depclean-pprovided");
    let install = |name: &str, rdepend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    install("pw", "dev-libs/pdprov");
    install("pwprov", "");
    install("pdprov", "");

    let world = &[
        ("dev-libs/pw".to_string(), "@selected".to_string()),
        ("dev-libs/pwprov".to_string(), "@selected".to_string()),
    ];
    let package_provided = vec![
        "dev-libs/pwprov-1.0".to_string(),
        "dev-libs/pdprov-1.0".to_string(),
    ];

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &package_provided,
    });
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(
        clean,
        vec![
            "dev-libs/pdprov-1.0".to_string(),
            "dev-libs/pwprov-1.0".to_string(),
        ]
    );

    // Without `package_provided` both stay protected: nothing to clean.
    let baseline = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(baseline.cleanlist.is_empty(), "{:?}", baseline.cleanlist);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_unresolved_halt_does_not_fire_for_a_dep_satisfied_only_by_package_provided() {
    // Portage treats a `package.provided`-matched atom as satisfied, so no halt.
    let root = masters_test_root("depclean-unresolved-pprovided");
    let install = |name: &str, rdepend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    install("uw", "dev-libs/uk");
    install("uk", "dev-libs/uprovided");

    let world = &[("dev-libs/uw".to_string(), "@selected".to_string())];
    let package_provided = vec!["dev-libs/uprovided-9.9".to_string()];

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &package_provided,
    });
    assert!(result.unresolved.is_empty(), "{:?}", result.unresolved);

    // Without `package_provided` the same setup halts.
    let baseline = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: world,
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert_eq!(
        baseline.unresolved,
        vec![(
            "dev-libs/uprovided".to_string(),
            "dev-libs/uk-1.0".to_string()
        )]
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_a_use_conditional_dep_that_is_off_does_not_keep_its_target() {
    let root = masters_test_root("depclean-usecond");
    let install = |name: &str, rdepend: &str, use_str: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
        if !use_str.is_empty() {
            std::fs::write(d.join("USE"), format!("{use_str}\n")).unwrap();
        }
    };
    // Inactive `off?` group: dchidden is orphan.
    install("dcw", "off? ( dev-libs/dchidden )", "");
    install("dchidden", "", "");
    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[("dev-libs/dcw".to_string(), "@selected".to_string())],
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(clean, vec!["dev-libs/dchidden-1.0".to_string()]);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_is_returned_in_topological_removal_order() {
    // Alphabetical order differs from dependency order: Portage removes a package
    // before the ones it depends on (dependents first, cpv-descending per level).
    let root = masters_test_root("depclean-order");
    let install = |name: &str, rdepend: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    install("mmid", "dev-libs/zztop");
    install("zztop", "dev-libs/aabase");
    install("aabase", "");
    install("loner", "");

    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[],
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(result.ordered);
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    // Level 0 ready first (cpv-descending), then dependents' deps.
    assert_eq!(
        clean,
        vec![
            "dev-libs/mmid-1.0".to_string(),
            "dev-libs/loner-1.0".to_string(),
            "dev-libs/zztop-1.0".to_string(),
            "dev-libs/aabase-1.0".to_string(),
        ]
    );

    // No edges: unordered, cat/pn sort kept.
    let root2 = masters_test_root("depclean-order-flat");
    for n in ["ccc", "aaa", "bbb"] {
        let d = root2.join("var/db/pkg/dev-libs").join(format!("{n}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
    }
    let flat = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root2,
        world_seeds: &[],
        system_atoms: &[],
        args: &[],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(!flat.ordered);
    assert_eq!(
        flat.cleanlist.iter().map(|p| p.cpv()).collect::<Vec<_>>(),
        vec![
            "dev-libs/aaa-1.0".to_string(),
            "dev-libs/bbb-1.0".to_string(),
            "dev-libs/ccc-1.0".to_string(),
        ]
    );

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&root2).ok();
}

#[test]
fn topological_removal_order_breaks_a_cycle_by_priority_and_pops_one() {
    // Two-node cycle with no true root: Portage's priority scan pops `lo` first,
    // then `hi` falls out as a plain root.
    let root = masters_test_root("depclean-cycle");
    let mk = |name: &str, key: &str, dep: &str| {
        let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        std::fs::write(d.join(key), format!("{dep}\n")).unwrap();
    };
    mk("hi", "DEPEND", "dev-libs/lo");
    mk("lo", "RDEPEND", "dev-libs/hi");

    let (ordered, out) = topological_removal_order(
        &root,
        vec![
            InstalledPackage {
                category: "dev-libs".into(),
                package: "hi".into(),
                version: "1.0".into(),
                slot: "0".into(),
            },
            InstalledPackage {
                category: "dev-libs".into(),
                package: "lo".into(),
                version: "1.0".into(),
                slot: "0".into(),
            },
        ],
    );
    assert!(ordered);
    assert_eq!(
        out.iter().map(|p| p.package.clone()).collect::<Vec<_>>(),
        vec!["lo".to_string(), "hi".to_string()]
    );

    // Mutual RDEPEND: the cpv-max node pops first.
    let root2 = masters_test_root("depclean-cycle-rr");
    mk_at(&root2, "zz", "RDEPEND", "dev-libs/yy");
    mk_at(&root2, "yy", "RDEPEND", "dev-libs/zz");
    let (_o, out2) = topological_removal_order(
        &root2,
        vec![
            InstalledPackage {
                category: "dev-libs".into(),
                package: "zz".into(),
                version: "1.0".into(),
                slot: "0".into(),
            },
            InstalledPackage {
                category: "dev-libs".into(),
                package: "yy".into(),
                version: "1.0".into(),
                slot: "0".into(),
            },
        ],
    );
    assert_eq!(
        out2.iter().map(|p| p.package.clone()).collect::<Vec<_>>(),
        vec!["zz".to_string(), "yy".to_string()]
    );

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&root2).ok();
}

#[test]
fn prune_cleanlist_removes_superseded_versions_only() {
    let root = masters_test_root("prune");
    let install = |name: &str, version: &str, rdepend: &str| {
        let d = root
            .join("var/db/pkg/dev-libs")
            .join(format!("{name}-{version}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    // keeper pins mm-2.0; zz-1.0's pin on aa-1.0 is only an ordering edge.
    install("aa", "1.0", "");
    install("aa", "2.0", "");
    install("zz", "1.0", "=dev-libs/aa-1.0");
    install("zz", "2.0", "");
    install("mm", "1.0", "");
    install("mm", "2.0", "");
    install("mm", "3.0", "");
    install("keeper", "1.0", "=dev-libs/mm-2.0");
    install("single", "1.0", "");

    let result = prune_cleanlist(&root, &[], &[], &[]);
    assert!(result.ordered);
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    // mm-2.0 kept, the rest pruned in topological order.
    assert_eq!(
        clean,
        vec![
            "dev-libs/zz-1.0".to_string(),
            "dev-libs/mm-1.0".to_string(),
            "dev-libs/aa-1.0".to_string(),
        ]
    );

    // Reverse-dep display names only the kept non-highest version with a parent.
    assert_eq!(
        result
            .kept_parents
            .iter()
            .map(|(p, lines)| (p.cpv(), lines.clone()))
            .collect::<Vec<_>>(),
        vec![(
            "dev-libs/mm-2.0".to_string(),
            vec!["dev-libs/keeper-1.0 requires =dev-libs/mm-2.0".to_string()],
        )]
    );

    // `--nodeps`: no dependency check, so every non-highest version is selected.
    let nd = prune_nodeps_selection(&root, &[]);
    assert_eq!(
        nd.iter()
            .map(|cp| (
                format!("{}/{}", cp.category, cp.package),
                cp.best_version.clone(),
                cp.other_versions.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "dev-libs/aa".to_string(),
                "2.0".to_string(),
                vec!["1.0".to_string()]
            ),
            (
                "dev-libs/mm".to_string(),
                "3.0".to_string(),
                vec!["1.0".to_string(), "2.0".to_string()]
            ),
            (
                "dev-libs/zz".to_string(),
                "2.0".to_string(),
                vec!["1.0".to_string()]
            ),
        ]
    );
    // `--nodeps <atom>`: best/rest split over the atom's own match set.
    let nd_atom = prune_nodeps_selection(&root, &["<dev-libs/mm-3.0".to_string()]);
    assert_eq!(
        nd_atom
            .iter()
            .map(|cp| (cp.best_version.clone(), cp.other_versions.clone()))
            .collect::<Vec<_>>(),
        vec![("2.0".to_string(), vec!["1.0".to_string()])]
    );

    // Named package: only its old versions are candidates.
    let narrowed = prune_cleanlist(&root, &["dev-libs/mm".to_string()], &[], &[]);
    assert_eq!(
        narrowed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/mm-1.0".to_string()]
    );

    // Single-version cps: nothing to prune.
    let root2 = masters_test_root("prune-none");
    for n in ["one", "two"] {
        let d = root2.join("var/db/pkg/dev-libs").join(format!("{n}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
    }
    assert!(prune_cleanlist(&root2, &[], &[], &[]).cleanlist.is_empty());

    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&root2).ok();
}

/// Installs `(package, RDEPEND)` pairs under `dev-libs` in a fresh root.
fn install_dev_libs_root(name: &str, installed: &[(&str, &str)]) -> std::path::PathBuf {
    let root = masters_test_root(name);
    for (package, rdepend) in installed {
        let d = root
            .join("var/db/pkg/dev-libs")
            .join(format!("{package}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
        if !rdepend.is_empty() {
            std::fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    root
}

/// `(pkg, members)` of each cycle notice, cpv strings.
fn cycle_notices(result: &DepcleanResult) -> Vec<(String, Vec<String>)> {
    result
        .cycle_suggestions
        .iter()
        .map(|s| {
            let mut members: Vec<String> = s.members.iter().map(|m| m.cpv()).collect();
            members.sort();
            (s.pkg.cpv(), members)
        })
        .collect()
}

#[test]
fn depclean_cycle_notice_names_the_pair_when_only_the_cycle_keeps_it() {
    // #508 / Portage 3.0.83-r1 `_calc_depclean`: A and B require each other,
    // B is protected (not named), C is world. `--depclean A` removes nothing
    // and the notice names both members (oracle: LLM/evidence/2026-10-23-508).
    let root = install_dev_libs_root(
        "depclean-cycle-pair-508",
        &[
            ("dccyca", "dev-libs/dccycb"),
            ("dccycb", "dev-libs/dccyca"),
            ("dccycw", ""),
        ],
    );
    let world = [("dev-libs/dccycw".to_string(), "@selected".to_string())];
    let run = |args: &[String]| {
        depclean_cleanlist(DepcleanCleanlistArgs {
            root: &root,
            world_seeds: &world,
            system_atoms: &[],
            args,
            deselect: true,
            lib_protected_providers: &[],
            package_provided: &[],
        })
    };
    let result = run(&["dev-libs/dccyca".to_string()]);
    assert!(result.cleanlist.is_empty());
    assert_eq!(
        cycle_notices(&result),
        vec![(
            "dev-libs/dccyca-1.0".to_string(),
            vec![
                "dev-libs/dccyca-1.0".to_string(),
                "dev-libs/dccycb-1.0".to_string()
            ],
        )]
    );
    // Naming the other member gives its own notice, with the same members.
    let result = run(&["dev-libs/dccycb".to_string()]);
    assert_eq!(
        cycle_notices(&result)
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        vec!["dev-libs/dccycb-1.0"]
    );
    // Without arguments there is no notice (Portage only asks in `args` mode).
    assert!(run(&[]).cycle_suggestions.is_empty());
}

#[test]
fn depclean_cycle_notice_is_silenced_by_a_parent_outside_the_cycle() {
    // dccycp is protected (not named) and requires dccyca: Portage's
    // `_parent_atoms` of dccyca then holds a Package outside the cycle.
    let root = install_dev_libs_root(
        "depclean-cycle-outside-508",
        &[
            ("dccyca", "dev-libs/dccycb"),
            ("dccycb", "dev-libs/dccyca"),
            ("dccycp", "dev-libs/dccyca"),
            ("dccycw", ""),
        ],
    );
    let world = [("dev-libs/dccycw".to_string(), "@selected".to_string())];
    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &world,
        system_atoms: &[],
        args: &["dev-libs/dccyca".to_string()],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(result.cycle_suggestions.is_empty());
    assert!(!result.kept_parents.is_empty());
}

#[test]
fn depclean_cycle_notice_lists_every_member_of_a_three_cycle() {
    // dccycx -> dccycy -> dccycz -> dccycx: one notice, three members.
    let root = install_dev_libs_root(
        "depclean-cycle-three-508",
        &[
            ("dccycx", "dev-libs/dccycy"),
            ("dccycy", "dev-libs/dccycz"),
            ("dccycz", "dev-libs/dccycx"),
            ("dccycw", ""),
        ],
    );
    let world = [("dev-libs/dccycw".to_string(), "@selected".to_string())];
    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &world,
        system_atoms: &[],
        args: &["dev-libs/dccycx".to_string()],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert_eq!(
        cycle_notices(&result),
        vec![(
            "dev-libs/dccycx-1.0".to_string(),
            vec![
                "dev-libs/dccycx-1.0".to_string(),
                "dev-libs/dccycy-1.0".to_string(),
                "dev-libs/dccycz-1.0".to_string(),
            ],
        )]
    );
}

#[test]
fn depclean_cycle_notice_ignores_a_package_that_only_requires_itself() {
    // A self-edge is a cycle of one: no notice (Portage `len(members) < 2`).
    let root = install_dev_libs_root("depclean-cycle-self-508", &[("dccyca", "dev-libs/dccyca")]);
    let result = depclean_cleanlist(DepcleanCleanlistArgs {
        root: &root,
        world_seeds: &[],
        system_atoms: &[],
        args: &["dev-libs/dccyca".to_string()],
        deselect: true,
        lib_protected_providers: &[],
        package_provided: &[],
    });
    assert!(result.cycle_suggestions.is_empty());
}

#[test]
fn strongly_connected_components_groups_cycles_and_survives_long_chains() {
    // 0 -> 1 -> 2 -> 0 is one component; 3 -> 2 hangs off it; 4 <-> 5 is
    // another; 6 stands alone.
    let adj = vec![vec![1], vec![2], vec![0], vec![2], vec![5], vec![4], vec![]];
    let comp = crate::depclean::strongly_connected_components(&adj);
    assert_eq!(comp[0], comp[1]);
    assert_eq!(comp[1], comp[2]);
    assert_eq!(comp[4], comp[5]);
    let distinct = [comp[0], comp[3], comp[4], comp[6]];
    for (i, a) in distinct.iter().enumerate() {
        for b in &distinct[i + 1..] {
            assert_ne!(a, b);
        }
    }
    // A 200 000 node chain has no cycles and must not overflow the stack.
    let n = 200_000;
    let chain: Vec<Vec<usize>> = (0..n)
        .map(|i| if i + 1 < n { vec![i + 1] } else { vec![] })
        .collect();
    let comp = crate::depclean::strongly_connected_components(&chain);
    let mut seen: Vec<usize> = comp.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), n);
}
