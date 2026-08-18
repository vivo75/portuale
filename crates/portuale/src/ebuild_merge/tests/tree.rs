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
