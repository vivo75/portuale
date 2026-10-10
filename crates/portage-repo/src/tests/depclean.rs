use super::*;

#[test]
fn depclean_cleanlist_keeps_the_required_closure_and_lists_the_rest() {
    // A tiny installed graph: dcworld (required, world) -> dcdep ->
    // dcsub, dcworld -[bar?, USE=bar]-> dccond; dcorphan -> dcorphandep
    // (both orphan). systempkg is required too (passed as a root).
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

    let result = depclean_cleanlist(
        &root,
        &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        &["dev-libs/systempkg".to_string()],
        &[],
        true,
        &[],
        &[],
    );
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(
        clean,
        vec![
            "dev-libs/dcorphan-1.0".to_string(),
            "dev-libs/dcorphandep-1.0".to_string(),
        ]
    );
    // dcworld, dcdep, dcsub, dccond, dcbuilddep (DEPEND), dcbdep
    // (BDEPEND), systempkg.
    assert_eq!(result.required_count, 7);

    // `args` mode: `-c dev-libs/dcorphan` -> just that one (its
    // private dep dcorphandep is protected, being non-arg).
    let narrowed = depclean_cleanlist(
        &root,
        &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        &["dev-libs/systempkg".to_string()],
        &["dev-libs/dcorphan".to_string()],
        true,
        &[],
        &[],
    );
    assert_eq!(
        narrowed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/dcorphan-1.0".to_string()]
    );
    // `-c dev-libs/dcsub` -> nothing (dcdep still needs it).
    let needed = depclean_cleanlist(
        &root,
        &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        &["dev-libs/systempkg".to_string()],
        &["dev-libs/dcsub".to_string()],
        true,
        &[],
        &[],
    );
    assert!(needed.cleanlist.is_empty());

    // `--depclean-lib-check` feedback: forcing dcorphan as a
    // protected provider keeps it *and* its own RDEPEND dcorphandep,
    // recomputing an empty cleanlist and a required_count of 9.
    let protected = InstalledPackage {
        category: "dev-libs".to_string(),
        package: "dcorphan".to_string(),
        version: "1.0".to_string(),
        slot: "0".to_string(),
    };
    let lib_kept = depclean_cleanlist(
        &root,
        &[("dev-libs/dcworld".to_string(), "@selected".to_string())],
        &["dev-libs/systempkg".to_string()],
        &[],
        true,
        std::slice::from_ref(&protected),
        &[],
    );
    assert!(lib_kept.cleanlist.is_empty(), "{:?}", lib_kept.cleanlist);
    assert_eq!(lib_kept.required_count, 9);

    // No kept package has an unsatisfiable hard runtime dep here.
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
    // uw (world) -> uk; uk RDEPENDs a missing package + a satisfied
    // `||` group, and DEPENDs a missing build dep (SOFT, ignored).
    install("uw", "dev-libs/uk", "");
    install(
        "uk",
        "dev-libs/umissing || ( dev-libs/uany dev-libs/ualtmissing )",
        "dev-libs/ubuildmissing",
    );
    install("uany", "", "");

    let result = depclean_cleanlist(
        &root,
        &[("dev-libs/uw".to_string(), "@selected".to_string())],
        &[],
        &[],
        true,
        &[],
        &[],
    );
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

    // `-c dev-libs/dw` (deselect default true): the world seed is
    // dropped in args mode -> dw is unreachable -> removed.
    let removed = depclean_cleanlist(&root, world, &[], args, true, &[], &[]);
    assert_eq!(
        removed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/dw-1.0".to_string()]
    );

    // `-c dev-libs/dw --deselect=n`: the world seed is kept ->
    // @selected still reaches dw -> nothing to remove.
    let kept = depclean_cleanlist(&root, world, &[], args, false, &[], &[]);
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

    let result = depclean_cleanlist(
        &root,
        &[("dev-libs/rw".to_string(), "@selected".to_string())],
        &[],
        &[],
        true,
        &[],
        &[],
    );
    // rorphan is the cleanlist; kept = rdep, rshared, rw (cpv order).
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
    // Real `-pc` advisory: "package.provided ... will be removed by
    // depclean, even if they are part of the world set." `pwprov` is
    // a @world member that is also listed in `package_provided` --
    // real `_resolve`'s per-arg loop intercepts it before it ever
    // becomes a graph root (`depgraph.py:5497-5503`, *before*
    // `_add_pkg`/`_select_package` run), so world membership does not
    // protect it. `pdprov` is only reachable via `pw`'s RDEPEND,
    // which is also `package_provided`-listed -- real
    // `dep_check.py:1052` intercepts that edge too (no graph node
    // for the literal installed provider), so `pdprov` stays
    // unreachable and joins the cleanlist as well. `pw` itself (a
    // genuine, non-provided world member) stays required throughout.
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

    let result = depclean_cleanlist(&root, world, &[], &[], true, &[], &package_provided);
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(
        clean,
        vec![
            "dev-libs/pdprov-1.0".to_string(),
            "dev-libs/pwprov-1.0".to_string(),
        ]
    );

    // Without `package_provided`, both stay protected (pwprov by its
    // own world membership, pdprov by pw's RDEPEND) -- nothing to
    // clean.
    let baseline = depclean_cleanlist(&root, world, &[], &[], true, &[], &[]);
    assert!(baseline.cleanlist.is_empty(), "{:?}", baseline.cleanlist);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_unresolved_halt_does_not_fire_for_a_dep_satisfied_only_by_package_provided() {
    // `uk` (kept, reachable from world) RDEPENDs on
    // `dev-libs/uprovided`, which has no literal vdb entry at all --
    // only a `package_provided` listing. Real dependency resolution
    // treats a `package.provided`-matched atom as already satisfied
    // (`dep_check.py:1052`), so `unresolved_deps()` must not flag it;
    // without the fix this spuriously halts with "not being
    // installed" (real behaviour: it doesn't).
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

    let result = depclean_cleanlist(&root, world, &[], &[], true, &[], &package_provided);
    assert!(result.unresolved.is_empty(), "{:?}", result.unresolved);

    // Without `package_provided`, the same setup halts.
    let baseline = depclean_cleanlist(&root, world, &[], &[], true, &[], &[]);
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
    // dcw's `off? ( ... )` group is inactive -> dchidden is orphan.
    install("dcw", "off? ( dev-libs/dchidden )", "");
    install("dchidden", "", "");
    let result = depclean_cleanlist(
        &root,
        &[("dev-libs/dcw".to_string(), "@selected".to_string())],
        &[],
        &[],
        true,
        &[],
        &[],
    );
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    assert_eq!(clean, vec!["dev-libs/dchidden-1.0".to_string()]);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn depclean_cleanlist_is_returned_in_topological_removal_order() {
    // Two orphan chains where alphabetical order != dependency order:
    // dev-libs/zztop RDEPENDs dev-libs/aabase, and dev-libs/mmid
    // RDEPENDs dev-libs/zztop. Real _calc_depclean removes a package
    // before the ones it depends on, so the order is mmid, zztop,
    // aabase -- the reverse of the cat/pn sort. dev-libs/loner is a
    // fourth orphan with no edges: it sorts by cpv among each level's
    // ready set (emitted descending, real `nodes.sort(reverse=True)`).
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

    let result = depclean_cleanlist(&root, &[], &[], &[], true, &[], &[]);
    assert!(result.ordered);
    let clean: Vec<String> = result.cleanlist.iter().map(|p| p.cpv()).collect();
    // Level 0 ready = {mmid, loner} (nothing depends on them),
    // emitted cpv-descending (mmid > loner). Then zztop, then aabase.
    assert_eq!(
        clean,
        vec![
            "dev-libs/mmid-1.0".to_string(),
            "dev-libs/loner-1.0".to_string(),
            "dev-libs/zztop-1.0".to_string(),
            "dev-libs/aabase-1.0".to_string(),
        ]
    );

    // No edges at all -> ordered = false, cat/pn sort kept.
    let root2 = masters_test_root("depclean-order-flat");
    for n in ["ccc", "aaa", "bbb"] {
        let d = root2.join("var/db/pkg/dev-libs").join(format!("{n}-1.0"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(d.join("SLOT"), "0\n").unwrap();
    }
    let flat = depclean_cleanlist(&root2, &[], &[], &[], true, &[], &[]);
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
    // A 2-node cycle: `hi` DEPENDs `lo` (buildtime, priority -4) and
    // `lo` RDEPENDs `hi` (runtime, -2). No true root. Real portage's
    // `ignore_priority_range` scan tries -4 first: only `lo` has all
    // its remaining incoming edges <= -4 (the single edge hi->lo),
    // so `lo` is popped; `hi` then falls out as a plain root.
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

    // Mutual RDEPEND (both edges -2): -4/-3 yield nothing, -2 makes
    // both eligible -> the cpv-max node (`zz`) is popped first.
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
    // aa, zz, mm are all multi-version; single is not. keeper pins
    // mm-2.0. zz-1.0 pins aa-1.0 (both themselves prunable, so the
    // pin is only an ordering edge, not a keep).
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
    // mm-2.0 kept (keeper needs it), mm-3.0/aa-2.0/zz-2.0 highest,
    // single single-version, keeper protected. Topological order:
    // level 0 ready {zz-1.0, mm-1.0} cpv-desc, then aa-1.0.
    assert_eq!(
        clean,
        vec![
            "dev-libs/zz-1.0".to_string(),
            "dev-libs/mm-1.0".to_string(),
            "dev-libs/aa-1.0".to_string(),
        ]
    );

    // `--prune --verbose`'s reverse-dep display: only mm-2.0 (a kept
    // non-highest version with a real Package parent) -- the highest
    // versions are pulled in only by the internal protected-set seed,
    // which `show_parents` filters out.
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

    // `--prune --nodeps`: NO dependency check -- every non-highest
    // version of every multi-version cp is selected, mm-2.0 included
    // (unlike `--prune`, which keeps it for `keeper`).
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
    // `--prune --nodeps <atom>`: best/rest split over the atom's own
    // match set -- `<dev-libs/mm-3.0` keeps mm-2.0, prunes mm-1.0.
    let nd_atom = prune_nodeps_selection(&root, &["<dev-libs/mm-3.0".to_string()]);
    assert_eq!(
        nd_atom
            .iter()
            .map(|cp| (cp.best_version.clone(), cp.other_versions.clone()))
            .collect::<Vec<_>>(),
        vec![("2.0".to_string(), vec!["1.0".to_string()])]
    );

    // `--prune dev-libs/mm`: only mm's old versions are candidates.
    let narrowed = prune_cleanlist(&root, &["dev-libs/mm".to_string()], &[], &[]);
    assert_eq!(
        narrowed
            .cleanlist
            .iter()
            .map(|p| p.cpv())
            .collect::<Vec<_>>(),
        vec!["dev-libs/mm-1.0".to_string()]
    );

    // A root with only single-version cps -> nothing to prune.
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
