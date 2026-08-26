use super::*;

#[test]
fn plib_registry_round_trips_through_json() {
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/preservepkg-old:0".to_string(),
        (
            "dev-libs/preservepkg-old-1.0".to_string(),
            "5".to_string(),
            vec![
                "/usr/lib/preservedtest/libfoo.so.1".to_string(),
                "/usr/lib/preservedtest/libfoo.so".to_string(),
            ],
        ),
    );
    let tmp = tempdir();
    write_plib_registry(
        &tmp,
        &PlibRegistry {
            entries: entries.clone(),
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("write succeeds");

    let text = std::fs::read_to_string(plib_registry_path(&tmp)).unwrap();
    let parsed = parse_plib_registry(&text).expect("real json.dumps-shaped output parses back");
    assert_eq!(parsed, entries);
}

#[test]
fn read_plib_registry_degrades_gracefully_when_missing_or_corrupt() {
    let tmp = tempdir();
    // No file at all: Portage's `load` degrades to empty.
    assert!(read_plib_registry(&tmp).entries.is_empty());

    std::fs::create_dir_all(plib_registry_path(&tmp).parent().unwrap()).unwrap();
    std::fs::write(plib_registry_path(&tmp), b"not json at all").unwrap();
    assert!(read_plib_registry(&tmp).entries.is_empty());
}

#[test]
fn plib_inode_map_skips_paths_that_no_longer_exist_on_disk() {
    let tmp = tempdir();
    std::fs::create_dir_all(tmp.join("usr/lib")).unwrap();
    std::fs::write(tmp.join("usr/lib/real.so"), b"x").unwrap();

    let mut preserved = BTreeMap::new();
    preserved.insert(
        "dev-libs/foo-1.0".to_string(),
        vec![
            "/usr/lib/real.so".to_string(),
            "/usr/lib/gone.so".to_string(),
        ],
    );
    let map = plib_inode_map(&tmp, &preserved);
    assert_eq!(
        map.len(),
        1,
        "only the still-existing path gets an inode entry"
    );
    let meta = std::fs::symlink_metadata(tmp.join("usr/lib/real.so")).unwrap();
    assert_eq!(
        map.get(&(meta.dev(), meta.ino())),
        Some(&vec![(
            "dev-libs/foo-1.0".to_string(),
            "/usr/lib/real.so".to_string()
        )])
    );
}

/// A non-empty registry serializes byte-identical to Portage's `json.dumps`
/// with `ensure_ascii=False`, tab indent and sorted keys.
/// Portage: `PreservedLibsRegistry.py`, `store`.
#[test]
fn plib_registry_serialization_is_byte_identical_to_python_json_dumps() {
    let tmp = tempdir();
    let mut entries = BTreeMap::new();
    entries.insert(
        "b:0".to_string(),
        ("x-1.0".to_string(), "1".to_string(), Vec::new()),
    );
    entries.insert(
        "a:0".to_string(),
        (
            "y-2.0".to_string(),
            "3".to_string(),
            vec!["/usr/lib/lib\u{e9}.so.1".to_string(), "a\"b\\c".to_string()],
        ),
    );
    write_plib_registry(
        &tmp,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("write succeeds");
    let bytes = std::fs::read(plib_registry_path(&tmp)).unwrap();
    let expected = "{\n\t\"a:0\": [\n\t\t\"y-2.0\",\n\t\t\"3\",\n\t\t[\n\t\t\t\"/usr/lib/lib\u{e9}.so.1\",\n\t\t\t\"a\\\"b\\\\c\"\n\t\t]\n\t],\n\t\"b:0\": [\n\t\t\"x-1.0\",\n\t\t\"1\",\n\t\t[]\n\t]\n}";
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        expected,
        "registry bytes must match Python json.dumps exactly (no trailing newline)"
    );
}

/// A package owning no files still unregisters its matching stale entry;
/// with no entry at all, nothing is written.
/// Portage: `PreservedLibsRegistry.py`, `store`.
#[test]
fn preserve_libs_on_unmerge_with_empty_contents_still_unregisters() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("COUNTER"), "5\n").unwrap();
    // The stale path exists, so load-time pruning keeps the entry.
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::write(root.join("usr/lib/stale.so"), b"fake elf").unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/foo:0".to_string(),
        (
            "dev-libs/foo-1.0".to_string(),
            "5".to_string(),
            vec!["/usr/lib/stale.so".to_string()],
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

    let preserved = preserve_libs_on_unmerge(PreserveLibsOnUnmergeArgs {
        root: &root,
        category: "dev-libs",
        pn: "foo",
        pf: "foo-1.0",
        slot: "0",
        contents_text: "",
        is_replacement: false,
    })
    .unwrap();
    assert!(preserved.is_empty());
    assert_eq!(
        std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
        "{}",
        "the matching stale entry must be unregistered even with empty CONTENTS"
    );

    let tmp2 = tempdir();
    let preserved = preserve_libs_on_unmerge(PreserveLibsOnUnmergeArgs {
        root: &tmp2,
        category: "dev-libs",
        pn: "foo",
        pf: "foo-1.0",
        slot: "0",
        contents_text: "",
        is_replacement: false,
    })
    .unwrap();
    assert!(preserved.is_empty());
    assert!(!plib_registry_path(&tmp2).exists());
}

/// A path in the preserved-libs registry is excluded from collision-protect
/// and handed over; the emptied record and the old `CONTENTS` claim go with it.
/// Portage: `vartree.py`, `_collision_protect`.
#[test]
fn preserve_libs_registry_entry_excludes_the_collision_and_hands_ownership_over() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("preservepkg-old"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("preservepkg-old merges cleanly");

    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/preservepkg-old:0".to_string(),
        (
            "dev-libs/preservepkg-old-1.0".to_string(),
            "0".to_string(),
            vec!["/usr/lib/preservedtest/libfoo.so.1".to_string()],
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

    let options = MergeOptions {
        collision_protect: true,
        ..MergeOptions::default()
    };
    let status = run_merge(
        &collision_fixture("preservepkg-new"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect("a preserved-lib collision is excluded, not aborted");
    assert_eq!(status, 0);

    assert_eq!(
        std::fs::read_to_string(root.join("usr/lib/preservedtest/libfoo.so.1")).unwrap(),
        "new library content\n"
    );

    let old_contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/preservepkg-old-1.0/CONTENTS"))
            .unwrap();
    assert!(!old_contents.contains("/usr/lib/preservedtest/libfoo.so.1"));

    let registry_text = std::fs::read_to_string(plib_registry_path(&root)).unwrap();
    let registry = parse_plib_registry(&registry_text).unwrap();
    assert!(registry.is_empty());
}
