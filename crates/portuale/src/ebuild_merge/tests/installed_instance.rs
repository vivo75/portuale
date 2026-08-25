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
fn read_installed_slot_returns_0_for_a_slot_less_vdb_entry() {
    // Portage's `aux_get` maps a missing/empty SLOT to "0".
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
    // Through the seam a multi-line SLOT collapses, and the collapsed
    // value is itself invalid, so the seam maps it on to `"0"`.
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
