//! `misc-functions.sh` steps.

use crate::ebuild_phases::*;

/// Real `bin/misc-functions.sh`'s own invocation shape -- unlike
/// `doebuild()` invokes commands like `"package"` as a *separate*
/// script, `bin/misc-functions.sh __dyn_<mydo>` (real
/// `lib/portage/package/ebuild/doebuild.py`'s own `misc_sh = ... +
/// " __dyn_%s"`), not through `bin/ebuild.sh`'s own phase dispatch at
/// all -- confirmed by reading it: `bin/phase-functions.sh`'s own
/// `__ebuild_main` case statement has no `"package"` branch whatsoever.
/// `misc-functions.sh` itself sources `bin/ebuild.sh` (inheriting the
/// same environment/ebuild-sourcing portuale's own `run_one_phase`
/// already relies on), captures its own positional args into
/// `MISC_FUNCTIONS_ARGS` *before* sourcing (so `ebuild.sh`'s own arg
/// handling never sees them), then its own tail unconditionally runs
/// `for x in ${MISC_FUNCTIONS_ARGS}; do ${x}; done` -- i.e. sourcing it
/// with `dyn_command` as a positional arg is enough to invoke it
/// directly; no separate `invoke_function` call is needed the way
/// `run_one_phase`'s own explicit `__ebuild_main` call is.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) async fn run_misc_functions(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let bin_dir = bin_dir()?.to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");

    // Real `_emerge.MiscFunctionsProcess`: `bin/misc-functions.sh` runs
    // `sandbox`-wrapped by default (`free = False` unless
    // `ld_preload_sandbox` says otherwise) -- but with its *own*
    // `SANDBOX_LOG` (`sandbox-misc.log`) so a QA-check violation doesn't
    // clobber the real phase's log. No `unshare` (real
    // `_PostPhaseCommands` passes only `ld_preload_sandbox`, never
    // `networked`). This forces the `Bash` backend, same as a
    // `sandbox`-wrapped phase.
    let fs_sandbox =
        fs_sandbox_requested(&features_string(extra_env)) && sandbox_binary().is_some();
    let effective_shell = if fs_sandbox {
        ShellBackend::Bash
    } else {
        shell
    };

    match effective_shell {
        ShellBackend::Brush => {
            run_misc_functions_brush(
                env,
                root,
                ebuild_phase_value,
                dyn_command,
                extra_env,
                debug,
                &bin_dir,
                &helpers_dir,
                config_root,
                log_file,
            )
            .await
        }
        ShellBackend::Bash => run_misc_functions_bash(
            env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env,
            debug,
            &bin_dir,
            &helpers_dir,
            config_root,
            log_file,
            fs_sandbox,
        ),
    }
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) async fn run_misc_functions_brush(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let _umask = UmaskGuard::save();
    let mut shell = new_brush_phase_shell().await?;
    let (params, pump) = brush_phase_params(&mut shell, log_file)?;

    let setup = phase_setup_script(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    // Same shell-before-pump drop discipline as
    // `run_one_phase_brush`: capture the outcome, drop in order, then
    // return -- see `open_log_file`.
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

/// `--shell bash`: spawns a genuine `bash <bin_dir>/misc-functions.sh
/// <dyn_command>` subprocess -- matching real `doebuild.py`'s own
/// `misc_sh = shlex.quote(misc_sh_binary) + " __dyn_%s"` invocation
/// shape exactly. See `run_one_phase_bash`'s own doc comment for why a
/// blocking `std::process::Command` here too.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn run_misc_functions_bash(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    log_file: Option<&Path>,
    fs_sandbox: bool,
) -> Result<i32, String> {
    let mut vars = phase_env_vars(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    if fs_sandbox {
        // Real `MiscFunctionsProcess._spawn`: swap in a separate log so
        // a misc-functions violation doesn't clobber the phase's own
        // `sandbox.log`; enable the sandbox (`bin/misc-functions.sh`
        // reads `SANDBOX_DISABLED` the same way `bin/ebuild.sh` does).
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
    // The gzip pump guard must outlive `cmd` (same contract as
    // `run_one_phase_bash`): declared BEFORE `cmd` so reverse drop
    // order destroys it AFTER `cmd` on every path; filled from the
    // sink below. The explicit `drop(cmd)` after the wait closes the
    // parent write-end copies deterministically.
    let mut pump_guard: Option<LogPump> = None;
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("misc-functions.sh"), dyn_command, iso);
    // Same curated environment real `MiscFunctionsProcess` gets (the
    // phase env, not the inherited process env): `misc-functions.sh`
    // sources `bin/ebuild.sh`, whose `__preprocess_ebuild_env` *saves*
    // the live env back to `${T}/environment` when a binpkg's
    // `environment.raw` marker is present -- so an unfiltered inherited
    // env (the calling `cargo test`/shell process) would leak into the
    // regenerated vdb env. See `run_one_phase_bash`'s own `env_clear`
    // for the phase-side equivalent. #326 D6 also drops
    // `PORTAGE_IPC_DAEMON` here (no IPC daemon).
    cmd.env_clear();
    cmd.envs(
        std::env::vars()
            .filter(|(k, _)| environ_whitelisted(k) && k != "LC_ALL" && k != "PORTAGE_IPC_DAEMON"),
    );
    cmd.envs(vars);
    if let Some(path) = log_file {
        let sink = open_log_file(path)?;
        cmd.stdout(sink.out).stderr(sink.err);
        pump_guard = sink.pump;
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for {dyn_command} failed: {e}"))?;
    drop(cmd);
    // Explicit (a pure-drop guard reads as unused otherwise): joins the
    // gzip thread now that every write end is closed.
    drop(pump_guard);
    Ok(status.code().unwrap_or(1))
}

/// One process-wide multi-threaded tokio runtime (real `_emerge/
/// Scheduler.py` runs its whole invocation -- every package's every
/// phase -- on a single `asyncio` event loop; portuale's own sync entry
/// points -- `run_misc_function`/`run_commands_logged`/
/// `run_single_phase`/`run_phase_from_saved_env` -- each used to spin up
/// and tear down their own fresh `Builder::new_multi_thread()` runtime,
/// meaning a single `emerge` merging many packages paid that thread-pool
/// setup/teardown cost once per phase per package, not once per
/// invocation). `OnceLock` rather than a value threaded through every
/// call site: none of these entry points are themselves called from
/// inside an existing tokio context (`ebuild_phases::run_one_phase`'s
/// own async body never calls back into any of the four), so lazily
/// building this once and reusing it from everywhere is safe, and
/// avoids turning this into a signature change touching every caller
/// across `ebuild.rs`/`ebuild_merge.rs`/`ebuild_package.rs`/
/// `emerge_build.rs`. Never torn down -- this is a CLI process, not a
/// long-lived server; process exit reclaims the thread pool same as any
/// other resource.
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

/// Synchronous entry point mirroring `run_single_phase`'s own shape, for
/// `ebuild_package`'s own real `__dyn_package` call.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn run_misc_function(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        run_misc_functions(
            &env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env,
            debug,
            config_root,
            shell,
            None,
        )
        .await
    })
}
