//! The per-entry build environment.

use crate::emerge_build::*;

/// This entry's per-package `PORTAGE_TMPDIR`, re-derived from its matched
/// `package.env` value so the pre-clean, phase chain, build log and
/// post-merge clean share one root. A missing matched directory aborts
/// the entry before anything runs.
// Portage: EbuildBuild.py, _check_temp_dir
pub(crate) fn entry_portage_tmpdir(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    version: &str,
    portage_tmpdir: &Path,
) -> Result<PathBuf, String> {
    let cpv_slot = entry_cpv_slot(entry, version);
    crate::ebuild_phases::resolve_entry_portage_tmpdir(
        &options.package_env_vars,
        &cpv_slot,
        portage_tmpdir,
        options.process_tmpdir.as_deref(),
    )
}

/// This entry's per-package `FEATURES`: the matched env-file value folds
/// onto the run-wide list in `[run-wide, pkg, calling-env]` order instead
/// of replacing it. `None` when nothing matched, so the caller keeps the
/// run-wide value untouched.
pub(crate) fn resolved_features_for(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    run_wide_features: &str,
    calling_features: &str,
) -> Option<String> {
    let pkg_raw = crate::ebuild_phases::match_package_env_incremental_raw(
        package_env_vars,
        cpv_slot,
        "FEATURES",
    );
    if pkg_raw.is_empty() {
        return None;
    }
    Some(crate::ebuild_phases::fold_package_env_incremental(
        run_wide_features,
        &pkg_raw,
        calling_features,
    ))
}

/// [`resolved_features_for`] for one scheduler entry. `None` when the
/// entry is not buildable either.
pub(crate) fn entry_resolved_features(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    calling_features: &str,
) -> Option<String> {
    let version = entry_version(&entry.outcome)?;
    let cpv_slot = entry_cpv_slot(entry, version);
    resolved_features_for(
        &options.package_env_vars,
        &cpv_slot,
        &options.features,
        calling_features,
    )
}

/// `[("USE", "<space-joined enabled IUSE flags>")]` for `entry`, or empty
/// when the entry declares no enabled flags.
pub(crate) fn build_use_env(entry: &GraphEntry) -> Vec<(String, String)> {
    let enabled: Vec<&str> = entry
        .use_flags_display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    if enabled.is_empty() {
        Vec::new()
    } else {
        vec![("USE".to_string(), enabled.join(" "))]
    }
}

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

/// `SLOT`, `PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS` for `entry`.
/// `SLOT` is `slot/sub_slot` when a sub-slot is declared, bare slot
/// otherwise, never empty (`entry.slot` unset means `"0"`).
/// `PORTAGE_REPO_REVISIONS` is `"{}"` until portuale tracks a revision;
// Portage: doebuild.py, doebuild_environment
pub(crate) fn entry_identity_env(
    entry: &GraphEntry,
    candidate: Option<&Candidate>,
) -> Vec<(String, String)> {
    let slot = entry
        .slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.slot.as_str()))
        .unwrap_or("0");
    let sub_slot = entry
        .sub_slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.sub_slot.as_str()))
        .unwrap_or(slot);
    let slot_value = if sub_slot.is_empty() || sub_slot == slot {
        slot.to_string()
    } else {
        format!("{slot}/{sub_slot}")
    };
    let repo_name = entry
        .repo_name
        .as_deref()
        .or_else(|| candidate.map(|c| c.repo_name.as_str()))
        .unwrap_or("");
    let mut env = vec![
        ("SLOT".to_string(), slot_value),
        ("PORTAGE_REPO_REVISIONS".to_string(), "{}".to_string()),
    ];
    if !repo_name.is_empty() {
        env.push(("PORTAGE_REPO_NAME".to_string(), repo_name.to_string()));
    }
    env
}

/// The run-wide half of `config.environ()` for a build, plus the dynamic
/// `PORTAGE_COMPRESSION_COMMAND` from portuale's compressor table.
pub(crate) fn run_wide_phase_env(config: &portage_profile::Config) -> Vec<(String, String)> {
    let mut env = portage_profile::phase_environ(config, None);
    let lookup = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var(key).ok())
            .or_else(|| config.other_vars.get(key).cloned())
    };
    if let Some(cmd) = ebuild_package::phase_compression_command(lookup) {
        env.push(("PORTAGE_COMPRESSION_COMMAND".to_string(), cmd));
    }
    env
}

/// The package metadata keys that stay exported in the saved environment
/// (`DEFINED_PHASES`, `KEYWORDS`, `LICENSE`); values come from the
/// md5-cache, `""` when omitted.
pub(crate) const EXPORTED_PKG_METADATA: [&str; 3] = ["DEFINED_PHASES", "KEYWORDS", "LICENSE"];

