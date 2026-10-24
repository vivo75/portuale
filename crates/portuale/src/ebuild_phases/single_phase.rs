//! Single phases, clean, instprep, and phases run from a saved environment.

use crate::ebuild_phases::*;

/// Builds the `ExecutionParameters` for a brush phase shell, redirecting
/// its stdout+stderr per `log`: appended to the file for `Capture`
/// (parallel builds run through `bash` instead; this brush path is only
/// for a forced backend), teed to our stdout and the file for `Tee`.
pub(crate) fn brush_phase_params(
    shell: &mut brush_core::Shell,
    log: PhaseLog<'_>,
) -> Result<(brush_core::ExecutionParameters, TeePump), String> {
    let mut guard = TeePump {
        thread: None,
        pump: None,
    };
    match log {
        PhaseLog::Capture(path) => {
            let sink = open_log_file(path)?;
            shell
                .open_files_mut()
                .set_fd(brush_core::openfiles::OpenFiles::STDOUT_FD, sink.out.into());
            shell
                .open_files_mut()
                .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, sink.err.into());
            // The shell owns the pipe write ends now; the caller holds the
            // guard across execution and drops the shell first.
            guard.pump = sink.pump;
        }
        PhaseLog::Tee(path) => {
            let (write_end, pump) = open_tee_log(path)?;
            let err_end = write_end
                .try_clone()
                .map_err(|e| format!("dup for brush stderr failed: {e}"))?;
            shell.open_files_mut().set_fd(
                brush_core::openfiles::OpenFiles::STDOUT_FD,
                write_end.into(),
            );
            shell
                .open_files_mut()
                .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, err_end.into());
            guard = pump;
        }
        // `Foreground` resolves upstream (`run_commands_async`,
        // `run_single_phase`); reaching here would tee nowhere, so
        // inherit like `Inherit` rather than misrouting the stream.
        PhaseLog::Inherit | PhaseLog::Foreground => {}
    }
    Ok((shell.default_exec_params(), guard))
}

/// Arguments of [`run_single_phase`].
pub(crate) struct RunSinglePhaseArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    /// Resolved `USE` for an `emerge` merge's own hooks (see
    /// `run_commands_async`); `&[]` otherwise.
    pub(crate) build_env: &'a [(String, String)],
    /// Background-mode `PORTAGE_LOG_FILE`, extended to this hook (see
    /// `ebuild_merge::MergeOptions::log_file`); `Foreground` tees through
    /// `${T}/build.log` like a direct `ebuild` run, `Inherit` elsewhere.
    pub(crate) log_file: PhaseLog<'a>,
}

