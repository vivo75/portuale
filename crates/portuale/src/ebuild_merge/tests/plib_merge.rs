use super::*;

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
