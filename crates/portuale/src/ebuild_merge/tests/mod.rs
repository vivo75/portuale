use super::*;

use portage_util::TempDir;

/// `chown()`s a test-source path directly, without going through the
/// function under test.
fn raw_chown(path: &Path, uid: u32, gid: u32, is_symlink: bool) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let ret = if is_symlink {
        // SAFETY: `c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::lchown(c.as_ptr(), uid, gid) }
    } else {
        // SAFETY: `c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::chown(c.as_ptr(), uid, gid) }
    };
    assert_eq!(
        ret,
        0,
        "{}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
}

fn tempdir() -> std::path::PathBuf {
    TempDir::new("portuale-ebuild-merge-test").keep()
}

fn collision_fixture(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs")
        .join(name)
        .join(format!("{name}-1.0.ebuild"))
}

fn versioned_fixture(name: &str, version: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs")
        .join(name)
        .join(format!("{name}-{version}.ebuild"))
}

/// Seeds a root where preserved `/usr/lib/keepA.so` has exactly one
/// consumer, `/usr/bin/cprog`, owned by `oldapp-1.0`; returns the root
/// and the `being_unmerged` set an unmerge of the old instance takes.
fn seed_old_owned_consumer_root(tmp: &std::path::Path) -> (PathBuf, BTreeSet<String>) {
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/keepA.so"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/cprog"), b"fake elf").unwrap();
    for (pf, contents, needed) in [
        (
            "provider-1.0",
            "obj /usr/lib/keepA.so def 1\n",
            "X86_64;/usr/lib/keepA.so;libkeep.so.1;;\n",
        ),
        (
            "oldapp-1.0",
            "obj /usr/bin/cprog abc 1\n",
            "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
        ),
    ] {
        let vdb = root.join("var/db/pkg/dev-libs").join(pf);
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("CONTENTS"), contents).unwrap();
        std::fs::write(vdb.join("NEEDED.ELF.2"), needed).unwrap();
    }
    std::fs::write(root.join("var/db/pkg/dev-libs/oldapp-1.0/COUNTER"), "7\n").unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/provider:0".to_string(),
        (
            "dev-libs/provider-1.0".to_string(),
            "4".to_string(),
            vec!["/usr/lib/keepA.so".to_string()],
        ),
    );
    write_plib_registry(
        &root,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("seeding the registry succeeds");
    let being: BTreeSet<String> = ["/usr/bin/cprog".to_string()].into_iter().collect();
    (root, being)
}

/// Compiles a linked `libA`/`libB` pair with the host `cc` under the
/// synthetic root, so `scanelf` sees genuine ELF headers.
fn compile_linked_pair(tmp: &std::path::Path, root: &std::path::Path) {
    let build = tmp.join("build");
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(build.join("a.c"), "int aval(void) { return 1; }\n").unwrap();
    std::fs::write(
        build.join("b.c"),
        "extern int aval(void);\nint bval(void) { return aval(); }\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("cc")
            .args(args)
            .current_dir(&build)
            .status()
            .expect("host cc must exist for the orphan-scan test");
        assert!(status.success(), "cc {args:?} must succeed");
    };
    run(&[
        "-shared",
        "-fPIC",
        "-Wl,-soname,libA.so.1",
        "-o",
        "libA.so.1.0.0",
        "a.c",
    ]);
    std::os::unix::fs::symlink("libA.so.1.0.0", build.join("libA.so")).unwrap();
    run(&[
        "-shared",
        "-fPIC",
        "-Wl,-soname,libB.so.1",
        "-o",
        "libB.so.1.0.0",
        "b.c",
        "-L.",
        "-lA",
    ]);
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::copy(
        build.join("libA.so.1.0.0"),
        root.join("usr/lib/libA.so.1.0.0"),
    )
    .unwrap();
    std::fs::copy(
        build.join("libB.so.1.0.0"),
        root.join("usr/lib/libB.so.1.0.0"),
    )
    .unwrap();
}

fn env_update_fixture() -> PathBuf {
    collision_fixture("envupdatepkg")
}

fn fixtures_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

type PublishHook = Box<dyn Fn(&Path)>;

thread_local! {
    /// Test hook run inside `publish_merged_entry` and
    /// `RetireWrites::commit` before the commit, on this thread.
    static PUBLISH_HOOK: std::cell::RefCell<Option<PublishHook>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn publish_hook(root: &Path) {
    PUBLISH_HOOK.with(|hook| {
        if let Some(f) = hook.borrow().as_ref() {
            f(root);
        }
    });
}

/// The merge on a database backend.
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod db_merge;

mod after_install;
mod collisions;
mod installed_instance;
mod options;
mod plib_merge;
mod plib_registry;
mod plib_unmerge;
mod protect;
mod run_merge;
mod tree;
