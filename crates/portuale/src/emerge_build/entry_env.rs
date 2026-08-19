//! The per-entry build environment.

use crate::emerge_build::*;

/// The `cat/pkg-ver:slot/sub` identity a build-bound entry is matched
/// against. Missing `SLOT` means slot `0`; a missing sub-slot repeats
/// the slot.
pub(crate) fn entry_cpv_slot(entry: &GraphEntry, version: &str) -> String {
    let slot = entry.slot.as_deref().unwrap_or("0");
    let sub_slot = entry.sub_slot.as_deref().unwrap_or(slot);
    format!(
        "{}/{}-{version}:{slot}/{sub_slot}",
        entry.category, entry.package
    )
}

/// The `package.env` build vars matching one `cat/pkg-ver:slot/sub`
/// identity; incrementals fold onto the run-wide base instead of
/// replacing it.
// Portage: config.py, _grab_pkg_env
pub(crate) fn matched_package_env_vars(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    profile_only_variables: &[String],
    base_env: &[(String, String)],
) -> Vec<(String, String)> {
    if package_env_vars.is_empty() {
        return Vec::new();
    }
    crate::ebuild_phases::match_package_env_vars(
        package_env_vars,
        cpv_slot,
        profile_only_variables,
        base_env,
        &portage_profile::config_env_all(),
    )
}

/// [`matched_package_env_vars`] for one scheduler entry, layered over
/// `options.build_env`.
pub(crate) fn entry_package_env_vars(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
) -> Vec<(String, String)> {
    let Some(version) = entry_version(&entry.outcome) else {
        return Vec::new();
    };
    let cpv_slot = entry_cpv_slot(entry, version);
    let profile_only_variables = options
        .resolved_config
        .as_deref()
        .and_then(|config| config.resolved_incremental("PROFILE_ONLY_VARIABLES"))
        .unwrap_or_default();
    matched_package_env_vars(
        &options.package_env_vars,
        &cpv_slot,
        &profile_only_variables,
        &options.build_env,
    )
}
