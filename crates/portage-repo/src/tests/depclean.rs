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
