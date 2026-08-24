use super::*;

/// A stale `-MERGING-<pf>` entry is invisible to every vdb reader, and the
/// next merge of the same package wipes it and publishes the live entry.
/// Portage: `dblink.py`, `treewalk`.
#[test]
fn stale_merging_entry_is_invisible_and_replaced_by_the_next_merge() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let stale = root.join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("SLOT"), "0\n").unwrap();
    std::fs::write(stale.join("COUNTER"), "41\n").unwrap();
    std::fs::write(
        stale.join("CONTENTS"),
        "obj /usr/share/mergepkg/hello.txt deadbeef 123\n",
    )
    .unwrap();
    std::fs::write(stale.join("NEEDED.ELF.2"), "junk\n").unwrap();
    assert!(!root.join("var/db/pkg/dev-libs/mergepkg-1.0").exists());

    assert!(portage_repo::all_installed_packages(&root).is_empty());
    assert!(portage_repo::installed_versions(&root, "dev-libs", "mergepkg").is_empty());
    assert!(
        find_owners(&root, &["/usr/share/mergepkg/hello.txt".to_string()]).is_empty(),
        "a stale half-written CONTENTS claims nothing"
    );
    assert!(
        crate::needed_elf::read_all_needed_entries(&root).is_empty(),
        "a stale NEEDED.ELF.2 indexes nothing"
    );

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");
    assert_eq!(
        run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None
        )
        .unwrap(),
        0
    );
    let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
    let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt "))
    );
    assert!(
        !contents.contains("deadbeef"),
        "the stale CONTENTS is fully replaced, not merged"
    );
    assert!(!stale.exists(), "the stale temporary is wiped");
    assert_eq!(portage_repo::all_installed_packages(&root).len(), 1);
}
