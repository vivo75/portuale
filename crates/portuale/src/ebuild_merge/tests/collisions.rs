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

/// A symlink pointing at a directory is never collision-checked, and
/// `merge_tree` lands it at the first `dest.backup.NNNN`, keeping the dir.
/// Portage: `vartree.py`, `mergeme`.
#[test]
fn directory_target_symlink_over_a_real_directory_is_ignored_and_backed_up() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let image = tmp.join("image");
    std::fs::create_dir_all(root.join("lib/firmware/nvidia/ad103")).unwrap();
    std::fs::write(
        root.join("lib/firmware/nvidia/ad103/some.bin"),
        b"installed",
    )
    .unwrap();
    // An unclaimed ordinary collision alongside: must not abort under
    // `protect_owned` alone either.
    std::fs::create_dir_all(root.join("lib/firmware/brcm")).unwrap();
    std::fs::write(root.join("lib/firmware/brcm/stray.txt"), b"stray on disk").unwrap();

    std::fs::create_dir_all(image.join("lib/firmware/nvidia/real")).unwrap();
    std::fs::write(image.join("lib/firmware/nvidia/real/some.bin"), b"new").unwrap();
    std::os::unix::fs::symlink("real", image.join("lib/firmware/nvidia/ad103")).unwrap();
    std::fs::create_dir_all(image.join("lib/firmware/brcm")).unwrap();
    std::fs::write(image.join("lib/firmware/brcm/stray.txt"), b"new stray").unwrap();

    let empty_plib = HashMap::new();
    let empty_blocked = HashSet::new();
    let (collisions, symlink_collisions, _) = find_collisions(FindCollisionsArgs {
        d: &image,
        root: &root,
        category: "sys-kernel",
        package: "linux-firmware",
        slot: "0",
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        plib_inodes: &empty_plib,
        blocked: &empty_blocked,
    })
    .expect("find_collisions succeeds");
    assert!(
        symlink_collisions.is_empty(),
        "a directory-target symlink is not a PMS 13.4 collision: {symlink_collisions:?}"
    );
    // The ordinary stray-file collision is reported...
    assert_eq!(collisions, vec!["/lib/firmware/brcm/stray.txt".to_string()]);
    assert!(
        find_owners(&root, &collisions).is_empty(),
        "the stray file is unclaimed"
    );

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &image,
        root: &root,
        category: "sys-kernel",
        installed_instance_pf: None,
        protect_if_modified: true,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert!(root.join("lib/firmware/nvidia/ad103").is_dir());
    assert_eq!(
        std::fs::read_link(root.join("lib/firmware/nvidia/ad103.backup.0000")).unwrap(),
        PathBuf::from("real")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("lib/firmware/brcm/stray.txt")).unwrap(),
        "new stray"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Every `!atom`/`!!atom` in an already-flat dep list is matched against
/// the installed vdb; non-blocker atoms are ignored.
/// Portage: `vartree.py`, `blocked_installed_packages`.
#[test]
fn blockers_from_flat_deps_matches_only_blocker_atoms_against_the_vdb() {
    let tmp = tempdir();
    let root = tmp.join("root");
    // `normalpkg` carries a sub-slot, so the re-point through the vdb seam
    // must preserve the `split_once('/')` post-processing.
    for (pf, slot) in [("blockedpkg-1.0", "0"), ("normalpkg-2.0", "3/3.1")] {
        let vdb = root.join("var/db/pkg/dev-libs").join(pf);
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("SLOT"), format!("{slot}\n")).unwrap();
    }

    let deps = [
        "!dev-libs/blockedpkg".to_string(),
        "dev-libs/normalpkg".to_string(),
        "!!dev-libs/notinstalled".to_string(),
    ];
    let blocked = blockers_from_flat_deps(&root, &deps);
    assert_eq!(
        blocked,
        HashSet::from([("dev-libs".to_string(), "blockedpkg-1.0".to_string())])
    );

    // A slot-restricted blocker only matches the matching slot.
    assert!(blockers_from_flat_deps(&root, &["!dev-libs/normalpkg:0".to_string()]).is_empty());
    assert_eq!(
        blockers_from_flat_deps(&root, &["!dev-libs/normalpkg:3".to_string()]),
        HashSet::from([("dev-libs".to_string(), "normalpkg-2.0".to_string())])
    );
}
