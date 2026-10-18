use super::*;

/// Excluded registry keys drop out, but the same key in the just-preserved
/// feed survives: on a same-version reinstall the two are the same string.
/// Expected from Portage 3.0.82.2 (`LinkageMapELF.rebuild` adds
/// `preserve_paths` unconditionally, `exclude_pkgs` registry-only), S0 cell P1.
/// Portage: `LinkageMapELF.py`, `LinkageMapELF.rebuild`.
#[test]
fn preserved_scan_input_excludes_the_registry_but_keeps_the_just_preserved_feed() {
    let preserved: BTreeMap<String, Vec<String>> = BTreeMap::from([
        (
            "dev-libs/oldapp-1.0".to_string(),
            vec!["/usr/lib/stale.so".to_string()],
        ),
        (
            "dev-libs/provider-1.0".to_string(),
            vec!["/usr/lib/keep.so".to_string()],
        ),
    ]);
    let feed: BTreeMap<String, Vec<String>> = BTreeMap::from([
        (
            "dev-libs/oldapp-1.0".to_string(),
            vec!["/usr/lib/just.so".to_string()],
        ),
        (
            "dev-libs/other-1.0".to_string(),
            vec!["/usr/lib/extra.so".to_string()],
        ),
    ]);
    let got = preserved_scan_input(&preserved, &["dev-libs/oldapp-1.0".to_string()], &feed);
    assert_eq!(
        got,
        BTreeMap::from([
            (
                "dev-libs/provider-1.0".to_string(),
                vec!["/usr/lib/keep.so".to_string()],
            ),
            (
                "dev-libs/oldapp-1.0".to_string(),
                vec!["/usr/lib/just.so".to_string()],
            ),
            (
                "dev-libs/other-1.0".to_string(),
                vec!["/usr/lib/extra.so".to_string()],
            ),
        ]),
        "the excluded registry entry drops, the same-key feed entry stays"
    );
    assert_eq!(
        preserved.len(),
        2,
        "the registry map itself is never mutated"
    );
}

/// Empty-`paths` unregisters only when the stored `cpv` and `counter` still
/// match; a different package or a stale counter survives untouched.
/// Portage: `PreservedLibsRegistry.py`, `unregister`.
#[test]
fn register_preserved_libs_unregister_only_matches_the_same_cpv_and_counter() {
    let mut registry = PlibRegistry {
        entries: BTreeMap::new(),
        orig_entries: BTreeMap::new(),
    };
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &["/usr/lib/libfoo.so.1".to_string()],
    });
    assert!(registry.entries.contains_key("dev-libs/foo:0"));

    // Wrong counter: Portage's `unregister` leaves the entry alone.
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "9",
        paths: &[],
    });
    assert!(
        registry.entries.contains_key("dev-libs/foo:0"),
        "a stale counter must not unregister someone else's live entry"
    );

    // Wrong cpv: the same "leave it alone" rule.
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-2.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &[],
    });
    assert!(
        registry.entries.contains_key("dev-libs/foo:0"),
        "a different cpv must not unregister someone else's live entry"
    );

    // Matching cpv and counter: Portage's `unregister` removes it.
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &[],
    });
    assert!(!registry.entries.contains_key("dev-libs/foo:0"));
}

/// Non-empty `paths` unconditionally overwrites whatever the key held.
/// Portage: `PreservedLibsRegistry.py`, `register`.
#[test]
fn register_preserved_libs_with_paths_unconditionally_overwrites() {
    let mut registry = PlibRegistry {
        entries: BTreeMap::new(),
        orig_entries: BTreeMap::new(),
    };
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &["/usr/lib/libfoo.so.1".to_string()],
    });
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-2.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "6",
        paths: &["/usr/lib/libfoo.so.2".to_string()],
    });

    let (cpv, counter, paths) = registry.entries.get("dev-libs/foo:0").unwrap();
    assert_eq!(cpv, "dev-libs/foo-2.0");
    assert_eq!(counter, "6");
    assert_eq!(paths, &["/usr/lib/libfoo.so.2".to_string()]);
}

