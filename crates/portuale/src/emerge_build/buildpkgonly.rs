//! `--buildpkgonly`: build binary packages without merging.

use crate::emerge_build::*;

/// Arguments of [`run_buildpkgonly`].
pub struct RunBuildpkgonlyArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a portage_profile::Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a PackageOptions,
    pub keep_going: bool,
    /// Scheduler-display mode (background + Jobs visibility only).
    pub mode: StatusMode,
}

/// Builds a binary package (never merges) for every entry Portage's
/// `--buildpkgonly` would build. Without `keep_going` returns the first
/// failure and stops; with it every entry is attempted (safe: the pretend
/// gate guarantees no entry depends on another) and failures are returned
/// together as one combined error.
pub fn run_buildpkgonly(args: RunBuildpkgonlyArgs<'_>) -> Result<(), String> {
    let RunBuildpkgonlyArgs {
        entries,
        config,
        repos,
        root,
        portage_tmpdir,
        options,
        keep_going,
        mode,
    } = args;
    // The run-wide half of `config.environ()`, once for the whole run.
    let run_wide = run_wide_phase_env(config);
    let progress = merge_progress_map(entries);
    // One `JobStatusDisplay` per merge run; no Jobs events fire here.
    let total_builds = entries.iter().filter(|e| scheduler_needs_build(e)).count();
    let display = StatusDisplay::new(mode, total_builds, progress_color());
    let mut failures = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        if entry.source == CandidateSource::Binary {
            continue;
        }
        let Some(version) = entry_version(&entry.outcome) else {
            continue;
        };
        let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, version)
        else {
            let failure = format!(
                "{}/{}-{version}: could not locate its own ebuild file \
                 (repo layout changed since resolution?)",
                entry.category, entry.package
            );
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        };
        let path = ebuild_path(&candidate, &entry.category, &entry.package, version);
        // Portage prints the same `Emerging (N of M)` line here and no
        // `Installing`/`Completed` line afterwards.
        let color = progress_color();
        let entry_progress = progress[idx];
        display.status(&emerging_line(entry, version, entry_progress, root, &color));
        // `--buildpkgonly` carries no `MergeOptions`; match the same
        // `cat/pkg-ver:slot/sub` identity against `config` directly.
        let cpv_slot = entry_cpv_slot(entry, version);
        // Re-derive this entry's tmpdir from its matched `package.env`
        // value; a missing matched directory aborts the entry.
        let process_tmpdir = std::env::var_os("PORTAGE_TMPDIR").map(PathBuf::from);
        let entry_tmpdir = match crate::ebuild_phases::resolve_entry_portage_tmpdir(
            &config.package_env_vars,
            &cpv_slot,
            portage_tmpdir,
            process_tmpdir.as_deref(),
        ) {
            Ok(dir) => dir,
            Err(e) => {
                let failure = format!("{}/{}-{version}: {e}", entry.category, entry.package);
                if keep_going {
                    failures.push(failure);
                    continue;
                }
                return Err(failure);
            }
        };
        let portage_tmpdir = entry_tmpdir.as_path();
        let build_env = buildpkgonly_entry_build_env(BuildpkgonlyEntryBuildEnvArgs {
            config,
            repos,
            entry,
            candidate: &candidate,
            cpv_slot: &cpv_slot,
            run_wide: &run_wide,
        });
        let use_flags = build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // The pre/post `clean` phases run unconditionally around the
        // build; a failed clean fails the build.
        let clean_failure = |phase: &str| -> Option<String> {
            match ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
                ebuild_path: &path,
                root,
                portage_tmpdir,
                build_env: &build_env,
                debug: options.debug,
                config_root: &options.config_root,
                shell: options.shell,
                log_file: None,
            }) {
                Ok(0) => None,
                Ok(status) => Some(format!(
                    "{}/{}-{version}: {phase} clean failed ({status})",
                    entry.category, entry.package
                )),
                Err(e) => Some(format!(
                    "{}/{}-{version}: {phase} clean failed: {e}",
                    entry.category, entry.package
                )),
            }
        };
        // Re-derive the binpkg-affecting `PackageOptions` fields from this
        // entry's own resolved `FEATURES`, not the shared run-wide value.
        let mut per_entry_options = options.clone();
        if let Some(features) = build_env
            .iter()
            .rev()
            .find(|(k, _)| k == "FEATURES")
            .map(|(_, v)| v.as_str())
        {
            per_entry_options.set_resolved_features(features);
        }
        let failure = match clean_failure("pre") {
            Some(failure) => Some(failure),
            None => match ebuild_package::run_package(ebuild_package::RunPackageArgs {
                ebuild_path: &path,
                root,
                portage_tmpdir,
                options: &per_entry_options,
                build_env: &build_env,
                use_flags,
            }) {
                Ok(0) => clean_failure("post"),
                Ok(_) => Some(format!(
                    "{}/{}-{version}: build failed",
                    entry.category, entry.package
                )),
                Err(e) => Some(format!(
                    "{}/{}-{version}: {e}",
                    entry.category, entry.package
                )),
            },
        };
        if let Some(failure) = failure {
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} package(s) failed to build (--keep-going):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

