use super::*;

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
