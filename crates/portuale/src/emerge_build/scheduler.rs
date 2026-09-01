//! The parallel build scheduler (`--jobs`).

use crate::emerge_build::*;

/// Arguments of [`build_one_source_entry`].
pub(crate) struct BuildOneSourceEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub(crate) capture_log: bool,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a StatusDisplay,
}

/// The `install` phase chain and any `--buildpkg` binpkg for one source
/// entry, without the vdb merge. Returns the located ebuild path for
/// `merge_one_built_entry` to reuse. Safe to run from a scheduler worker
/// thread: every build gets its own `${PORTAGE_BUILDDIR}` and the
/// environment is passed explicitly. With `capture_log` the `install`
/// output goes to `${T}/build.log`, whose tail is folded into the error
/// on failure.
// Portage: EbuildBuild.py, EbuildBuild; EbuildBinpkg.py, EbuildBinpkg
pub(crate) fn build_one_source_entry(args: BuildOneSourceEntryArgs<'_>) -> Result<PathBuf, String> {
    let BuildOneSourceEntryArgs {
        entry,
        repos,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        capture_log,
        progress,
        display,
    } = args;
    let (cp, version) = scheduler_cp_version(entry)?;
    let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, &version) else {
        return Err(format!(
            "{cp}-{version}: could not locate its own ebuild file \
             (repo layout changed since resolution?)"
        ));
    };
    let path = ebuild_path(&candidate, &entry.category, &entry.package, &version);
    // The build half prints the per-package line when the build starts.
    let color = progress_color();
    display.status(&emerging_line(entry, &version, progress, root, &color));
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();

    let log_path = capture_log.then(|| {
        build_log_path(
            portage_tmpdir,
            &entry.category,
            &entry.package,
            &version,
            &resolved_features(options),
        )
    });
    if let Some(lp) = &log_path {
        // Truncate, don't delete: with `PORTAGE_LOGDIR` set `lp` is a
        // symlink, and deleting it would strand the new log in `${T}`.
        // `set_len(0)` follows the link to the file.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(lp) {
            let _ = f.set_len(0);
        }
    }
    // A captured parallel build runs through `bash`, whose stdout+stderr
    // redirect to `build.log` cleanly at the OS level.
    let shell = if log_path.is_some() {
        ebuild_phases::ShellBackend::Bash
    } else {
        options.shell
    };
    let build_env = entry_build_env(options, entry, repos);
    // The `clean` phase runs before every build, unconditionally.
    let clean_status = ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        build_env: &build_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: log_path.as_deref(),
    })?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    let status = ebuild_phases::run_commands_logged(ebuild_phases::RunCommandsLoggedArgs {
        ebuild_path: &path,
        commands: &["install"],
        root,
        portage_tmpdir,
        distdir: &options.distdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: log_path.as_deref(),
        build_env: &build_env,
    })?;
    if status != 0 {
        let mut msg = format!("{cp}-{version}: build failed ({status})");
        if let Some(lp) = &log_path {
            msg.push_str(&format!(
                "\n----- last lines of {} -----\n{}\n----------",
                lp.display(),
                tail_of(lp, 40)
            ));
        }
        return Err(msg);
    }
    if let Some(package_options) = buildpkg {
        println!(">>> Building package for {cp}-{version}...");
        // The binpkg's `USE` field is the same resolved flag set the
        // `install` phase just ran with, reused verbatim so the two can
        // never drift apart.
        let use_flags = build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // Re-derive the binpkg-affecting tokens from the per-entry folded
        // list; `None` (no match) keeps the already-resolved value.
        let mut per_entry_package_options = package_options.clone();
        let per_entry_features = entry_resolved_features(
            options,
            entry,
            &std::env::var("FEATURES").unwrap_or_default(),
        );
        if let Some(features) = &per_entry_features {
            per_entry_package_options.set_resolved_features(features);
        }
        // The layout stays run-wide; the `BUILD_ID` export follows this
        // entry's own token.
        let per_entry_binpkg_multi_instance = per_entry_features
            .as_deref()
            .map(|features| {
                features
                    .split_whitespace()
                    .any(|t| t == "binpkg-multi-instance")
            })
            .unwrap_or(per_entry_package_options.binpkg_multi_instance);
        let status =
            ebuild_package::package_after_install(ebuild_package::PackageAfterInstallArgs {
                ebuild_path: &path,
                root,
                portage_tmpdir,
                options: &per_entry_package_options,
                use_flags,
                per_entry_binpkg_multi_instance,
            })?;
        if status != 0 {
            return Err(format!("{cp}-{version}: binpkg build failed ({status})"));
        }
    }
    Ok(path)
}

