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
