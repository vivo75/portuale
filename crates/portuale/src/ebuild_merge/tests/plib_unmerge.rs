use super::*;

/// Fix round 1 for review item 3 (backlog #178): real
/// `unmerge_no_replacement = unmerge and not
/// unmerge_with_replacement` (`dblink._prune_plib_registry`) -- in
/// replacement mode a consumer owned by the old instance still
/// keeps a preserved lib alive (it survives through the replacing
/// package). Pre-fix, portuale always passed `true`, eliminating
/// those consumers and wrongly pruning the lib.
#[test]
fn prune_keeps_a_preserved_lib_whose_only_consumer_is_old_owned_in_replacement_mode() {
    let tmp = tempdir();
    let (root, being) = seed_old_owned_consumer_root(&tmp);

    let unneeded = find_unused_preserved_libs(
        &root,
        true,
        &|p| being.contains(p),
        &[],
        &BTreeMap::new(),
        &[],
    );
    assert_eq!(
        unneeded,
        BTreeMap::from([(
            "dev-libs/provider-1.0".to_string(),
            BTreeSet::from(["/usr/lib/keepA.so".to_string()]),
        )]),
        "standalone mode eliminates the about-to-be-unmerged consumer"
    );

    let unneeded = find_unused_preserved_libs(
        &root,
        false,
        &|p| being.contains(p),
        &[],
        &BTreeMap::new(),
        &[],
    );
    assert!(
        unneeded.is_empty(),
        "replacement mode keeps the old-owned consumer, so the lib is needed: {unneeded:?}"
    );
}

/// Fix round 1 for review item 4, `exclude_pkgs` half (backlog
/// #178): real `LinkageMap.rebuild(exclude_pkgs=...)` drops the
/// unmerged instance's `NEEDED.ELF.2` lines, so a consumer only the
/// old instance knows about cannot keep a preserved lib alive past
/// the replacement. Without the exclusion the stale consumer data
/// "would only serve to corrupt the `LinkageMap`".
#[test]
fn prune_excludes_the_replaced_instance_linkage_data() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/staleD.so"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/oldtool"), b"fake elf").unwrap();
    // The library is indexed (its provider is still installed); its
    // only consumer is known solely to the instance being replaced.
    let vdb_prov = root.join("var/db/pkg/dev-libs/provider-1.0");
    std::fs::create_dir_all(&vdb_prov).unwrap();
    std::fs::write(vdb_prov.join("CONTENTS"), "obj /usr/lib/staleD.so def 1\n").unwrap();
    std::fs::write(
        vdb_prov.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/staleD.so;libstale.so.1;;\n",
    )
    .unwrap();
    let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
    std::fs::create_dir_all(&vdb_old).unwrap();
    std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/bin/oldtool abc 1\n").unwrap();
    std::fs::write(
        vdb_old.join("NEEDED.ELF.2"),
        "X86_64;/usr/bin/oldtool;;;libstale.so.1\n",
    )
    .unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/provider:0".to_string(),
        (
            "dev-libs/provider-1.0".to_string(),
            "4".to_string(),
            vec!["/usr/lib/staleD.so".to_string()],
        ),
    );
    write_plib_registry(
        &root,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("seeding the registry succeeds");

    let unneeded = find_unused_preserved_libs(
        &root,
        false,
        &|_| false,
        &["dev-libs/oldapp-1.0".to_string()],
        &BTreeMap::new(),
        &[],
    );
    assert_eq!(
        unneeded,
        BTreeMap::from([(
            "dev-libs/provider-1.0".to_string(),
            BTreeSet::from(["/usr/lib/staleD.so".to_string()]),
        )]),
        "with the replaced instance excluded nothing consumes the lib"
    );

    let unneeded = find_unused_preserved_libs(&root, false, &|_| false, &[], &BTreeMap::new(), &[]);
    assert!(
        unneeded.is_empty(),
        "without the exclusion the stale consumer keeps it (the corruption real excludes): {unneeded:?}"
    );
}