/// Arguments of [`merge_one_built_entry`].
pub(crate) struct MergeOneBuiltEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) ebuild_path: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a StatusDisplay,
}

/// The vdb merge for a source entry whose `install` phase already ran.
/// Always serialized; only the build above runs in parallel. Reuses
/// `run_qmerge` (same-slot replace of an upgraded/reinstalled version
/// included).
pub(crate) fn merge_one_built_entry(args: MergeOneBuiltEntryArgs<'_>) -> Result<(), String> {
    let MergeOneBuiltEntryArgs {
        entry,
        repos,
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        progress,
        display,
    } = args;
    let (cp, version) = scheduler_cp_version(entry)?;
    // `pkg_preinst`/`pkg_postinst` see this entry's resolved `USE` too.
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry, repos);
    if let Some((protect, mask)) = reloaded_config_protect(options) {
        per_entry.config_protect = protect;
        per_entry.config_protect_mask = mask;
    }
    // The merge half's gates read the per-entry folded list, not the
    // run-wide one; `None` (no match) keeps the run-wide value.
    if let Some(features) = entry_resolved_features(
        options,
        entry,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        per_entry.set_resolved_features(&features);
    }
    // The merge half runs under the same resolved root the build half
    // used (resolved again here to keep the `portage_tmpdir` contract).
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    // Only reached once the build half captured `install` output here, so
    // `pkg_preinst`/`pkg_postinst` output appends to the same continuous log.
    per_entry.log_file = Some(build_log_path(
        portage_tmpdir,
        &entry.category,
        &entry.package,
        &version,
        &resolved_features(options),
    ));
    // The merge half prints its per-package line before the serialized
    // vdb merge.
    let color = progress_color();
    display.status(&installing_line(entry, &version, progress, root, &color));
    let status = ebuild_merge::run_qmerge(ebuild_path, root, portage_tmpdir, &per_entry)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
    }
    // The `clean` phase runs after a successful merge unless
    // `FEATURES=noclean`. `run_qmerge` itself deliberately does not clean
    // (`doebuild qmerge` implies noclean), so this is where the split gets
    // the post-merge behavior.
    if !ebuild_merge::feature_enabled(&per_entry, "noclean") {
        ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
            ebuild_path,
            root,
            portage_tmpdir,
            build_env: &per_entry.build_env,
            debug: per_entry.debug,
            config_root: &per_entry.config_root,
            shell: per_entry.shell,
            log_file: per_entry.log_file.as_deref(),
        })?;
    }
    // The `Completed (N of M) cpv::repo` line.
    display.status(&completed_line(entry, &version, progress, root, &color));
    Ok(())
}

/// Runs up to `jobs` package *builds* concurrently, dispatching a build
/// only once every dependency in the merge set is already merged, and
/// serializing the vdb merge step. `jobs == usize::MAX` (a bare `--jobs`)
/// means "as many as the graph allows".
///
/// The `--load-average` gate reads the 1-minute load average, or `0.0` when
/// unreadable (which disables the throttle rather than stalling).
pub(crate) fn system_loadavg_1min() -> f64 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}