/// Runs exactly `phase`, with no `actionmap_deps` prerequisite chain.
/// Portage: dbapi/vartree.py, dblink.treewalk.
/// Safe to call even when the ebuild defines neither hook function.
pub(crate) fn run_single_phase(args: RunSinglePhaseArgs<'_>) -> Result<i32, String> {
    let RunSinglePhaseArgs {
        ebuild_path,
        phase,
        root,
        portage_tmpdir,
        debug,
        config_root,
        shell,
        build_env,
        log_file,
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        // Every caller of this entry point is a source-merge hook or a
        // direct `ebuild <file> <phase>` run (both `EMERGE_FROM=ebuild`):
        // default `MERGE_TYPE=source` unless the caller threaded one.
        // `exports_merge_type` (EAPI 4+) is enforced centrally in
        // `apply_eapi_phase_env`. Saved-env unmerge hooks never come
        // through here (they use `run_phase_from_saved_env` with no
        // merge type); `ebuild <file> unmerge` shares the direct shape
        // (portuale-only command, no oracle cell).
        // Portage: doebuild.py, doebuild (`tree == "porttree"`)
        let mut extra_env = build_env.to_vec();
        if !extra_env.iter().any(|(k, _)| k == "MERGE_TYPE") {
            extra_env.push(("MERGE_TYPE".to_string(), "source".to_string()));
        }
        // A foreground run resolves `${T}/build.log` here (the path needs
        // the environment) and tees the phase through it; `PORTAGE_LOG_FILE`
        // rides the phase env so `die` prints its build-log line.
        // Portage: doebuild.py, _spawn_phase; prepare_build_dirs.py
        let foreground_path: Option<PathBuf>;
        let log_file = match log_file {
            PhaseLog::Foreground => {
                let (logdir, sep) = crate::emerge_build::foreground_log_env();
                foreground_path = Some(crate::emerge_build::foreground_log_path(
                    crate::emerge_build::ForegroundLogArgs {
                        t_dir: &env.t(),
                        builddir: env.portage_builddir(),
                        category: &env.category,
                        pf: &env.split.pf,
                        features: &features_string(&extra_env),
                        logdir: logdir.as_deref(),
                        sep: &sep,
                    },
                ));
                PhaseLog::Tee(foreground_path.as_deref().unwrap())
            }
            resolved => resolved,
        };
        if let PhaseLog::Tee(path) | PhaseLog::Capture(path) = log_file {
            extra_env.push(("PORTAGE_LOG_FILE".to_string(), path.display().to_string()));
        }
        // `A`/`AA` come from `apply_eapi_phase_env` (Portage's
        // `doebuild_environment` computes them for every non-`depend`
        // phase, including these standalone hooks).
        run_one_phase(RunOnePhaseArgs {
            env: &env,
            root,
            phase,
            debug,
            extra_env: &extra_env,
            config_root,
            shell,
            log_file,
        })
        .await
    })
}

/// Arguments of [`run_clean`].
pub(crate) struct RunCleanArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) build_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: PhaseLog<'a>,
}

/// The `clean` phase: removes the image, the resume markers, `${T}` and
/// `WORKDIR`/`build-info`/`files` unless `FEATURES=keeptemp`/`keepwork`.
/// Portage: bin/phase-functions.sh, __dyn_clean.
pub(crate) fn run_clean(args: RunCleanArgs<'_>) -> Result<i32, String> {
    let RunCleanArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        build_env,
        debug,
        config_root,
        shell,
        log_file,
    } = args;
    run_single_phase(RunSinglePhaseArgs {
        ebuild_path,
        phase: "clean",
        root,
        portage_tmpdir,
        debug,
        config_root,
        shell,
        build_env,
        log_file,
    })
}

/// Seeds `${T}/environment` from a binpkg's saved env, plus the `.raw` marker.
/// Portage: _emerge/BinpkgEnvExtractor.
pub(crate) fn seed_saved_environment(
    env: &Environment,
    saved_env_bz2: &Path,
) -> Result<(), String> {
    let dest_env = env.t().join("environment");
    let out =
        std::fs::File::create(&dest_env).map_err(|e| format!("{}: {e}", dest_env.display()))?;
    let status = std::process::Command::new("bzip2")
        .args(["-d", "-c", "--"])
        .arg(saved_env_bz2)
        .stdout(out)
        .status()
        .map_err(|e| format!("failed to spawn bzip2: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&dest_env);
        return Err(format!(
            "bzip2 failed to decompress {} ({status})",
            saved_env_bz2.display()
        ));
    }
    std::fs::write(env.t().join("environment.raw"), [])
        .map_err(|e| format!("{}: {e}", env.t().join("environment.raw").display()))
}

/// `EMERGE_FROM`/`MERGE_TYPE=binary` for a binary merge: eclasses gate
/// build-time work on `MERGE_TYPE`, which `EMERGE_FROM` alone doesn't cover.
pub(crate) fn binary_merge_env() -> Vec<(String, String)> {
    vec![
        ("EMERGE_FROM".to_string(), "binary".to_string()),
        ("MERGE_TYPE".to_string(), "binary".to_string()),
    ]
}

/// Arguments of [`run_instprep`].
pub(crate) struct RunInstprepArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) saved_env_bz2: Option<&'a Path>,
    /// Extract `${T}/environment` first: `true` for the first phase of a
    /// binary merge, `false` once an earlier hook seeded it.
    pub(crate) seed: bool,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) build_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: PhaseLog<'a>,
    /// The binary merge's repo for the hook env (`PORTAGE_REPO_NAME`),
    /// like [`RunPhaseFromSavedEnvArgs::repo_name`].
    pub(crate) repo_name: Option<&'a str>,
}