/// Fix round 1 for review item 4, `preserve_paths` half (backlog
/// #178): real `LinkageMap.rebuild(preserve_paths=...)` scans the
/// merge-side just-preserved set, so a preserved library whose only
/// linkmap consumer is a just-preserved file -- indexed in no
/// `NEEDED.ELF.2` anywhere -- is kept, not deleted as unneeded.
/// Grounded in real `LinkageMap.rebuild` (the `preserve_paths`
/// parameter): without the feed the consumer is invisible to the
/// prune.
#[test]
fn prune_sees_just_preserved_files_missing_from_every_needed_elf2() {
    let tmp = tempdir();
    let root = tmp.join("root");
    compile_linked_pair(&tmp, &root);
    // `libA` is indexed (hand-written provider entry, the way any
    // still-installed owner's `NEEDED.ELF.2` would); `libB` appears
    // in no `NEEDED.ELF.2` -- only the `preserve_paths` feed (real
    // `LinkageMap.rebuild(preserve_paths=...)`) can show it to the
    // prune.
    let vdb_prov = root.join("var/db/pkg/dev-libs/provider-1.0");
    std::fs::create_dir_all(&vdb_prov).unwrap();
    std::fs::write(
        vdb_prov.join("CONTENTS"),
        "obj /usr/lib/libA.so.1.0.0 def 1\n",
    )
    .unwrap();
    std::fs::write(
        vdb_prov.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/libA.so.1.0.0;libA.so.1;;\n",
    )
    .unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/provider:0".to_string(),
        (
            "dev-libs/provider-1.0".to_string(),
            "4".to_string(),
            vec!["/usr/lib/libA.so.1.0.0".to_string()],
        ),
    );
    write_plib_registry(
        &root,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("seeding the registry succeeds");

    let unneeded = find_unused_preserved_libs(&root, false, &|_| false, &[], &BTreeMap::new(), &[]);
    assert_eq!(
        unneeded,
        BTreeMap::from([(
            "dev-libs/provider-1.0".to_string(),
            BTreeSet::from(["/usr/lib/libA.so.1.0.0".to_string()]),
        )]),
        "without the just-preserved feed the consumer is invisible"
    );

    let feed: BTreeMap<String, Vec<String>> = BTreeMap::from([(
        "dev-libs/newapp-2.0".to_string(),
        vec!["/usr/lib/libB.so.1.0.0".to_string()],
    )]);
    let unneeded = find_unused_preserved_libs(&root, false, &|_| false, &[], &feed, &[]);
    assert!(
        unneeded.is_empty(),
        "the just-preserved consumer keeps the lib alive: {unneeded:?}"
    );
    assert!(
        root.join("usr/lib/libA.so.1.0.0").is_file(),
        "nothing was deleted by the read-only probe above"
    );
}

/// Real `_find_unneeded_preserved_nodes` cycle handling (bug 652382):
/// two preserved libraries that only consume each other, with nothing
/// outside the pair consuming either, are both unneeded.
#[test]
fn find_unneeded_preserved_drops_a_self_contained_cycle() {
    let tmp = tempdir();
    let root = tmp.join("root");
    for p in ["usr/lib/liba.so.1", "usr/lib/libb.so.1"] {
        std::fs::create_dir_all(root.join("usr/lib")).unwrap();
        std::fs::write(root.join(p), b"x").unwrap();
    }
    let preserved: BTreeSet<String> = ["/usr/lib/liba.so.1", "/usr/lib/libb.so.1"]
        .into_iter()
        .map(String::from)
        .collect();
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    edges.insert(
        "/usr/lib/liba.so.1".into(),
        BTreeSet::from(["/usr/lib/libb.so.1".to_string()]),
    );
    edges.insert(
        "/usr/lib/libb.so.1".into(),
        BTreeSet::from(["/usr/lib/liba.so.1".to_string()]),
    );
    let unneeded = crate::needed_elf::find_unneeded_preserved(&root, &edges, &preserved);
    assert_eq!(unneeded, preserved);

    // But an outside (non-preserved) consumer of one keeps that one --
    // and, transitively, the other it depends on.
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/bin/app"), b"x").unwrap();
    edges
        .get_mut("/usr/lib/liba.so.1")
        .unwrap()
        .insert("/usr/bin/app".to_string());
    let unneeded = crate::needed_elf::find_unneeded_preserved(&root, &edges, &preserved);
    assert!(unneeded.is_empty(), "{unneeded:?}");
}
