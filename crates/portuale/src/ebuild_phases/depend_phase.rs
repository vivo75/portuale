//! The depend phase and its metadata cache.

use crate::ebuild_phases::*;

/// `$PORTAGE_DEPCACHEDIR` when set, else `/var/cache/edb/dep`; process env only.
/// Portage: config.py, settings.depcachedir
pub(crate) fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}
