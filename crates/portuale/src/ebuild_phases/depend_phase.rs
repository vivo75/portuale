//! The depend phase and its metadata cache.

use crate::ebuild_phases::*;

/// Portage's flat depcachedir layout (`cache/flat_hash.py`):
/// `<depcachedir>/<repo path without '/'>/<cat>/<pf>`.
pub(crate) fn depcache_entry_path(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> std::path::PathBuf {
    let location = repo_location.to_string_lossy();
    depcachedir()
        .join(location.trim_start_matches('/').trim_end_matches('/'))
        .join(category)
        .join(pf)
}

/// `$PORTAGE_DEPCACHEDIR` when set, else `/var/cache/edb/dep`; process env only.
/// Portage: config.py, settings.depcachedir
pub(crate) fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}
