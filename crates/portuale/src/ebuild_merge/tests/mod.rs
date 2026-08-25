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

mod collisions;
/// The merge on a database backend.
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod db_merge;
mod plib_registry;
mod protect;
mod run_merge;
mod tree;
