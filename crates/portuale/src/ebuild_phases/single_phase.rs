//! Single phases, clean, instprep, and phases run from a saved environment.

use crate::ebuild_phases::*;

/// Builds the `ExecutionParameters` for a brush phase shell, redirecting
/// its stdout+stderr to `log_file` when given. Captured parallel builds
/// run through `bash` instead; this brush path is only for a forced backend.
pub(crate) fn brush_phase_params(
    shell: &mut brush_core::Shell,
    log_file: Option<&Path>,
) -> Result<(brush_core::ExecutionParameters, Option<LogPump>), String> {
    let mut pump = None;
    if let Some(path) = log_file {
        let sink = open_log_file(path)?;
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDOUT_FD, sink.out.into());
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, sink.err.into());
        // The shell owns the pipe write ends now; the caller holds the
        // guard across execution and drops the shell first.
        pump = sink.pump;
    }
    Ok((shell.default_exec_params(), pump))
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
    /// `ebuild_merge::MergeOptions::log_file`); `None` for foreground.
    pub(crate) log_file: Option<&'a Path>,
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
        // No `A`/`AA` here: these hooks run after `unpack` completed.
        run_one_phase(RunOnePhaseArgs {
            env: &env,
            root,
            phase,
            debug,
            extra_env: build_env,
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
    pub(crate) log_file: Option<&'a Path>,
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
    pub(crate) log_file: Option<&'a Path>,
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
    /// `ebuild_merge::MergeOptions::log_file`); `None` for foreground.
    pub(crate) log_file: Option<&'a Path>,
    /// Regenerates the vdb env from the live phase environment when `Some`;
    /// non-empty `refresh_features` overrides `FEATURES` for that regeneration.
    pub(crate) update_env: Option<&'a Path>,
    pub(crate) refresh_features: Option<&'a str>,
    /// Threaded into the hook env for binary-merge hooks; `None` for unmerge
    /// hooks (see `ebuild_merge::replacing_versions` for the source).
    pub(crate) replacing_versions: Option<&'a str>,
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
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        if seed {
            seed_saved_environment(&env, saved_env_bz2)?;
        }

        let mut extra_env = binary_merge_env();
        if let Some(version) = replacing_versions {
            extra_env.push(("REPLACING_VERSIONS".to_string(), version.to_string()));
        }
        if let Some(p) = update_env {
            extra_env.push(("PORTAGE_UPDATE_ENV".to_string(), p.display().to_string()));
            if let Some(features) = refresh_features.filter(|f| !f.is_empty()) {
                extra_env.push(("FEATURES".to_string(), features.to_string()));
                extra_env.push(("PORTAGE_FEATURES".to_string(), features.to_string()));
            }
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
