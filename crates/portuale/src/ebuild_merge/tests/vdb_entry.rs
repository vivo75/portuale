use super::*;

/// Backlog #183: interrupting a merge while its vdb entry is still
/// being written leaves `-MERGING-<pf>` and nothing at `<pf>` -- real
/// `dbtmpdir` visibility from its creation (before `pkg_preinst` and
/// the `${ROOT}` copy) to the `_movefile` into place after the old
/// instances are unmerged. Drives the staged writer directly:
/// `create` + `populate` + `write contents` is the interrupt point
/// (no `publish` ever runs), pinning that every pre-`publish` stage
/// leaves the live entry untouched and the temporary invisible.
#[test]
fn interrupted_vdb_write_leaves_merging_and_no_final_entry() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let build_info = tmp.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    std::fs::write(build_info.join("SLOT"), "0\n").unwrap();

    create_vdb_tmp(&root, "dev-libs", "victimpkg-1.0").unwrap();
    // A kill here (during `pkg_preinst` / the `${ROOT}` copy) leaves
    // an empty temporary -- real's own C4 shape, where the kill lands
    // in the `pkg_preinst` sleep before any info file is copied.
    let tmp_dir = root.join("var/db/pkg/dev-libs/-MERGING-victimpkg-1.0");
    assert!(tmp_dir.is_dir());
    assert!(!root.join("var/db/pkg/dev-libs/victimpkg-1.0").exists());

    populate_vdb_tmp(
        &root,
        "dev-libs",
        "victimpkg-1.0",
        &build_info,
        "0",
        "testrepo",
    )
    .unwrap();
    write_vdb_tmp_contents(
        &root,
        "dev-libs",
        "victimpkg-1.0",
        "obj /usr/bin/victim abc 123\n",
    )
    .unwrap();
    // ... and a kill here leaves the fully-populated temporary, still
    // with nothing live and nothing enumerated.
    assert!(!root.join("var/db/pkg/dev-libs/victimpkg-1.0").exists());
    assert!(portage_repo::all_installed_packages(&root).is_empty());

    publish_vdb_tmp(&root, "dev-libs", "victimpkg-1.0").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/victimpkg-1.0/CONTENTS")).unwrap(),
        "obj /usr/bin/victim abc 123\n"
    );
    assert!(!tmp_dir.exists(), "no temporary survives a publish");
}
