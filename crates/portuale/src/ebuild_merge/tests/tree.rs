use super::*;

#[test]
fn format_contents_line_matches_portage_dblink_format() {
    assert_eq!(
        format_contents_line("dir", "/usr/share/x", None, None, None),
        "dir /usr/share/x\n"
    );
    assert_eq!(
        format_contents_line("obj", "/usr/share/x/f", Some("abc123"), None, Some(100)),
        "obj /usr/share/x/f abc123 100\n"
    );
    assert_eq!(
        format_contents_line("sym", "/usr/lib/x.so", None, Some("x.so.1"), Some(100)),
        "sym /usr/lib/x.so -> x.so.1 100\n"
    );
}

#[test]
fn repository_name_for_finds_the_nearest_ancestor_repo_name() {
    let tmp = tempdir();
    let repo = tmp.join("myrepo");
    let pkg_dir = repo.join("dev-libs/foo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
    assert_eq!(repository_name_for(&pkg_dir), Some("myrepo".to_string()));
}

#[test]
fn repository_name_for_is_none_when_no_ancestor_has_one() {
    let tmp = tempdir();
    let pkg_dir = tmp.join("dev-libs/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(repository_name_for(&pkg_dir), None);
}

#[test]
fn merge_tree_copies_files_dirs_and_symlinks_and_writes_matching_contents() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
    std::fs::write(d.join("usr/share/x/hello.txt"), b"hello").unwrap();
    std::os::unix::fs::symlink("hello.txt", d.join("usr/share/x/link.txt")).unwrap();
    std::fs::create_dir_all(&root).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert!(root.join("usr/share/x/hello.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/x/hello.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        std::fs::read_link(root.join("usr/share/x/link.txt")).unwrap(),
        PathBuf::from("hello.txt")
    );

    assert!(contents.contains("dir /usr\n"));
    assert!(contents.contains("dir /usr/share\n"));
    assert!(contents.contains("dir /usr/share/x\n"));
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/x/hello.txt "))
    );
    assert!(contents.contains("sym /usr/share/x/link.txt -> hello.txt"));
}

#[test]
fn merge_tree_preserves_ownership_from_the_source_when_root() {
    // The destination takes the source's uid/gid (Portage's movefile/mergeme).
    // Root-only: skipped otherwise (run under `sudo cargo test ...` to exercise).
    // SAFETY: `geteuid` takes no arguments and returns a plain int.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipping merge_tree_preserves_ownership_from_the_source_when_root: not root");
        return;
    }
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("usr/share/x")).unwrap();
    std::fs::write(d.join("usr/share/x/hello.txt"), b"hello").unwrap();
    std::os::unix::fs::symlink("hello.txt", d.join("usr/share/x/link.txt")).unwrap();
    std::fs::create_dir_all(&root).unwrap();

    // A uid/gid that's neither 0 nor this process's own.
    let (uid, gid) = (65534u32, 65534u32);
    for p in [
        d.join("usr"),
        d.join("usr/share"),
        d.join("usr/share/x"),
        d.join("usr/share/x/hello.txt"),
    ] {
        raw_chown(&p, uid, gid, false);
    }
    raw_chown(&d.join("usr/share/x/link.txt"), uid, gid, true);

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
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    for p in [
        root.join("usr"),
        root.join("usr/share"),
        root.join("usr/share/x"),
        root.join("usr/share/x/hello.txt"),
        root.join("usr/share/x/link.txt"),
    ] {
        let meta = std::fs::symlink_metadata(&p).unwrap();
        assert_eq!(meta.uid(), uid, "{}: uid", p.display());
        assert_eq!(meta.gid(), gid, "{}: gid", p.display());
    }

    // An existing directory keeps its ownership (Portage's "kept as-is" branch).
    raw_chown(&d.join("usr/share/x"), 0, 0, false);
    let mut cfgfiledict2 = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict2,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");
    let meta = std::fs::symlink_metadata(root.join("usr/share/x")).unwrap();
    assert_eq!(meta.uid(), uid, "existing dir must keep its ownership");
    assert_eq!(meta.gid(), gid, "existing dir must keep its ownership");
}

#[test]
fn merge_tree_does_not_protect_a_brand_new_file() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(&root).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "new content"
    );
    assert!(!root.join("etc/._cfg0000_foo.conf").exists());
}

#[test]
fn merge_tree_leaves_an_unchanged_protected_file_alone() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"same content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"same content").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "same content"
    );
    assert!(!root.join("etc/._cfg0000_foo.conf").exists());
}

#[test]
fn merge_tree_protects_a_changed_file_under_a_protected_path() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "user's own edits"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
        "new content"
    );
    // CONTENTS records the logical path with the new content's MD5.
    let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} ")))
    );
    assert_eq!(cfgfiledict.get("/etc/foo.conf"), Some(&new_md5));
}

#[test]
fn merge_tree_protect_if_modified_applies_directly_when_dest_still_matches_the_installed_instance()
{
    // The destination still matches the installed instance's CONTENTS, so it counts
    // as unmodified and the new content applies directly (Portage's protect_if_modified).
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"old content").unwrap();
    let old_md5 = md5_hex(&root.join("etc/foo.conf")).unwrap();

    let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        format!("obj /etc/foo.conf {old_md5} 100\n"),
    )
    .unwrap();
    std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: Some("foopkg-1.0"),
        protect_if_modified: true,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "new content",
        "unmodified-since-installed content is overwritten directly, not protected"
    );
    assert!(!root.join("etc/._cfg0000_foo.conf").exists());
    let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with(&format!("obj /etc/foo.conf {new_md5} ")))
    );
}

