//! Display roots: Portage normalises every root to exactly one trailing slash.

use crate::*;

/// Portage's normalised root for display: exactly one trailing slash
/// (`/` stays `/`). Every non-`/` root is normalised this way at config
/// load, so `pkg.root`/`settings['ROOT']` always carry it.
// Portage: package/ebuild/_config/LocationsManager.py, set_root_override
pub fn root_with_trailing_slash(root: &Path) -> String {
    let shown = root.display().to_string();
    if shown.ends_with('/') {
        shown
    } else {
        format!("{shown}/")
    }
}
