//! VDB metadata written after src_install.

use crate::ebuild_phases::*;

/// The resolved `USE` for this package, from the `build_env` slice (empty for
/// standalone runs).
pub(crate) fn build_phase_use(build_env: &[(String, String)]) -> std::collections::HashSet<String> {
    build_env
        .iter()
        .find(|(k, _)| k == "USE")
        .map(|(_, v)| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}
