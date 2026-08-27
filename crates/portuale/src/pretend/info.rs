//! `emerge --info`.

use crate::pretend::*;

/// `emerge --info`: the deterministic config/repository block plus the per-atom
/// `Package Settings` section. Host-state output (version probes, timestamps)
/// is deliberately cut; see the notes. With `pkg_info()` defined, the phase
/// actually runs.
/// Portage: _emerge/actions.py, action_info
///
/// One `USE_EXPAND` variable's `emerge --info` display value: the flags actually
/// enabled for that prefix in the final USE set, in the raw value's own order
/// followed by any extra enabled flags sorted. `None` when nothing is enabled.
/// Portage: config.py, regenerate
pub(crate) fn use_expand_display_value(
    var: &str,
    resolved_use: &std::collections::BTreeSet<String>,
    config: &portage_profile::Config,
) -> Option<String> {
    let prefix = format!("{}_", var.to_lowercase());
    let enabled: std::collections::BTreeSet<&str> = resolved_use
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix))
        .collect();
    if enabled.is_empty() {
        return None;
    }
    let raw = config.other_vars.get(var).map(String::as_str).unwrap_or("");
    let mut out: Vec<&str> = raw
        .split_whitespace()
        .filter(|t| enabled.contains(t))
        .collect();
    for f in &enabled {
        if !out.contains(f) {
            out.push(f);
        }
    }
    // `out` is now raw-order kept + the rest appended; the appended tail
    // is already sorted because `enabled` is a `BTreeSet`.
    Some(out.join(" "))
}

/// The highest installed version of `cat/pkg` (`-r0` dropped), if any.
pub(crate) fn highest_installed_pvr(root: &Path, category: &str, package: &str) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .max_by(|a, b| portage_versions::vercmp(a, b).unwrap_or(0).cmp(&0))
        .map(|v| v.strip_suffix("-r0").unwrap_or(&v).to_string())
}