/// The `instprep` internal phase on every merge, before `INSTALL_MASK`,
/// collision-protect, `pkg_preinst` or any file copy (Portage:
/// dblink.treewalk + `__dyn_instprep`). `saved_env_bz2` seeds binary
/// merges; `build_env` carries the phase env the gates read.
pub(crate) fn run_instprep(args: RunInstprepArgs<'_>) -> Result<i32, String> {
    let RunInstprepArgs {
        ebuild_path,
        saved_env_bz2,
        seed,
        root,
        portage_tmpdir,
        build_env,
        debug,
        config_root,
        shell,
        log_file,
        repo_name,
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        let mut extra_env = build_env.to_vec();
        if let Some(saved) = saved_env_bz2 {
            if seed {
                seed_saved_environment(&env, saved)?;
            }
            extra_env.extend(binary_merge_env());
            // Same repo as the other binary hooks (see
            // `run_phase_from_saved_env`); the saved env alone never
            // carries it.
            if let Some(repo) = repo_name {
                extra_env.push(("PORTAGE_REPO_NAME".to_string(), repo.to_string()));
            }
        }
        run_misc_functions(RunMiscFunctionsArgs {
            env: &env,
            root,
            ebuild_phase_value: "instprep",
            dyn_command: "__dyn_instprep",
            extra_env: &extra_env,
            debug,
            config_root,
            shell,
            log_file,
        })
        .await
    })
}

/// Arguments of [`run_phase_from_saved_env`].
pub(crate) struct RunPhaseFromSavedEnvArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) saved_env_bz2: &'a Path,
    /// Extract `${T}/environment` first: `true` for the first hook of a
    /// package or an unmerge, `false` for the following hooks of one merge.
    pub(crate) seed: bool,
    pub(crate) phase: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    /// Background-mode `PORTAGE_LOG_FILE`, extended to this hook (see
    /// `ebuild_merge::MergeOptions::log_file`); `Inherit` for foreground
    /// hooks (unmerge) and direct-path `clean`.
    pub(crate) log_file: PhaseLog<'a>,
    /// Regenerates the vdb env from the live phase environment when `Some`;
    /// non-empty `refresh_features` overrides `FEATURES` for that regeneration.
    pub(crate) update_env: Option<&'a Path>,
    pub(crate) refresh_features: Option<&'a str>,
    /// Threaded into the hook env for binary-merge hooks; `None` for unmerge
    /// hooks (see `ebuild_merge::replacing_versions` for the source).
    pub(crate) replacing_versions: Option<&'a str>,
    /// The merge type for a binary-merge hook (`Some("binary")`, which also
    /// sets `EMERGE_FROM=binary`); `None` for an unmerge hook, where both
    /// stay unset and `ebuild.sh` re-creates them readonly-empty from the
    /// filtered saved env (Portage: `dblink.unmerge` never sets them).
    pub(crate) merge_type: Option<&'a str>,
    /// Threaded into the hook env for a replaced version's unmerge hooks
    /// (`Some` with the replacing version); `None` for a standalone
    /// unmerge and for binary-merge hooks.
    /// Portage: vartree.py, `dblink.treewalk` (`REPLACED_BY_VERSION`).
    pub(crate) replaced_by_version: Option<&'a str>,
    /// The binary merge's repo for the hook env (`PORTAGE_REPO_NAME`,
    /// feeding the `die` `::repo` suffix); `None` for unmerge hooks,
    /// which keep today's behavior.
    /// Portage: doebuild.py, `doebuild_environment`
    /// (`configdict["pkg"]["PORTAGE_REPO_NAME"] = repo.name`).
    pub(crate) repo_name: Option<&'a str>,
    /// The merge's whitelisted config environment for a binary-merge hook
    /// (empty for unmerge hooks): `config.environ()` reaches every phase,
    /// not only `instprep`.
    /// Portage: config.py, environ
    pub(crate) build_env: &'a [(String, String)],
}

