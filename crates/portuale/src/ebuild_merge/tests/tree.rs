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

/// #329: the merged file's xattrs follow real `movefile()`. Same
/// device: real renames, so every attribute survives, excluded
/// patterns and `-xattr` included. Cross device: real copies, then
/// `_copyxattr`s only under `FEATURES=xattr`, minus
/// `PORTAGE_XATTR_EXCLUDE` (`movefile.py:326-358`). `_needs_move`
/// (`vartree.py:6376-6379`) re-merges a byte-identical file whose
/// xattrs differ, but only under `FEATURES=xattr`.
#[test]
fn merge_tree_carries_xattrs_like_portage_movefile() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let b = |p: &Path| p.as_os_str().as_bytes().to_vec();
    let get = |p: &Path, a: &[u8]| crate::helpers::xattr_get(&b(p), a).ok();
    let image = |base: &Path| -> PathBuf {
        let d = base.join("D");
        std::fs::create_dir_all(d.join("usr/share/xa")).unwrap();
        let f = d.join("usr/share/xa/f");
        std::fs::write(&f, b"payload").unwrap();
        crate::helpers::xattr_set(&b(&f), b"user.keep", b"k").unwrap();
        crate::helpers::xattr_set(&b(&f), b"user.skip", b"s").unwrap();
        d
    };
    let merge = |d: &Path, root: &Path, policy: &XattrPolicy| {
        std::fs::create_dir_all(root).unwrap();
        merge_tree(MergeTreeArgs {
            d,
            root,
            category: "dev-libs",
            installed_instance_pf: None,
            protect_if_modified: false,
            config_protect: "/etc",
            config_protect_mask: "/etc/env.d",
            noconfmem: false,
            cfgfiledict: &mut BTreeMap::new(),
            xattrs: policy,
        })
        .expect("merge_tree succeeds");
    };
    let on = XattrPolicy {
        enabled: true,
        exclude: "user.sk*".to_string(),
    };
    let off = XattrPolicy {
        enabled: false,
        exclude: "user.sk*".to_string(),
    };

    let tmp = tempdir();
    let probe = tmp.join("probe");
    std::fs::write(&probe, b"").unwrap();
    if crate::helpers::xattr_set(&b(&probe), b"user.probe", b"1").is_err() {
        eprintln!("skipped: no user xattrs on {}", tmp.display());
        return;
    }

    // Same device, `-xattr`: rename semantics, everything survives.
    let d = image(&tmp);
    let root = tmp.join("ROOT");
    merge(&d, &root, &off);
    let dest = root.join("usr/share/xa/f");
    assert_eq!(get(&dest, b"user.keep"), Some(b"k".to_vec()));
    assert_eq!(get(&dest, b"user.skip"), Some(b"s".to_vec()));

    // `needs_move`: same bytes and mode, the xattr dropped on `dest`.
    let src = d.join("usr/share/xa/f");
    let bare = tmp.join("bare");
    std::fs::copy(&src, &bare).unwrap();
    std::fs::set_permissions(&bare, std::fs::metadata(&src).unwrap().permissions()).unwrap();
    assert!(needs_move(&src, &bare, &on));
    assert!(!needs_move(&src, &bare, &off));
    // An excluded-only difference never forces a move.
    crate::helpers::xattr_set(&b(&bare), b"user.keep", b"k").unwrap();
    assert!(!needs_move(&src, &bare, &on));

    // Cross device: the image on another filesystem than `ROOT`.
    let other = std::path::PathBuf::from("/dev/shm")
        .join(format!("portuale-xattr-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&other).unwrap();
    let same_fs =
        std::fs::metadata(&other).unwrap().dev() == std::fs::metadata(&tmp).unwrap().dev();
    let other_probe = other.join("probe");
    std::fs::write(&other_probe, b"").unwrap();
    let xattrs_there = crate::helpers::xattr_set(&b(&other_probe), b"user.probe", b"1").is_ok();
    if !same_fs && xattrs_there {
        for (name, policy, keep) in [("on", &on, true), ("off", &off, false)] {
            let d = image(&other.join(name));
            let root = tmp.join(format!("ROOT-x-{name}"));
            merge(&d, &root, policy);
            let dest = root.join("usr/share/xa/f");
            assert_eq!(get(&dest, b"user.keep").is_some(), keep, "{name}");
            assert_eq!(get(&dest, b"user.skip"), None, "{name}");
        }
    } else {
        eprintln!("cross-device half skipped (same fs or no xattrs on /dev/shm)");
    }
    let _ = std::fs::remove_dir_all(&other);
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

/// Backlog #96's core invariant: replacing a file must **never write
/// the existing inode**. The test holds the destination open and
/// checks that its bytes are untouched after the merge, while the
/// path itself carries the new content (a fresh inode). An in-place
/// `std::fs::copy` fails this (the open fd would read the new bytes);
/// the rename-based `replace_file_atomic` passes. This is the
/// property that keeps an mmap'd `libc.so.6`/`libreadline.so.8` alive
/// through its own merge, where `ETXTBSY` does **not** protect the
/// running process.
#[test]
fn merge_tree_never_writes_the_existing_inode() {
    use std::io::Read;

    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("lib/libx.so.1"), b"old library bytes").unwrap();
    std::fs::write(d.join("lib/libx.so.1"), b"new library bytes, longer").unwrap();
    let old_inode = std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino();

    // Stand-in for a process that has the file open/mapped: the fd
    // must keep reading the old inode after the merge.
    let mut old_fd = std::fs::File::open(root.join("lib/libx.so.1")).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "sys-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    let mut still_open = Vec::new();
    old_fd
        .read_to_end(&mut still_open)
        .expect("the old fd is still readable");
    assert_eq!(
        still_open, b"old library bytes",
        "the existing inode must never be written to"
    );
    assert_eq!(
        std::fs::read(root.join("lib/libx.so.1")).unwrap(),
        b"new library bytes, longer",
        "the path must carry the new content"
    );
    assert_ne!(
        std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino(),
        old_inode,
        "the destination must be a fresh inode"
    );
    let leftovers: Vec<String> = std::fs::read_dir(root.join("lib"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_portage_merge_"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no merge temporary may remain: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The live shape of backlog #96: a package upgrading a binary that
/// is **currently executing** (`/bin/bash` on every shell upgrade).
/// The old code's `std::fs::copy` hit `ETXTBSY` and aborted the merge
/// mid-copy (leaving the new files unowned, the vdb unwritten); the
/// rename-based replacement succeeds and the running process keeps
/// its old inode. `/bin/sleep` stands in for the shell: it is the
/// same execve-protected kernel path.
#[test]
fn merge_tree_replaces_a_running_executable_atomically() {
    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .expect("a system sleep binary");
    let true_bin = ["/bin/true", "/usr/bin/true"]
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .expect("a system true binary");

    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::copy(sleep, root.join("bin/prog")).unwrap();
    std::fs::copy(true_bin, d.join("bin/prog")).unwrap();
    let old_inode = std::fs::metadata(root.join("bin/prog")).unwrap().ino();

    let mut child = match std::process::Command::new(root.join("bin/prog"))
        .arg("60")
        .spawn()
    {
        Ok(child) => child,
        Err(first) => {
            // A fully loaded test machine (the whole-workspace run
            // executes every crate's tests) can transiently fail a
            // fork/exec with EAGAIN; retry once before giving up.
            std::thread::sleep(std::time::Duration::from_millis(500));
            std::process::Command::new(root.join("bin/prog"))
                .arg("60")
                .spawn()
                .unwrap_or_else(|e| panic!("spawn the old binary ({first}, {e})"))
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "the old binary must still be executing before the merge"
    );

    let mut cfgfiledict = BTreeMap::new();
    let outcome = merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "app-shells",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    });

    let alive = child.try_wait().expect("try_wait").is_none();
    let _ = child.kill();
    let _ = child.wait();
    outcome.expect("replacing a running executable must not write its inode (ETXTBSY would abort)");
    assert!(alive, "the running process must survive the merge");
    assert_eq!(
        std::fs::read(root.join("bin/prog")).unwrap(),
        std::fs::read(true_bin).unwrap(),
        "the path must carry the new binary"
    );
    assert_ne!(
        std::fs::metadata(root.join("bin/prog")).unwrap().ino(),
        old_inode,
        "the destination must be a fresh inode"
    );
    let leftovers: Vec<String> = std::fs::read_dir(root.join("bin"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_portage_merge_"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no merge temporary may remain: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A symlink replacement uses the same rename-based atomic path (no
/// remove-then-create window, no temporaries left behind), and the
/// destination is still a symlink pointing at the new target.
#[test]
fn merge_tree_replaces_a_symlink_without_temporaries() {
    let tmp = tempdir();
    let d = tmp.join("D");
    let root = tmp.join("ROOT");
    std::fs::create_dir_all(d.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::write(root.join("usr/lib/libx.so.1"), b"lib").unwrap();
    std::fs::write(d.join("usr/lib/libx.so.1"), b"lib").unwrap();
    std::os::unix::fs::symlink("libx.so.0", root.join("usr/lib/libx.so")).unwrap();
    std::os::unix::fs::symlink("libx.so.1", d.join("usr/lib/libx.so")).unwrap();

    let mut cfgfiledict = BTreeMap::new();
    merge_tree(MergeTreeArgs {
        d: &d,
        root: &root,
        category: "sys-libs",
        installed_instance_pf: None,
        protect_if_modified: false,
        config_protect: "/etc",
        config_protect_mask: "/etc/env.d",
        noconfmem: false,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::default(),
    })
    .expect("merge_tree succeeds");

    assert_eq!(
        std::fs::read_link(root.join("usr/lib/libx.so")).unwrap(),
        PathBuf::from("libx.so.1")
    );
    assert!(
        std::fs::symlink_metadata(root.join("usr/lib/libx.so"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let leftovers: Vec<String> = std::fs::read_dir(root.join("usr/lib"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_portage_merge_"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no merge temporary may remain: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn merge_tree_preserves_ownership_from_the_source_when_root() {
    // Real movefile()'s lchown/chown (symlink, regular file) and
    // mergeme()'s own directory chown/chmod (a newly created
    // directory only): the merged destination's owner/group must
    // match the *source*'s own recorded uid/gid, not whoever ran
    // the merge. Only actually exercisable as root -- chowning to
    // an arbitrary uid needs privilege, the same real precondition
    // this whole feature has (see `lchown_or_chown`'s own doc
    // comment). Run under `sudo cargo test ...` to exercise it;
    // skipped (not failed) otherwise, matching that real
    // precondition rather than asserting something no local
    // environment could ever satisfy.
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

    // A uid/gid that's certainly neither 0 nor this process's own.
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

    // An already-existing directory is left alone (real mergeme()'s
    // "kept as-is" branch, vartree.py:5808-5810) -- merging a
    // *second* time with a different source owner must not clobber
    // usr/share/x's now-installed ownership.
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

    // The real, logical path is untouched...
    assert_eq!(
        std::fs::read_to_string(root.join("etc/foo.conf")).unwrap(),
        "user's own edits"
    );
    // ...and the new content lands in a ._cfg0000_ sibling instead.
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_foo.conf")).unwrap(),
        "new content"
    );
    // CONTENTS still records the logical path with the *new*
    // content's own MD5 (real dblink.mergeme()'s own behavior --
    // see merge_tree's own doc comment).
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
    // Real `_installed_instance`/`protect_if_modified`
    // (`vartree.py:5849-5866`): the live destination still holds
    // *exactly* what the previous same-slot instance's own real
    // CONTENTS recorded -- the admin never touched it -- so even
    // though it differs from the new src content, it's not
    // "modified" in the sense this feature cares about, and the new
    // content is applied directly instead of diverted.
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
    // Same setup as above, but the live destination's own content no
    // longer matches what the installed instance recorded -- the
    // admin *did* modify it locally, so protect_if_modified must not
    // waive protection here.
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
    // Real bug #523684 (`vartree.py:5852-5859`): the installed
    // instance's own CONTENTS recorded this exact path, but nothing
    // exists there on disk at all right now (the admin deleted or
    // renamed it) -- real `force = True` diverts into a fresh
    // ._cfgNNNN_ sibling instead of silently re-creating the path
    // the admin deliberately removed, even though there's nothing
    // to compare content against.
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
    // Real `move_me = protected = bool(cfgfiledict["IGNORE"])` with
    // `IGNORE == 0` (`vartree.py:5877`, "confmem rejected this
    // update"): re-merging an already-offered, unmodified-since
    // update skips the write entirely -- the live destination stays
    // exactly what the admin last left it as, no second `._cfg0001_`
    // file spawned either. See `protect_decision`'s own doc comment
    // for the full real trace.
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

    // Re-merging the exact same new content again: already
    // remembered in cfgfiledict, so real portage leaves the live
    // destination completely untouched this time -- no second
    // ._cfg0001_ file spawned either.
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

    // CONTENTS still logically records this package as the owner of
    // the *new* content, using the source's own MD5 -- real
    // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (the source's
    // own mtime, set before the real `if moveme:` gate and never
    // touched when it's skipped) flowing into `_format_contents_line`
    // regardless of `moveme`.
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

    // Re-merging the exact same content with NOCONFMEM-equivalent
    // (noconfmem=true): unlike the default (dest gets directly
    // overwritten, see the test above), this forces re-protection
    // every time regardless of cfgfiledict memory -- real
    // `--noconfmem`/`cfgfiledict["IGNORE"]` -- so the logical path is
    // left alone again. `new_protect_filename`'s own "reuse the last
    // file when content already matches" logic (this slice's own
    // third piece) then reuses ._cfg0000_ rather than spawning a
    // ._cfg0001_ with identical content, so the *visible* difference
    // from the default isn't a new numbered file -- it's that the
    // logical path itself is protected instead of overwritten.
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

    // The real, logical path is untouched...
    assert_eq!(
        std::fs::read_link(root.join("etc/link.conf")).unwrap(),
        PathBuf::from("users-own-target")
    );
    // ...and the new target lands in a ._cfg0000_ sibling instead.
    assert_eq!(
        std::fs::read_link(root.join("etc/._cfg0000_link.conf")).unwrap(),
        PathBuf::from("new-target")
    );
    // CONTENTS still records the logical path with the *new* target.
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
    // Real dblink._protect()'s own type-independent comparison
    // (vartree.py:5434-5480): dest_md5/dest_link are computed from
    // the live destination's own on-disk type regardless of the
    // incoming source's type -- a symlink source landing on a path
    // the admin's own regular file still occupies is real-protected
    // too, not silently overwritten.
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

    // The admin's own regular file at the logical path is untouched...
    assert_eq!(
        std::fs::read_to_string(root.join("etc/thing.conf")).unwrap(),
        "the admin's own regular file"
    );
    // ...and the new symlink lands in a ._cfg0000_ sibling instead.
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
    // The mirror image of the test above: an `obj` source landing on
    // a path a symlink (admin-installed, or left over from a
    // previous, differently-shaped package version) still occupies.
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

    // The admin's own symlink at the logical path is untouched...
    assert_eq!(
        std::fs::read_link(root.join("etc/thing.conf")).unwrap(),
        PathBuf::from("admins-own-target")
    );
    // ...and the new regular-file content lands in a ._cfg0000_
    // sibling instead.
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

/// Device-node creation (`mknod(2)` with `S_IFCHR`/`S_IFBLK`) genuinely
/// requires root/`CAP_MKNOD` for a *real* (nonzero major:minor)
/// device on a real Linux system -- confirmed empirically both via a
/// plain standalone `mknod(2)` call and via this very function, as
/// portuale's own unprivileged dev/test user. (A privilege-free
/// carve-out does exist for `mknod(path, S_IFCHR, 0)` specifically --
/// the real kernel's own overlayfs "whiteout" convention, `dev_t ==
/// 0` never being a usable real device -- which is precisely why
/// this test passes `/dev/null` itself as `src`, not an arbitrary
/// regular file: only a real char device's own real, nonzero `rdev`
/// actually exercises the real privileged path.) Not reproducible as
/// a real, live end-to-end test in this environment, unlike the
/// `fif` case above. This narrower test instead confirms
/// `create_special_node` itself propagates that real failure cleanly
/// via `Result` (no panic) -- a permission error surfacing as an
/// ordinary merge failure, not a crash.
#[test]
fn create_special_node_reports_a_permission_failure_cleanly_rather_than_panicking() {
    let tmp = tempdir();
    let dest = tmp.join("devnode");

    let dev_null = Path::new("/dev/null");
    let dev_null_type = std::fs::symlink_metadata(dev_null).unwrap().file_type();
    assert!(dev_null_type.is_char_device());

    let err = create_special_node(dev_null, &dest, &dev_null_type)
        .expect_err("mknod(2) for a real, nonzero-rdev char device requires root");
    assert!(
        err.contains("Operation not permitted") || err.contains("permitted"),
        "{err}"
    );
    assert!(
        !dest.exists(),
        "a failed mknod must not leave a partial node behind"
    );
}
