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
    /// Backlog #197: the scheduler-display mode (background + Jobs
    /// visibility). `--buildpkgonly` prints real's `Emerging (N of M)`
    /// line through the same `_status_msg` path, so the blank rule
    /// applies; its serial loop emits no `>>> Jobs:` events (a cut,
    /// stated on `StatusDisplay`), so the mode only carries the blank
    /// half here.
    pub mode: StatusMode,
}

/// Actually builds a binary package (never merges) for every entry in
/// `entries` that real `--buildpkgonly` would build -- see the module
/// doc comment for the full scope. Without `keep_going`, returns the
/// *first* failure encountered (message already includes which package
/// failed) and stops there, matching portuale's own long-established
/// default. With real `--keep-going` (real `main.py`'s own `y_or_n`
/// option, narrowed by portuale's own CLI transcription to the bare/
/// `y` form -- see `pretend.rs`'s own `keep_going` doc comment), every
/// entry is still attempted regardless of earlier failures -- safe here
/// specifically because the gate `pretend.rs` already checks before
/// calling this at all (`GraphResult::buildpkgonly_deps_unsatisfied`)
/// guarantees no entry depends on another, so unlike real portage's own
/// general `--keep-going` (which must also skip every *dependent* of a
/// failed package, tracked via real `Scheduler.py`'s own mergelist
/// recalculation), there is nothing here that a failure could ever
/// invalidate for a later entry. Failures are collected and returned
/// together at the end as a single combined error listing every one --
/// `Ok(())` only once every entry has a real binary package on disk.
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
    // The run-wide half of real `config.environ()`, once for the whole
    // run (`--buildpkgonly` has no `MergeOptions`; `entry_phase_env_tail`
    // adds the per-entry half). #37 S2.
    let run_wide = run_wide_phase_env(config);
    // Real `Scheduler._pkg_count` for this run (backlog #177).
    let progress = merge_progress_map(entries);
    // Backlog #197: real `Scheduler` owns one `JobStatusDisplay` per
    // merge run (no Jobs events fire on this path -- see `mode`).
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
        // Real `MergeListItem._start`'s per-package line (backlog #177):
        // real `--buildpkgonly` prints the same `Emerging (N of M)`
        // line (it is not fetch-only), and -- like real, whose
        // `PackageMerge._should_show_status` is false for buildpkgonly
        // -- no `Installing`/`Completed` line follows. This replaces the
        // `>>> Building binary for ...` line portuale used to print,
        // which has no real counterpart (real builds the binpkg
        // silently on stdout; only `emerge.log` records it, which
        // portuale does not write -- see the S0 table).
        let color = progress_color();
        let entry_progress = progress[idx];
        display.status(&emerging_line(entry, version, entry_progress, root, &color));
        // Real per-package `package.env` (backlog #129): `--buildpkgonly`
        // carries no `MergeOptions`, but `config` already has the
        // resolved `package_env_vars` table (`Config::package_env_vars`),
        // so this matches the same `cat/pkg-ver:slot/sub` identity the
        // merge scheduler's `entry_build_env` does, just reading `config`
        // directly instead of threading it through a `MergeOptions`.
        let cpv_slot = entry_cpv_slot(entry, version);
        // Real per-package `PORTAGE_TMPDIR` (backlog #99, #129 residue):
        // re-derive the tmpdir this entry's build directories live under
        // from its matched `package.env` value, same as the merge
        // scheduler's `entry_portage_tmpdir`. A missing matched directory
        // is real's `_check_temp_dir` failure and aborts the entry.
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
        // Real `_emerge/EbuildBuild._start_pre_clean` before the build
        // and `_buildpkgonly_success_hook_exit` (`EbuildBuild.py:525-535`)
        // after the package: `--buildpkgonly` pre-cleans like every other
        // build and post-cleans unconditionally -- the phase itself, not
        // a `noclean` gate, is what honors `keeptemp`/`keepwork` (real's
        // `_clean_exit` treats a failed clean as a failed build).
        // Backlog #42.
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
        // Real per-package `FEATURES` (backlog #130): `build_env` already
        // carries this entry's fully resolved `FEATURES` (the per-entry
        // `package.env` fold when one matched, the run-wide value
        // otherwise -- `buildpkgonly_entry_build_env`'s own doc comment),
        // so re-derive the binpkg-affecting `PackageOptions` fields from
        // it here rather than the caller's single shared, run-wide value.
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
            None => match ebuild_package::run_package(
                &path,
                root,
                portage_tmpdir,
                &per_entry_options,
                &build_env,
                use_flags,
            ) {
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

/// Real `Scheduler._allocate_config` (`Scheduler.py:1899-1913`) calls
/// `config.reload()` for every package task, and `reload()` re-reads
/// `env.d` (`config.py:2691-2699`), so a package built after a merge that
/// installed an `/etc/env.d/*` file (and ran `env-update`) sees the new
/// variables (backlog #332). Returns the reloaded config when
/// `<eroot>/etc/profile.env` changed since `config` was resolved (or last
/// reloaded), `None` otherwise -- the caller then keeps its old code path
/// byte for byte. A config without a recorded `eroot` never reloads.
pub(crate) fn env_d_reloaded(config: &portage_profile::Config) -> Option<portage_profile::Config> {
    if config.envd_eroot.as_os_str().is_empty() || !config.env_d_changed(&config.envd_eroot) {
        return None;
    }
    let mut reloaded = config.clone();
    reloaded.reload_env_d(&config.envd_eroot);
    Some(reloaded)
}

/// Backlog #333 on top of #332: real's per-task `config.reload()` also
/// feeds the task's `dblink`, whose protect object reads
/// `settings["CONFIG_PROTECT"]` / `["CONFIG_PROTECT_MASK"]`
/// (`vartree.py:2063-2072`), so a path an earlier merge of the same run
/// added to `CONFIG_PROTECT` through `/etc/env.d` is protected for the
/// later merges. The new `(config_protect, config_protect_mask)` pair
/// when `profile.env` changed since `options.resolved_config` was
/// resolved, `None` otherwise (the run-wide values stand).
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

/// [`run_buildpkgonly`]'s per-entry `config.environ()` (backlog #129):
/// `run_wide` plus `entry`'s matched `package.env` build vars
/// ([`matched_package_env_vars`], layered before the tail so an
/// incremental's base is the run-wide value, matching `entry_build_env`'s
/// order) with the per-package `FEATURES` fold ([`resolved_features_for`])
/// on top, then [`entry_phase_env_tail`]'s resolved `USE`/`IUSE_EFFECTIVE`/
/// `USE_EXPAND` and `SLOT`/repo identity rows. A free function (not a
/// closure in the loop) so it is directly unit-testable without a real
/// build, the same way `entry_build_env` already is.
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
    // Backlog #332: a package task re-reads env.d (`env_d_reloaded`).
    // `run_wide` was computed once at the top of the run; when
    // `profile.env` changed since, recompute it from the reloaded config.
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
    // Real per-package `BINPKG_COMPRESS` (backlog #147 S2): the
    // matched scalars are layered above, so re-derive the xpak pipe
    // command from them here -- the install phases save this env and
    // real `__dyn_package` reads it back through the saved
    // `${T}/environment`. Unmatched entries recompute the run-wide
    // command byte-identically (backlog #180: the re-derivation falls
    // back through the calling env to the config's own file chain, so
    // a `make.conf` value survives it).
    crate::ebuild_package::refresh_entry_compression_command(&mut build_env, Some(config));
    build_env
}
