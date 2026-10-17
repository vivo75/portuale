use super::*;

/// In replacement mode a consumer owned by the old instance still keeps a
/// preserved lib alive through the replacing package.
/// Portage: `dblink.py`, `_prune_plib_registry`.
#[test]
fn prune_keeps_a_preserved_lib_whose_only_consumer_is_old_owned_in_replacement_mode() {
    let tmp = tempdir();
    let (root, being) = seed_old_owned_consumer_root(&tmp);

    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: true,
        being_unmerged: &|p| being.contains(p),
        exclude_cpvs: &[],
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
    });
    assert_eq!(
        unneeded,
        BTreeMap::from([(
            "dev-libs/provider-1.0".to_string(),
            BTreeSet::from(["/usr/lib/keepA.so".to_string()]),
        )]),
        "standalone mode eliminates the about-to-be-unmerged consumer"
    );

    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|p| being.contains(p),
        exclude_cpvs: &[],
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
    });
    assert!(
        unneeded.is_empty(),
        "replacement mode keeps the old-owned consumer, so the lib is needed: {unneeded:?}"
    );
}

/// The replaced instance's `NEEDED.ELF.2` lines are excluded from the prune,
/// so a consumer only it knows cannot keep a preserved lib alive.
/// Portage: `dblink.py`, `_prune_plib_registry`.
#[test]
fn prune_excludes_the_replaced_instance_linkage_data() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/staleD.so"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/oldtool"), b"fake elf").unwrap();
    // The library is indexed; its only consumer is known solely to the
    // instance being replaced.
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

    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|_| false,
        exclude_cpvs: &["dev-libs/oldapp-1.0".to_string()],
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
    });
    assert_eq!(
        unneeded,
        BTreeMap::from([(
            "dev-libs/provider-1.0".to_string(),
            BTreeSet::from(["/usr/lib/staleD.so".to_string()]),
        )]),
        "with the replaced instance excluded nothing consumes the lib"
    );

    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|_| false,
        exclude_cpvs: &[],
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
    });
    assert!(
        unneeded.is_empty(),
        "without the exclusion the stale consumer keeps it (the corruption real excludes): {unneeded:?}"
    );
}

/// Just-preserved files indexed in no `NEEDED.ELF.2` still keep a preserved
/// lib alive through the merge-side feed.
/// Portage: `dblink.py`, `_prune_plib_registry`.
#[test]
fn prune_sees_just_preserved_files_missing_from_every_needed_elf2() {
    let tmp = tempdir();
    let root = tmp.join("root");
    compile_linked_pair(&tmp, &root);
    // `libB` is in no `NEEDED.ELF.2`; only the `preserve_paths` feed can
    // show it to the prune.
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

    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|_| false,
        exclude_cpvs: &[],
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
    });
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
    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|_| false,
        exclude_cpvs: &[],
        replacement_preserved: &feed,
        replacement_needed: &[],
    });
    assert!(
        unneeded.is_empty(),
        "the just-preserved consumer keeps the lib alive: {unneeded:?}"
    );
    assert!(
        root.join("usr/lib/libA.so.1.0.0").is_file(),
        "nothing was deleted by the read-only probe above"
    );
}

/// On a same-version reinstall the replaced and replacing cpvs are the same
/// string, so the just-preserved feed is keyed by an excluded cpv: the
/// exclusion must not drop it, or a registry-preserved lib whose only
/// consumer is the just-preserved lib is wrongly judged unneeded (#355).
/// Expected from Portage 3.0.82.2, S0 cell P1: `libt355base.so.1` kept.
/// Portage: `LinkageMapELF.rebuild` (`LinkageMapELF.py:180`, `preserve_paths`
/// unconditional, `exclude_pkgs` registry-only).
#[test]
fn prune_keeps_a_lib_whose_only_consumer_was_just_preserved_on_a_same_version_reinstall() {
    let tmp = tempdir();
    let root = tmp.join("root");
    compile_linked_pair(&tmp, &root);
    // `libB` is in no `NEEDED.ELF.2`; only the `preserve_paths` feed can
    // show it to the prune.
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

    // Same-version reinstall: the feed owner and the excluded cpv are the
    // same string.
    let feed: BTreeMap<String, Vec<String>> = BTreeMap::from([(
        "dev-libs/newapp-2.0".to_string(),
        vec!["/usr/lib/libB.so.1.0.0".to_string()],
    )]);
    let unneeded = find_unused_preserved_libs(FindUnusedPreservedLibsArgs {
        root: &root,
        unmerge_no_replacement: false,
        being_unmerged: &|_| false,
        exclude_cpvs: &["dev-libs/newapp-2.0".to_string()],
        replacement_preserved: &feed,
        replacement_needed: &[],
    });
    assert!(
        unneeded.is_empty(),
        "the just-preserved consumer keeps the lib alive even when its cpv is excluded: {unneeded:?}"
    );
    assert!(
        root.join("usr/lib/libA.so.1.0.0").is_file(),
        "nothing was deleted by the read-only probe above"
    );
}

/// Two preserved libs that only consume each other are both unneeded; one
/// outside consumer keeps the pair.
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

    // An outside consumer of one keeps that one and, transitively, the other.
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/bin/app"), b"x").unwrap();
    edges
        .get_mut("/usr/lib/liba.so.1")
        .unwrap()
        .insert("/usr/bin/app".to_string());
    let unneeded = crate::needed_elf::find_unneeded_preserved(&root, &edges, &preserved);
    assert!(unneeded.is_empty(), "{unneeded:?}");
}
