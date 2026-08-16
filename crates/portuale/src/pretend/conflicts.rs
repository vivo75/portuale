//! Slot-conflict text helpers.

use crate::pretend::*;

/// Portage's `Package.__str__` quoted destination for a skipped-block node
/// whose own root is not `/` -- ` to '<ROOT>'`, inside the paren, or empty
/// when that root is `/`. `running` selects which display root applies.
/// Portage: _emerge/Package.py, Package.__str__
pub(crate) fn skipped_node_suffix(
    running: bool,
    root: &Path,
    running_root: Option<&Path>,
) -> String {
    let own: Option<&Path> = if running { running_root } else { Some(root) };
    match own {
        Some(r) if r != Path::new("/") => format!(" to '{}'", r.display()),
        _ => String::new(),
    }
}
