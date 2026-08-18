use super::*;

use portage_util::TempDir;

fn tempdir() -> std::path::PathBuf {
    TempDir::new("portuale-ebuild-merge-test").keep()
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
