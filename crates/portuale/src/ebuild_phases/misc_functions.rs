//! `misc-functions.sh` steps.

use crate::ebuild_phases::*;

/// Arguments of [`run_misc_functions`].
pub(crate) struct RunMiscFunctionsArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: PhaseLog<'a>,
}

/// Runs `bin/misc-functions.sh __dyn_<mydo>` directly: unlike `doebuild` commands
/// like `"package"`, it never goes through `bin/ebuild.sh`'s phase dispatch
/// (`__ebuild_main` has no `"package"` branch). Sourcing it with `dyn_command`
/// as a positional arg is enough; no `invoke_function` call is needed.
/// Portage: doebuild.py
pub(crate) async fn run_misc_functions(args: RunMiscFunctionsArgs<'_>) -> Result<i32, String> {
    let RunMiscFunctionsArgs {
        env,
        root,
        ebuild_phase_value,
        dyn_command,
        extra_env,
        debug,
        config_root,
        shell,
        log_file,
    } = args;
    let bin_dir = bin_dir()?.to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");

    // Portage's `MiscFunctionsProcess`: sandbox-wrapped by default, with its own
    // `SANDBOX_LOG` (`sandbox-misc.log`); never `unshare`. Forces `Bash`.
    let fs_sandbox =
        fs_sandbox_requested(&features_string(extra_env)) && sandbox_binary().is_some();
    let effective_shell = if fs_sandbox {
        ShellBackend::Bash
    } else {
        shell
    };

    match effective_shell {
        ShellBackend::Brush => {
            run_misc_functions_brush(RunMiscFunctionsBrushArgs {
                env,
                root,
                ebuild_phase_value,
                dyn_command,
                extra_env,
                debug,
                bin_dir: &bin_dir,
                helpers_dir: &helpers_dir,
                config_root,
                log_file,
            })
            .await
        }
        ShellBackend::Bash => run_misc_functions_bash(RunMiscFunctionsBashArgs {
            env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env,
            debug,
            bin_dir: &bin_dir,
            helpers_dir: &helpers_dir,
            config_root,
            log_file,
            fs_sandbox,
        }),
    }
}
/// Arguments of [`run_misc_functions_brush`].
pub(crate) struct RunMiscFunctionsBrushArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: PhaseLog<'a>,
}

pub(crate) async fn run_misc_functions_brush(
    args: RunMiscFunctionsBrushArgs<'_>,
) -> Result<i32, String> {
    let RunMiscFunctionsBrushArgs {
        env,
        root,
        ebuild_phase_value,
        dyn_command,
        extra_env,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        log_file,
    } = args;
    let _umask = UmaskGuard::save();
    let mut shell = new_brush_phase_shell().await?;
    let (params, pump) = brush_phase_params(&mut shell, log_file)?;

    let setup = phase_setup_script(PhaseSetupScriptArgs {
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    });
    // Drop shell before pump (see `open_log_file`): capture the outcome, then return.
    let outcome: Result<i32, String> = async {
        shell
            .run_string(&setup, &brush_core::SourceInfo::default(), &params)
            .await
            .map_err(|e| format!("environment setup failed: {e}"))?;

        shell
            .source_script(
                bin_dir.join("misc-functions.sh"),
                [dyn_command.to_string()].into_iter(),
                &params,
            )
            .await
            .map_err(|e| format!("running {dyn_command} failed: {e}"))
            .map(|result| i32::from(u8::from(result.exit_code)))
    }
    .await;
    drop(shell);
    drop(pump);
    outcome
}

/// Arguments of [`run_misc_functions_bash`].
pub(crate) struct RunMiscFunctionsBashArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: PhaseLog<'a>,
    pub(crate) fs_sandbox: bool,
}

/// `Bash`: a genuine `bash <bin_dir>/misc-functions.sh <dyn_command>` subprocess,
/// matching Portage's `misc_sh` invocation shape. Blocking `Command`, like `fetch.rs`.
/// Portage: doebuild.py
pub(crate) fn run_misc_functions_bash(args: RunMiscFunctionsBashArgs<'_>) -> Result<i32, String> {
    let RunMiscFunctionsBashArgs {
        env,
        root,
        ebuild_phase_value,
        dyn_command,
        extra_env,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        log_file,
        fs_sandbox,
    } = args;
    let mut vars = phase_env_vars(PhaseEnvVarsArgs {
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    });
    if fs_sandbox {
        // Portage: MiscFunctionsProcess._spawn — separate log, sandbox enabled the
        // way `bin/ebuild.sh` reads it (`SANDBOX_DISABLED`).
        for (k, v) in vars.iter_mut() {
            match k.as_str() {
                "SANDBOX_LOG" => *v = env.t().join("sandbox-misc.log").display().to_string(),
                "SANDBOX_DISABLED" => *v = "0".to_string(),
                _ => {}
            }
        }
    }
    let iso = Isolation {
        fs_sandbox,
        ..Isolation::default()
    };
    spawn_bash_script(SpawnBashScriptArgs {
        script: &bin_dir.join("misc-functions.sh"),
        argv_word: dyn_command,
        vars,
        iso,
        log_file,
        context: dyn_command,
        interactive: false,
    })
}

/// One process-wide multi-threaded tokio runtime (Portage runs a whole invocation
/// on a single asyncio loop; per-phase runtimes paid setup/teardown per package).
/// `OnceLock`-shared: none of the entry points is called from inside an existing
/// tokio context, so lazy reuse is safe. Never torn down (CLI process).
pub(crate) fn shared_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to start async runtime: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Arguments of [`run_misc_function`].
pub(crate) struct RunMiscFunctionArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    /// Tee both streams to our stdout and the build log, like Portage's
    /// `_spawn_phase` logging pipe; `false` inherits our stdio.
    pub(crate) foreground: bool,
}

/// Sync entry point for `ebuild_package`'s `__dyn_package` call; mirrors
/// `run_single_phase`.
pub(crate) fn run_misc_function(args: RunMiscFunctionArgs<'_>) -> Result<i32, String> {
    let RunMiscFunctionArgs {
        ebuild_path,
        portage_tmpdir,
        root,
        ebuild_phase_value,
        dyn_command,
        extra_env,
        debug,
        config_root,
        shell,
        foreground,
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        // Portage: _emerge/EbuildBinpkg.py -- the misc-functions child's
        // stdout and stderr share the phase logging pipe, so `__vecho`'s
        // stderr lines reach stdout.
        let foreground_path: Option<PathBuf>;
        let mut extra_env = extra_env.to_vec();
        let log_file = if foreground {
            let (logdir, sep) = crate::emerge_build::foreground_log_env();
            let path = crate::emerge_build::foreground_log_path(
                crate::emerge_build::ForegroundLogArgs {
                    t_dir: &env.t(),
                    builddir: env.portage_builddir(),
                    category: &env.category,
                    pf: &env.split.pf,
                    features: &features_string(&extra_env),
                    logdir: logdir.as_deref(),
                    sep: &sep,
                },
            );
            extra_env.push(("PORTAGE_LOG_FILE".to_string(), path.display().to_string()));
            foreground_path = Some(path);
            PhaseLog::Tee(foreground_path.as_deref().unwrap())
        } else {
            PhaseLog::Inherit
        };
        run_misc_functions(RunMiscFunctionsArgs {
            env: &env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env: &extra_env,
            debug,
            config_root,
            shell,
            log_file,
        })
        .await
    })
}