/// Like `run_single_phase`, but seeds `${T}/environment` from a binary
/// package's saved env, so hooks run against the build-time environment.
/// `EMERGE_FROM=binary` keeps `pkg_setup` on the saved env.
/// Portage: _emerge/BinpkgEnvExtractor.
pub(crate) fn run_phase_from_saved_env(args: RunPhaseFromSavedEnvArgs<'_>) -> Result<i32, String> {
    let RunPhaseFromSavedEnvArgs {
        ebuild_path,
        saved_env_bz2,
        seed,
        phase,
        root,
        portage_tmpdir,
        debug,
        config_root,
        shell,
        log_file,
        update_env,
        refresh_features,
        replacing_versions,
        merge_type,
        replaced_by_version,
        repo_name,
        build_env,
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        if seed {
            seed_saved_environment(&env, saved_env_bz2)?;
        }

        // Binary-merge hooks keep `EMERGE_FROM`/`MERGE_TYPE=binary` (which
        // `EMERGE_FROM` alone doesn't cover for eclasses gating on
        // `MERGE_TYPE`); unmerge hooks carry neither. Both
        // `exports_merge_type` and the replace-var phase gates are
        // enforced centrally in `apply_eapi_phase_env`.
        let mut extra_env = build_env.to_vec();
        if merge_type.is_some() {
            extra_env.extend(binary_merge_env());
        }
        // The binary merge's repo, like `doebuild_environment`'s
        // `configdict["pkg"]` assignment; unmerge hooks keep it unset.
        // Portage: doebuild.py, doebuild_environment
        if let Some(repo) = repo_name {
            extra_env.push(("PORTAGE_REPO_NAME".to_string(), repo.to_string()));
        }
        if let Some(version) = replacing_versions {
            extra_env.push(("REPLACING_VERSIONS".to_string(), version.to_string()));
        }
        if let Some(version) = replaced_by_version {
            extra_env.push(("REPLACED_BY_VERSION".to_string(), version.to_string()));
        }
        if let Some(p) = update_env {
            extra_env.push(("PORTAGE_UPDATE_ENV".to_string(), p.display().to_string()));
            if let Some(features) = refresh_features.filter(|f| !f.is_empty()) {
                extra_env.push(("FEATURES".to_string(), features.to_string()));
                extra_env.push(("PORTAGE_FEATURES".to_string(), features.to_string()));
            }
        }
        // Like a source phase: a foreground hook tees through `${T}/build.log`
        // and `PORTAGE_LOG_FILE` rides the env, so `die` prints its
        // `The complete build log is located at` line.
        // Portage: _emerge/Binpkg.py (PORTAGE_LOG_FILE); doebuild.py, _spawn_phase
        let foreground_path: Option<PathBuf>;
        let log_file = match log_file {
            PhaseLog::Foreground => {
                let (logdir, sep) = crate::emerge_build::foreground_log_env();
                foreground_path = Some(crate::emerge_build::foreground_log_path(
                    crate::emerge_build::ForegroundLogArgs {
                        t_dir: &env.t(),
                        builddir: env.portage_builddir(),
                        category: &env.category,
                        pf: &env.split.pf,
                        features: &features_string(&extra_env),
                        logdir: logdir.as_deref(),
                        sep: &sep,
                    },
                ));
                PhaseLog::Tee(foreground_path.as_deref().unwrap())
            }
            resolved => resolved,
        };
        if let PhaseLog::Tee(path) | PhaseLog::Capture(path) = log_file {
            extra_env.push(("PORTAGE_LOG_FILE".to_string(), path.display().to_string()));
        }
        run_one_phase(RunOnePhaseArgs {
            env: &env,
            root,
            phase,
            debug,
            extra_env: &extra_env,
            config_root,
            shell,
            log_file,
        })
        .await
    })
}