/// Portage re-reads `env.d` for every package task: returns the reloaded
/// config when `<eroot>/etc/profile.env` changed since `config` was
/// resolved, `None` otherwise (the caller keeps its old path).
// Portage: Scheduler.py, _allocate_config
pub(crate) fn env_d_reloaded(config: &portage_profile::Config) -> Option<portage_profile::Config> {
    if config.envd_eroot.as_os_str().is_empty() || !config.env_d_changed(&config.envd_eroot) {
        return None;
    }
    let mut reloaded = config.clone();
    reloaded.reload_env_d(&config.envd_eroot);
    Some(reloaded)
}

/// The reloaded `(CONFIG_PROTECT, CONFIG_PROTECT_MASK)` pair when
/// `profile.env` changed since the run config was resolved, `None`
/// otherwise (the run-wide values stand).
pub(crate) fn reloaded_config_protect(
    options: &ebuild_merge::MergeOptions,
) -> Option<(String, String)> {
    let reloaded = options
        .resolved_config
        .as_deref()
        .and_then(env_d_reloaded)?;
    let mut probe = ebuild_merge::MergeOptions {
        config_protect: options.config_protect.clone(),
        config_protect_mask: options.config_protect_mask.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    probe.apply_config_protect(&reloaded);
    Some((probe.config_protect, probe.config_protect_mask))
}

/// Arguments of [`buildpkgonly_entry_build_env`].
pub(crate) struct BuildpkgonlyEntryBuildEnvArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entry: &'a GraphEntry,
    pub(crate) candidate: &'a Candidate,
    pub(crate) cpv_slot: &'a str,
    pub(crate) run_wide: &'a [(String, String)],
}

/// [`run_buildpkgonly`]'s per-entry build env: `run_wide` plus the
/// entry's matched `package.env` build vars with the per-package `FEATURES`
/// fold on top, then [`entry_phase_env_tail`]. A free function (not a loop
/// closure) so it is directly unit-testable without a build.
pub(crate) fn buildpkgonly_entry_build_env(
    args: BuildpkgonlyEntryBuildEnvArgs<'_>,
) -> Vec<(String, String)> {
    let BuildpkgonlyEntryBuildEnvArgs {
        config,
        repos,
        entry,
        candidate,
        cpv_slot,
        run_wide,
    } = args;
    // A package task re-reads env.d; recompute `run_wide` from the
    // reloaded config when `profile.env` changed.
    let reloaded = env_d_reloaded(config);
    let (config, run_wide) = match &reloaded {
        Some(r) => (r, run_wide_phase_env(r)),
        None => (config, run_wide.to_vec()),
    };
    let run_wide = run_wide.as_slice();
    let profile_only_variables = config
        .resolved_incremental("PROFILE_ONLY_VARIABLES")
        .unwrap_or_default();
    let mut build_env = run_wide.to_vec();
    build_env.extend(matched_package_env_vars(
        &config.package_env_vars,
        cpv_slot,
        &profile_only_variables,
        run_wide,
    ));
    let run_wide_features = run_wide
        .iter()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let calling_features = std::env::var("FEATURES").unwrap_or_default();
    if let Some(features) = resolved_features_for(
        &config.package_env_vars,
        cpv_slot,
        run_wide_features,
        &calling_features,
    ) {
        build_env.push(("FEATURES".to_string(), features.clone()));
        build_env.push(("PORTAGE_FEATURES".to_string(), features));
    }
    build_env.extend(entry_phase_env_tail(
        Some(config),
        repos,
        entry,
        Some(candidate),
    ));
    // A `--buildpkgonly` build is neither a source merge nor a binary
    // install (`EbuildBuild` sets exactly this; the emerge scheduler sets
    // `source` the same way for real merges). `exports_merge_type`
    // (EAPI 4+) is enforced centrally in `apply_eapi_phase_env`.
    // Portage: _emerge/EbuildBuild.py
    if !build_env.iter().any(|(k, _)| k == "MERGE_TYPE") {
        build_env.push(("MERGE_TYPE".to_string(), "buildonly".to_string()));
    }
    // Re-derive the xpak pipe command from the layered env, as the
    // install phases save it and `__dyn_package` reads it back through
    // `${T}/environment`.
    crate::ebuild_package::refresh_entry_compression_command(&mut build_env, Some(config));
    build_env
}
