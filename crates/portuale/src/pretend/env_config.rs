//! Config and repo loading, EMERGE_DEFAULT_OPTS, scheduling policy.

use crate::pretend::*;

/// `INSTALL_MASK` folded with the `no{man,info,doc}` tokens; non-empty process env
/// wins, else the resolved config. Empty-blanking-the-config is an unhandled corner.
/// Portage: bin/misc-functions.sh, preinst_mask
///
/// Explicit `--buildpkg[=n]` wins, else resolved `FEATURES` decides; split out for
/// testability. Portage: create_depgraph_params.py
pub(crate) fn buildpkg_from_config(
    buildpkg_opt: Option<bool>,
    config: &portage_profile::Config,
) -> bool {
    buildpkg_opt.unwrap_or_else(|| config_features_list(config).iter().any(|t| t == "buildpkg"))
}

/// Resolved `FEATURES` if assigned anywhere: the folded stack, else a raw split of
/// `other_vars["FEATURES"]`. `None` means nothing configured anywhere.
pub(crate) fn config_features_opt(config: &portage_profile::Config) -> Option<Vec<String>> {
    config.resolved_incremental("FEATURES").or_else(|| {
        config
            .other_vars
            .get("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
    })
}

pub(crate) fn config_features_list(config: &portage_profile::Config) -> Vec<String> {
    config_features_opt(config).unwrap_or_default()
}

/// Resolved merge-time `PORTAGE_BZIP2_COMMAND`; shared with the remote server.
pub(crate) fn config_bzip2_command(config: &portage_profile::Config) -> String {
    config
        .other_vars
        .get("PORTAGE_BZIP2_COMMAND")
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bzip2".to_string())
}