#[test]
fn merge_tree_still_protects_a_locally_modified_file_despite_protect_if_modified() {
    // The destination no longer matches the recorded CONTENTS, so it counts as
    // locally modified and protection still applies.
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"the admin's own local edits").unwrap();

    let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        "obj /etc/foo.conf deadbeefdeadbeefdeadbeefdeadbeef 100\n",
    )
    .unwrap();
    std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: Some("foopkg-1.0"),
        protect_if_modified: true,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "the admin's own local edits",
        "locally-modified content is still protected"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
        "new content"
    );
}

#[test]
fn merge_tree_force_protects_a_path_the_installed_instance_recorded_but_the_admin_deleted() {
    // The admin deleted a recorded path: divert to a fresh `._cfgNNNN_` sibling
    // rather than re-creating it (Portage's `force = True`).
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();

    let vdb_dir = root.join("var/db/pkg/dev-libs/foopkg-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        "obj /etc/foo.conf deadbeefdeadbeefdeadbeefdeadbeef 100\n",
    )
    .unwrap();
    std::fs::write(vdb_dir.join("COUNTER"), "5").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: Some("foopkg-1.0"),
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert!(
        !root.join("etc/foo.conf").exists(),
        "the admin's own deletion is respected -- nothing is silently re-created"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
        "new content"
    );
}

#[test]
fn merge_tree_remembers_an_already_offered_update_and_leaves_the_live_file_untouched() {
    // Already offered and remembered in cfgfiledict: skip the write entirely, with
    // no second `._cfg0001_` (Portage's confmem).
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("first merge_tree succeeds");
    assert!(root.join("etc/._cfg0000_foo.conf").exists());

    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("second merge_tree succeeds");
    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "user's own edits",
        "the admin's own live edits must survive a re-offered, already-remembered update"
    );
    assert!(!root.join("etc/._cfg0001_foo.conf").exists());

    // CONTENTS still records the new content's MD5 despite the skipped write.
    let new_md5 = md5_hex(&d.join("etc/foo.conf")).unwrap();
    assert!(
        contents.contains(&format!("obj /etc/foo.conf {new_md5}")),
        "{contents}"
    );
}

#[test]
fn merge_tree_noconfmem_reprotects_an_already_offered_update() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/foo.conf"), b"new content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("first merge_tree succeeds");
    assert!(root.join("etc/._cfg0000_foo.conf").exists());

    // With noconfmem every re-merge re-protects regardless of cfgfiledict memory,
    // reusing `._cfg0000_` when its content already matches.
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: true,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("second merge_tree succeeds");
    assert!(!root.join("etc/._cfg0001_foo.conf").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
        "new content"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "user's own edits"
    );
}

#[test]
fn merge_tree_protects_a_changed_symlink_under_a_protected_path() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::os::unix::fs::symlink("new-target", d.join("etc/link.conf")).unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::os::unix::fs::symlink("users-own-target", root.join("etc/link.conf")).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_link(root.join("etc/link.conf")).unwrap(),
        PathBuf::from("users-own-target")
    );
    assert_eq!(
        std::fs::read_link(root.join("etc/._cfg0000_link.conf")).unwrap(),
        PathBuf::from("new-target")
    );
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("sym /etc/link.conf -> new-target"))
    );
}

#[test]
fn merge_tree_does_not_protect_an_unchanged_symlink() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::os::unix::fs::symlink("same-target", d.join("etc/link.conf")).unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::os::unix::fs::symlink("same-target", root.join("etc/link.conf")).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_link(root.join("etc/link.conf")).unwrap(),
        PathBuf::from("same-target")
    );
    assert!(!root.join("etc/._cfg0000_link.conf").exists());
}

#[test]
fn merge_tree_protects_a_symlink_source_replacing_a_regular_file_dest() {
    // Type-independent comparison: a symlink source over a regular-file dest is
    // still protected (Portage's `dblink._protect`).
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::os::unix::fs::symlink("new-target", d.join("etc/thing.conf")).unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/thing.conf"), b"the admin's own regular file").unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_to_string(root.join("etc/thing.conf")).unwrap(),
        "the admin's own regular file"
    );
    assert_eq!(
        std::fs::read_link(root.join("etc/._cfg0000_thing.conf")).unwrap(),
        PathBuf::from("new-target")
    );
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("sym /etc/thing.conf -> new-target"))
    );
}

#[test]
fn merge_tree_protects_a_regular_file_source_replacing_a_symlink_dest() {
    // Mirror image: a regular-file source landing on a symlink-occupied path.
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("etc")).unwrap();
    std::fs::write(d.join("etc/thing.conf"), b"new regular content").unwrap();
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::os::unix::fs::symlink("admins-own-target", root.join("etc/thing.conf")).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    let contents = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "dev-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_link(root.join("etc/thing.conf")).unwrap(),
        PathBuf::from("admins-own-target")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_thing.conf")).unwrap(),
        "new regular content"
    );
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("obj /etc/thing.conf"))
    );
}
