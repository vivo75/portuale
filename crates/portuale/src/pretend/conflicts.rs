//! Slot-conflict text helpers.

use crate::pretend::*;

/// Portage's `pkg_use_display(pkg, ...)` return string -- `USE="…"` followed
/// by one ` VAR="…"` per non-hidden `USE_EXPAND` group. Always renders a
/// `USE=` field, even with no flags. Flag colorization is cut.
/// Portage: dep/slot_collision.py, pkg_use_display
pub(crate) fn render_pkg_use_display(disp: &[(String, String)]) -> String {
    let use_body = disp
        .iter()
        .find(|(name, _)| name == "USE")
        .map(|(_, body)| body.as_str())
        .unwrap_or("");
    let mut out = format!("USE=\"{use_body}\"");
    for (name, body) in disp {
        if name != "USE" {
            out.push_str(&format!(" {name}=\"{body}\""));
        }
    }
    out
}

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