/// Merge-side preserve sees the replacing package's `-MERGING-` NEEDED feed,
/// so a lib owned by the replaced instance and consumed only by the
/// replacement is preserved. Portage: `dblink.py`, `treewalk`.
#[test]
fn merge_side_preserve_sees_the_replacing_packages_merging_needed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/libold.so.1"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/newtool"), b"fake elf").unwrap();
    let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
    std::fs::create_dir_all(&vdb_old).unwrap();
    std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/lib/libold.so.1 def 1\n").unwrap();
    std::fs::write(
        vdb_old.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/libold.so.1;libold.so.1;;\n",
    )
    .unwrap();
    std::fs::write(vdb_old.join("SLOT"), "0\n").unwrap();
    std::fs::write(vdb_old.join("COUNTER"), "7\n").unwrap();
    // The replacing package is still `-MERGING-`, so only the explicit
    // include feed can show its consumer to the preserve computation.
    let vdb_new = root.join("var/db/pkg/dev-libs/-MERGING-oldapp-2.0");
    std::fs::create_dir_all(&vdb_new).unwrap();
    std::fs::write(
        vdb_new.join("NEEDED.ELF.2"),
        "X86_64;/usr/bin/newtool;;;libold.so.1\n",
    )
    .unwrap();
    let new_image_paths: BTreeSet<String> = ["/usr/bin/newtool".to_string()].into_iter().collect();

    let (paths, _) = find_preserve_paths_for_merge(FindPreservePathsForMergeArgs {
        root: &root,
        category: "dev-libs",
        package: "oldapp",
        new_pf: "oldapp-2.0",
        main_slot: "0",
        new_image_paths: &new_image_paths,
        preserve_libs: true,
    })
    .expect("a same-slot instance owning files preserves");
    assert!(
        paths.contains("/usr/lib/libold.so.1"),
        "the old-owned lib consumed only by the replacing package must be preserved: {paths:?}"
    );
}

/// Removing a path from `CONTENTS` also drops its `NEEDED.ELF.2` entry;
/// entries for still-owned paths survive untouched.
/// Portage: `vartree.py`, `removeFromContents`.
#[test]
fn remove_from_contents_prunes_the_matching_needed_elf2_entry_too() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        "obj /usr/lib/a.so abc123 100\nobj /usr/lib/b.so def456 100\n",
    )
    .unwrap();
    std::fs::write(
        vdb_dir.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/a.so;liba.so.1;;\nX86_64;/usr/lib/b.so;libb.so.1;;\n",
    )
    .unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/a.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
    assert!(!contents.contains("/usr/lib/a.so"));
    assert!(contents.contains("/usr/lib/b.so"));

    let needed = std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap();
    assert!(
        !needed.contains("/usr/lib/a.so"),
        "the entry for the removed path must be pruned: {needed}"
    );
    assert!(
        needed.contains("/usr/lib/b.so"),
        "the entry for the still-owned path must survive: {needed}"
    );
}

/// When the package never had a `NEEDED.ELF.2`, removing `CONTENTS`
/// entries writes none.
/// Portage: `vartree.py`, `removeFromContents`.
#[test]
fn remove_from_contents_does_not_create_a_needed_elf2_file_that_never_existed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/a.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    assert!(!vdb_dir.join("NEEDED.ELF.2").exists());
}

/// When nothing was actually removed, `NEEDED.ELF.2` stays byte-identical.
/// Portage: `vartree.py`, `removeFromContents`.
#[test]
fn remove_from_contents_leaves_needed_elf2_untouched_when_nothing_was_removed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();
    let original_needed = "X86_64;/usr/lib/a.so;liba.so.1;;\n";
    std::fs::write(vdb_dir.join("NEEDED.ELF.2"), original_needed).unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/nonexistent.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap(),
        original_needed
    );
}

