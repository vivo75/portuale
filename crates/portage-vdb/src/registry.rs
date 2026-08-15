//! The process-wide backend registry.
//!
//! The resolver and the merge code are free functions that take a
//! `root: &Path`; they look up the backend for that root here instead of
//! threading a new parameter through their callers.
//!
//! **Root keys are the path as given**, not canonicalised. `Path` equality
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