pub(crate) fn entry_metadata_env(
    entry: &GraphEntry,
    candidate: &Candidate,
) -> Vec<(String, String)> {
    let pf = format!("{}-{}", entry.package, candidate.version);
    let cache = mrg_director::Md5Cache::new(&candidate.repo_location, &candidate.repo_name);
    let Ok(metadata) = mrg_director::RepoCache::metadata(&cache, &entry.category, &pf) else {
        return Vec::new();
    };
    EXPORTED_PKG_METADATA
        .iter()
        .map(|k| (k.to_string(), metadata.get(*k).cloned().unwrap_or_default()))
        .collect()
}

/// The per-entry tail of [`entry_build_env`]: resolved `USE` (or the
/// legacy enabled-IUSE-only `USE` when no config is in scope) plus the
/// `SLOT`/repo identity. Shared with `run_buildpkgonly`.
///
/// With `config` set the `USE` is the resolver's full effective set
/// narrowed to `IUSE ∪ IUSE_EFFECTIVE`.
pub(crate) fn entry_phase_env_tail(
    config: Option<&portage_profile::Config>,
    repos: &[RepoConfig],
    entry: &GraphEntry,
    candidate: Option<&Candidate>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let Some(config) = config else {
        env.extend(build_use_env(entry));
        return env;
    };
    if let Some(candidate) = candidate {
        env.extend(entry_metadata_env(entry, candidate));
        let enabled = portage_repo::candidate_effective_use_flags(
            repos,
            config,
            &entry.category,
            &entry.package,
            &candidate.version,
        );
        let iuse: Vec<String> = candidate
            .iuse
            .split_whitespace()
            .map(String::from)
            .collect();
        env.extend(portage_profile::phase_environ_pkg(
            config,
            Some(portage_profile::PhaseUse {
                iuse: &iuse,
                enabled: &enabled,
            }),
        ));
        // Portage's `setcpv` unprefixed filter over the combined map:
        // `ARCH="amd64"` with no `amd64` in USE exports as `""`.
        let use_flags = portage_profile::portage_use(config, &iuse, &enabled);
        let use_set: std::collections::HashSet<String> = use_flags.iter().cloned().collect();
        for var in &config.use_expand_unprefixed {
            if let Some(current) = env.iter().find(|(k, _)| k == var).map(|(_, v)| v.clone())
                && let Some(slot) = env.iter_mut().find(|(k, _)| k == var)
            {
                slot.1 = portage_profile::filter_unprefixed_use_expand_value(&current, &use_set);
            }
        }
    }
    env.extend(entry_identity_env(entry, candidate));
    env
}

/// The full per-entry build-phase env: the run-wide resolved env, then any
/// per-package `package.env` build vars, then [`entry_phase_env_tail`].
pub(crate) fn entry_build_env(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    repos: &[RepoConfig],
) -> Vec<(String, String)> {
    let candidate = entry_version(&entry.outcome)
        .and_then(|version| locate_candidate(repos, &entry.category, &entry.package, version));
    // A package task re-reads env.d; unchanged `profile.env` keeps the
    // old path.
    let reloaded = options.resolved_config.as_deref().and_then(env_d_reloaded);
    let mut env = match &reloaded {
        Some(r) => run_wide_phase_env(r),
        None => options.build_env.clone(),
    };
    env.extend(entry_package_env_vars(options, entry));
    if let (Some(config), Some(version)) = (
        reloaded.as_ref().or(options.resolved_config.as_deref()),
        entry_version(&entry.outcome),
    ) {
        env.extend(crate::ebuild_phases::package_bashrc_env(
            config,
            &entry_cpv_slot(entry, version),
        ));
    }
    // Last-wins: the folded per-entry list overrides the run-wide
    // `FEATURES`/`PORTAGE_FEATURES` pair downstream. `None` appends
    // nothing, so unmatched entries keep byte-identical output.
    let calling_features = std::env::var("FEATURES").unwrap_or_default();
    if let Some(features) = entry_resolved_features(options, entry, &calling_features) {
        env.push(("FEATURES".to_string(), features.clone()));
        env.push(("PORTAGE_FEATURES".to_string(), features));
    }
    env.extend(entry_phase_env_tail(
        reloaded.as_ref().or(options.resolved_config.as_deref()),
        repos,
        entry,
        candidate.as_ref(),
    ));
    // Re-derive the compression command as in
    // `buildpkgonly_entry_build_env`; `None` means pairs-then-env only.
    crate::ebuild_package::refresh_entry_compression_command(
        &mut env,
        reloaded.as_ref().or(options.resolved_config.as_deref()),
    );
    env
}