fn vdb_with_restrict(root: &Path, pf: &str, restrict: &str, use_flags: &str) {
    let dir = root.join("var/db/pkg/dev-libs").join(pf);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SLOT"), "0\n").unwrap();
    std::fs::write(dir.join("RESTRICT"), format!("{restrict}\n")).unwrap();
    std::fs::write(dir.join("USE"), format!("{use_flags}\n")).unwrap();
}

/// The merge-side gate follows `treewalk`: `FEATURES` first, then the RESTRICT of the
/// replaced same-slot instances (USE-reduced with their recorded USE); the new
/// package's own RESTRICT is never read. Source-derived, not captured:
/// `vartree.py:2053` (features) and `:4541-4545`, `:4560-4564` (instances).
#[test]
fn merge_side_gate_reads_features_and_the_replaced_instances_restrict() {
    let tmp = tempdir();
    let root = tmp.join("root");
    vdb_with_restrict(&root, "oldapp-1.0", "", "");
    assert!(preserve_libs_for_merge(
        true, &root, "dev-libs", "oldapp", "0"
    ));
    assert!(
        !preserve_libs_for_merge(false, &root, "dev-libs", "oldapp", "0"),
        "-preserve-libs in FEATURES turns the merge side off"
    );

    vdb_with_restrict(&root, "oldapp-1.0", "preserve-libs", "");
    assert!(
        !preserve_libs_for_merge(true, &root, "dev-libs", "oldapp", "0"),
        "RESTRICT=preserve-libs on the replaced instance"
    );

    vdb_with_restrict(&root, "oldapp-1.0", "foo? ( preserve-libs )", "");
    assert!(
        preserve_libs_for_merge(true, &root, "dev-libs", "oldapp", "0"),
        "a conditional restrict with the flag off does not restrict"
    );
    vdb_with_restrict(&root, "oldapp-1.0", "foo? ( preserve-libs )", "foo");
    assert!(
        !preserve_libs_for_merge(true, &root, "dev-libs", "oldapp", "0"),
        "the instance's recorded USE reduces its RESTRICT"
    );

    // One restricted instance restricts the others in the slot too; another slot does not.
    vdb_with_restrict(&root, "oldapp-1.0", "", "");
    vdb_with_restrict(&root, "oldapp-1.1", "preserve-libs", "");
    assert!(!preserve_libs_for_merge(
        true, &root, "dev-libs", "oldapp", "0"
    ));

    // A restricted instance in another slot does not count.
    let other = tmp.join("other-root");
    vdb_with_restrict(&other, "oldapp-1.0", "", "");
    vdb_with_restrict(&other, "oldapp-2.0", "preserve-libs", "");
    std::fs::write(other.join("var/db/pkg/dev-libs/oldapp-2.0/SLOT"), "1\n").unwrap();
    assert!(preserve_libs_for_merge(
        true, &other, "dev-libs", "oldapp", "0"
    ));
}

/// With the gate off the merge selects nothing.
#[test]
fn find_preserve_paths_for_merge_selects_nothing_when_the_gate_is_off() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
    std::fs::create_dir_all(&vdb_old).unwrap();
    std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/lib/libold.so.1 def 1\n").unwrap();
    std::fs::write(vdb_old.join("SLOT"), "0\n").unwrap();
    std::fs::write(vdb_old.join("COUNTER"), "7\n").unwrap();
    let image: BTreeSet<String> = BTreeSet::new();
    assert!(
        find_preserve_paths_for_merge(FindPreservePathsForMergeArgs {
            root: &root,
            category: "dev-libs",
            package: "oldapp",
            new_pf: "oldapp-2.0",
            main_slot: "0",
            new_image_paths: &image,
            preserve_libs: false,
        })
        .is_none()
    );
}
