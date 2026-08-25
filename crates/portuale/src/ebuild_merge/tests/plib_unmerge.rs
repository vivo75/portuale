use super::*;

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
