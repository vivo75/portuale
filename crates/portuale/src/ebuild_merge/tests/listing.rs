//! The direct-path merge/unmerge listing (backlog #403): every marker
//! Portage's `dblink` prints on the foreground path, pinned byte-for-byte
//! against the S403 oracle (`LLM/evidence/2026-10-18-403/matrix.md`).
//! Portage: lib/portage/dbapi/vartree.py.
use super::*;

#[test]
fn listing_printer_formats_every_marker_like_portage() {
    let mut listing = MergeListing::new_for_test(true, true);
    listing.collision_line(3);
    listing.merging("app-misc/t348-b-phases-7", std::path::Path::new("/root"));
    listing.merge_dir(std::path::Path::new("/root/usr"), false);
    listing.merge_dir(std::path::Path::new("/root/etc"), true);
    listing.merge_file(
        std::path::Path::new("/root/usr/share/f"),
        MergeFileKind::New,
    );
    listing.merge_file(
        std::path::Path::new("/root/usr/share/same"),
        MergeFileKind::Identical,
    );
    listing.merge_file(
        std::path::Path::new("/root/etc/conf"),
        MergeFileKind::ConfmemRejected,
    );
    listing.merge_file(
        std::path::Path::new("/root/etc/._cfg0000_conf"),
        MergeFileKind::New,
    );
    listing.merge_symlink(std::path::Path::new("/root/usr/bin/l"), "target", true);
    listing.merge_symlink(std::path::Path::new("/root/usr/bin/l2"), "other", false);
    listing.merge_special(std::path::Path::new("/root/dev/fifo"), true);
    listing.merge_special(std::path::Path::new("/root/dev/old"), false);
    listing.safely_unmerging();
    listing.unmerge_replaced("obj", std::path::Path::new("/root/usr/share/f"));
    listing.unmerge_removed("obj", std::path::Path::new("/root/usr/share/gone"));
    listing.unmerge_kept(
        "cfgpro",
        "obj",
        std::path::Path::new("/root/lib/modules/x.ko"),
    );
    listing.unmerge_kept("!empty", "dir", std::path::Path::new("/root/lib/modules"));
    listing.unmerge_kept(
        "!mtime",
        "obj",
        std::path::Path::new("/root/usr/share/changed"),
    );
    listing.unmerge_kept("", "fif", std::path::Path::new("/root/dev/fifo"));
    listing.original_unmerged();
    listing.merged("app-misc/t348-b-phases-7");
    assert_eq!(
        listing.lines(),
        &[
            " \u{1b}[32;01m*\u{1b}[39;49;00m checking 3 files for package collisions",
            ">>> Merging app-misc/t348-b-phases-7 to /root/",
            ">>> /root/usr/",
            "--- /root/etc/",
            ">>> /root/usr/share/f",
            "=== /root/usr/share/same",
            "--- /root/etc/conf",
            ">>> /root/etc/._cfg0000_conf",
            ">>> /root/usr/bin/l -> target",
            "--- /root/usr/bin/l2 -> other",
            ">>> /root/dev/fifo",
            "!!! /root/dev/old",
            ">>> Safely unmerging already-installed instance...",
            "--- replaced obj /root/usr/share/f",
            "<<<          obj /root/usr/share/gone",
            "--- cfgpro   obj /root/lib/modules/x.ko",
            "--- !empty   dir /root/lib/modules",
            "--- !mtime   obj /root/usr/share/changed",
            "---          fif /root/dev/fifo",
            ">>> Original instance of package unmerged safely.",
            ">>> app-misc/t348-b-phases-7 merged.",
        ]
    );
}

#[test]
fn listing_printer_stays_plain_without_color() {
    let mut listing = MergeListing::new_for_test(true, false);
    listing.collision_line(1);
    assert_eq!(
        listing.lines(),
        &[" * checking 1 files for package collisions"]
    );
}

#[test]
fn listing_printer_is_silent_when_disabled() {
    let mut listing = MergeListing::new_for_test(false, true);
    listing.collision_line(3);
    listing.merging("app-misc/x-1.0", std::path::Path::new("/root"));
    listing.merged("app-misc/x-1.0");
    assert_eq!(listing.lines(), &[] as &[String]);
}

/// `merge_tree` with the listing enabled prints the direct-path listing
/// for a small tree: new dirs (`>>>`), kept dirs (`---`), a new file
/// (`>>>`), a byte-identical file (`===`) and a symlink (`>>> … -> …`).
/// Portage: vartree.py, `dblink.mergeme` (the sibling-subtree order is
/// portuale's own sorted traversal, documented in matrix.md §"Order").
#[test]
fn merge_tree_prints_the_direct_path_listing_on_a_small_tree() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("root");
    std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
    std::fs::write(d.join("usr/share/x/new.txt"), b"new").unwrap();
    std::fs::write(d.join("usr/share/x/same.txt"), b"same").unwrap();
    std::os::unix::fs::symlink("new.txt", d.join("usr/share/x/link")).unwrap();
    // `kept` exists on both sides (an empty image dir merging into a
    // live one); `usr`/`usr/share` exist in the root only as the side
    // effect of creating `kept` — still `---`, like Portage.
    std::fs::create_dir_all(d.join("usr/share/kept")).unwrap();
    std::fs::create_dir_all(root.join("usr/share/kept")).unwrap();
    std::fs::create_dir_all(root.join("usr/share/x")).unwrap();
    std::fs::write(root.join("usr/share/x/same.txt"), b"same").unwrap();

    let mut listing = MergeListing::new_for_test(true, false);
    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::from_options(&MergeOptions::default()),
        listing: &mut listing,
    })
    .expect("merge_tree succeeds");

    let r = root.display().to_string();
    assert_eq!(
        listing.lines(),
        &[
            format!("--- {r}/usr/"),
            format!("--- {r}/usr/share/"),
            format!("--- {r}/usr/share/kept/"),
            format!("--- {r}/usr/share/x/"),
            format!(">>> {r}/usr/share/x/link -> new.txt"),
            format!(">>> {r}/usr/share/x/new.txt"),
            format!("=== {r}/usr/share/x/same.txt"),
        ]
    );
}
