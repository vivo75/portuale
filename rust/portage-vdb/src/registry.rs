//! The process-wide backend registry (`vdb_to_db.md` §6.2).
//!
//! The resolver and the merge code are free functions that take a
//! `root: &Path`; they look up the backend for that root here instead of
//! threading a new parameter through their callers. Same env-free
//! process-global pattern as `portage_repo::set_resolver_debug`, with a
//! map instead of a flag. `mrg` registers its chosen backend before
//! `pretend::run` (S3.1); `emerge` never registers, so every root falls
//! back to a [`FilesDb`].
//!
//! **Root keys are the path as given**, not canonicalised, exactly like
//! today's `root.join("var/db/pkg")` (no extra syscall). `Path` equality
//! compares components, so `/r` and `/r/` are the same key, but `/r` and
//! a symlink to it are not. Callers must register and look up with the
//! same spelling of the root.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use crate::{FilesDb, InstalledDb};

type Registry = RwLock<HashMap<PathBuf, Arc<dyn InstalledDb>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Make `db` the backend for `root`, replacing what was there (a
/// registered backend or the cached default). Returns the previous one.
pub fn register(root: &Path, db: Arc<dyn InstalledDb>) -> Option<Arc<dyn InstalledDb>> {
    registry()
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(root.to_path_buf(), db)
}

/// The backend for `root`: the registered one, or else a
/// [`FilesDb::new(root)`](FilesDb::new), created on the first call and
/// cached so later calls return the same instance (and its in-process
/// caches). A hit costs one read lock and one hash lookup.
pub fn for_root(root: &Path) -> Arc<dyn InstalledDb> {
    if let Some(db) = registry()
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(root)
    {
        return Arc::clone(db);
    }
    let mut map = registry().write().unwrap_or_else(PoisonError::into_inner);
    // Another thread may have filled it between the two locks.
    Arc::clone(
        map.entry(root.to_path_buf())
            .or_insert_with(|| Arc::new(FilesDb::new(root))),
    )
}

/// Forget every registered and cached backend. For tests only: a test
/// that registers a backend calls this (or registers over it) so the
/// next test sees the default again. Handles already returned by
/// [`for_root`] stay valid.
#[doc(hidden)]
pub fn reset() {
    registry()
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendKind, Error};
    use std::sync::Mutex;

    /// The registry is process-global and the test harness runs tests in
    /// parallel: serialise the tests that touch it.
    static LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn vdb(db: &Arc<dyn InstalledDb>) -> PathBuf {
        db.vdb_dir().expect("files backend has a vdb dir")
    }

    #[test]
    fn default_is_a_cached_files_db_for_that_root() {
        let _g = guard();
        reset();
        let root = Path::new("/nonexistent/s1.1/default");
        let a = for_root(root);
        assert_eq!(a.kind(), BackendKind::Files);
        assert_eq!(vdb(&a), root.join("var/db/pkg"));
        let b = for_root(root);
        assert!(Arc::ptr_eq(&a, &b), "the default is cached per root");
        // Same key for a trailing slash (component equality, no
        // canonicalisation).
        let c = for_root(Path::new("/nonexistent/s1.1/default/"));
        assert!(Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn register_overrides_the_default_and_returns_the_previous() {
        let _g = guard();
        reset();
        let root = Path::new("/nonexistent/s1.1/override");
        let default = for_root(root);
        let other: Arc<dyn InstalledDb> =
            Arc::new(FilesDb::open_vdb_dir(Path::new("/nonexistent/shadow")));
        let prev = register(root, Arc::clone(&other)).expect("default was cached");
        assert!(Arc::ptr_eq(&prev, &default));
        let now = for_root(root);
        assert!(Arc::ptr_eq(&now, &other));
        assert_eq!(vdb(&now), Path::new("/nonexistent/shadow"));
        // Registering again replaces it.
        let third: Arc<dyn InstalledDb> = Arc::new(FilesDb::new(Path::new("/elsewhere")));
        let prev = register(root, Arc::clone(&third)).expect("registered one");
        assert!(Arc::ptr_eq(&prev, &other));
        assert!(Arc::ptr_eq(&for_root(root), &third));
    }

    #[test]
    fn two_roots_are_independent() {
        let _g = guard();
        reset();
        let a = Path::new("/nonexistent/s1.1/root-a");
        let b = Path::new("/nonexistent/s1.1/root-b");
        let shadow: Arc<dyn InstalledDb> =
            Arc::new(FilesDb::open_vdb_dir(Path::new("/nonexistent/a-shadow")));
        assert!(register(a, Arc::clone(&shadow)).is_none());
        let db_b = for_root(b);
        assert!(!Arc::ptr_eq(&for_root(a), &db_b));
        assert!(Arc::ptr_eq(&for_root(a), &shadow));
        assert_eq!(vdb(&db_b), b.join("var/db/pkg"));
    }

    #[test]
    fn reset_restores_the_default() {
        let _g = guard();
        reset();
        let root = Path::new("/nonexistent/s1.1/reset");
        let shadow: Arc<dyn InstalledDb> =
            Arc::new(FilesDb::open_vdb_dir(Path::new("/nonexistent/r-shadow")));
        register(root, Arc::clone(&shadow));
        reset();
        let db = for_root(root);
        assert!(!Arc::ptr_eq(&db, &shadow));
        assert_eq!(db.kind(), BackendKind::Files);
        assert_eq!(vdb(&db), root.join("var/db/pkg"));
        // The old handle is still usable.
        assert_eq!(vdb(&shadow), Path::new("/nonexistent/r-shadow"));
    }

    #[test]
    fn for_root_is_safe_across_threads() {
        let _g = guard();
        reset();
        let root = PathBuf::from("/nonexistent/s1.1/threads");
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                std::thread::spawn(move || for_root(&root))
            })
            .collect();
        let dbs: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(dbs.iter().all(|d| Arc::ptr_eq(d, &dbs[0])));
    }

    #[test]
    fn files_reports_unsupported_for_unmoved_methods_and_not_panics() {
        let db = FilesDb::new(Path::new("/nonexistent/s1.1/stub"));
        let key = crate::EntryKey::new("app-shells", "bash-5.2");
        assert_eq!(
            db.entry_path(&key),
            Some(PathBuf::from(
                "/nonexistent/s1.1/stub/var/db/pkg/app-shells/bash-5.2"
            ))
        );
        // S1.2 moved the per-entry reads here: a missing VDB is "no
        // entries", not an error.
        assert_eq!(db.generation().unwrap(), 0);
        assert_eq!(db.aux_get(&key, "SLOT").unwrap(), None);
        assert!(matches!(
            db.reverse_dependents("a/b", &[]),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(db.begin_write(), Err(Error::Unsupported(_))));
        let msg = db.world().unwrap_err().to_string();
        assert!(
            msg.contains("FilesDb::world") && msg.contains("S1.6"),
            "{msg}"
        );
    }
}
