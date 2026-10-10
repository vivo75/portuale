use super::*;

#[test]
fn installed_instance_pf_picks_the_highest_counter_same_slot_version() {
    let tmp = tempdir();
    let root = tmp.join("ROOT");
    for (version, slot, counter) in [("1.0", "0", "3"), ("2.0", "0", "7"), ("3.0", "1", "99")] {
        let vdb_dir = root
            .join("var/db/pkg/dev-libs")
            .join(format!("instpkg-{version}"));
        std::fs::create_dir_all(&vdb_dir).unwrap();
        std::fs::write(vdb_dir.join("SLOT"), format!("{slot}\n")).unwrap();
        std::fs::write(vdb_dir.join("COUNTER"), counter).unwrap();
    }

    assert_eq!(
        installed_instance_pf(&root, "dev-libs", "instpkg", "0"),
        Some("instpkg-2.0".to_string()),
        "the higher-COUNTER same-slot version wins, not the higher version number"
    );
    assert_eq!(
        installed_instance_pf(&root, "dev-libs", "instpkg", "2"),
        None,
        "no installed version at all in this slot"
    );
}

#[test]
fn replacing_versions_is_the_installed_same_slot_version() {
    // #158: real's `REPLACING_VERSIONS` is the version of the
    // installed instance(s) this merge replaces (`vartree.py:4769`),
    // which is exactly the `{pn}-{version}` PF `installed_instance_pf`
    // selects.
    assert_eq!(
        replacing_versions("awk", Some("awk-4")),
        Some("4".to_string())
    );
    assert_eq!(
        replacing_versions("awk", Some("awk-3.1-r1")),
        Some("3.1-r1".to_string())
    );
    assert_eq!(
        replacing_versions("awk", None),
        None,
        "a first-ever install has no replaced version (real's empty string)"
    );
}

#[test]
fn read_installed_slot_returns_0_for_a_slot_less_vdb_entry() {
    // #126 (O5 ruling 2026-09-23): real `aux_get` translates a
    // missing/empty SLOT to "0", so a SLOT-less entry groups with
    // slot-"0" entries instead of being skipped.
    let tmp = tempdir();
    let root = tmp.join("ROOT");
    let vdb_dir = root.join("var/db/pkg/dev-libs/slotlesspkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("COUNTER"), "1").unwrap();

    assert_eq!(
        read_installed_slot(&root, "dev-libs", "slotlesspkg", "1.0"),
        Some("0".to_string())
    );
}

#[test]
fn read_installed_slot_returns_0_for_an_empty_slot_file() {
    let tmp = tempdir();
    let root = tmp.join("ROOT");
    let vdb_dir = root.join("var/db/pkg/dev-libs/emptyslotpkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("SLOT"), "").unwrap();

    assert_eq!(
        read_installed_slot(&root, "dev-libs", "emptyslotpkg", "1.0"),
        Some("0".to_string())
    );
}

#[test]
fn read_installed_slot_collapses_a_multiline_slot_before_the_main_slot_projection() {
    // #126: through the seam a multi-line SLOT collapses
    // (`" ".join(myd.split())`, real `_aux_get`) where the old bare
    // `.trim()` kept the embedded newline.
    // #115 S1: the collapsed value (`"0 1"`) is itself invalid, so
    // the seam translates it on to `"0"` (real `aux_get`'s
    // `_get_slot_re` rule) -- the pre-S1 expectation is kept in git
    // history.
    let tmp = tempdir();
    let root = tmp.join("ROOT");
    let vdb_dir = root.join("var/db/pkg/dev-libs/multilinepkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("SLOT"), "0\n1\n").unwrap();

    assert_eq!(
        read_installed_slot(&root, "dev-libs", "multilinepkg", "1.0"),
        Some("0".to_string())
    );
}

#[test]
fn owned_node_value_pf_reads_an_obj_and_a_sym_entrys_own_value() {
    let tmp = tempdir();
    let root = tmp.join("ROOT");
    let vdb_dir = root.join("var/db/pkg/dev-libs/instpkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        "obj /etc/foo.conf abc123 100\nsym /etc/link -> target 100\ndir /etc\n",
    )
    .unwrap();

    assert_eq!(
        owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/foo.conf"),
        Some(("obj".to_string(), "abc123".to_string()))
    );
    assert_eq!(
        owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/link"),
        Some(("sym".to_string(), "target".to_string()))
    );
    assert_eq!(
        owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc"),
        None,
        "a dir entry has no MD5/target value at all"
    );
    assert_eq!(
        owned_node_value_pf(&root, "dev-libs", "instpkg-1.0", "/etc/nope"),
        None
    );
}

#[test]
fn apply_install_mask_deletes_matched_paths_and_records_the_mask() {
    let tmp = tempdir();
    let image = tmp.join("image");
    let build_info = tmp.join("build-info");
    std::fs::create_dir_all(image.join("usr/share/info")).unwrap();
    std::fs::create_dir_all(image.join("usr/bin")).unwrap();
    std::fs::create_dir_all(&build_info).unwrap();
    std::fs::write(image.join("usr/share/info/foo.info"), b"x").unwrap();
    std::fs::write(image.join("usr/bin/foo"), b"x").unwrap();

    let options = MergeOptions {
        install_mask: "/usr/share/info".to_string(),
        install_mask_prunes_usr_share: true,
        ..MergeOptions::default()
    };
    apply_install_mask(&image, &build_info, &options).unwrap();

    assert!(!image.join("usr/share/info").exists(), "masked dir removed");
    assert!(
        !image.join("usr/share").exists(),
        "emptied usr/share pruned"
    );
    assert!(image.join("usr/bin/foo").is_file(), "unmasked file kept");
    assert_eq!(
        std::fs::read_to_string(build_info.join("INSTALL_MASK"))
            .unwrap()
            .trim(),
        "/usr/share/info"
    );

    // Empty mask: no-op, no INSTALL_MASK file.
    let bi2 = tmp.join("bi2");
    std::fs::create_dir_all(&bi2).unwrap();
    apply_install_mask(&image, &bi2, &MergeOptions::default()).unwrap();
    assert!(!bi2.join("INSTALL_MASK").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}
