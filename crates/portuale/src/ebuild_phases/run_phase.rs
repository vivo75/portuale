//! Running one phase on brush or bash.

use crate::ebuild_phases::*;

/// Arguments of [`run_one_phase`].
pub(crate) struct RunOnePhaseArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: PhaseLog<'a>,
}

/// Runs one phase in a fresh embedded shell: Portage spawns a brand-new `bin/ebuild.sh`
/// process per phase, which matters because `ebuild.sh` makes `EBUILD_PHASE` readonly —
/// a second phase can't re-export it in a shared shell. Resume markers make re-running
/// done prerequisites cheap.
/// Portage: doebuild.py, spawnebuild
pub(crate) async fn run_one_phase(args: RunOnePhaseArgs<'_>) -> Result<i32, String> {
    let RunOnePhaseArgs {
        env,
        root,
        phase,
        debug,
        extra_env,
        config_root,
        shell,
        log_file,
    } = args;
    let bin_dir = bin_dir()?.to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");

    // Isolated `src_*` phases run in a subprocess whatever the backend: no
    // namespace or sandbox can confine the in-process Brush interpreter.
    let iso = phase_isolation(env, phase, &features_string(extra_env));
    let effective_shell = if iso.any() { ShellBackend::Bash } else { shell };

    match effective_shell {
        ShellBackend::Brush => {
            run_one_phase_brush(RunOnePhaseBrushArgs {
                env,
                root,
                phase,
                debug,
                extra_env,
                bin_dir: &bin_dir,
                helpers_dir: &helpers_dir,
                config_root,
                log_file,
            })
            .await
        }
        ShellBackend::Bash => run_one_phase_bash(RunOnePhaseBashArgs {
            env,
            root,
            phase,
            debug,
            extra_env,
            bin_dir: &bin_dir,
            helpers_dir: &helpers_dir,
            config_root,
            log_file,
            iso,
        }),
    }
}

/// The host `bash` binary, as a phase's `$BASH` (brush never sets it, which broke
/// `__filter_readonly_variables`). First `bash` on `PATH`, resolved once.
pub(crate) fn real_bash_path() -> String {
    use std::sync::OnceLock;
    static RESOLVED: OnceLock<String> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    let candidate = dir.join("bash");
                    if candidate.is_file() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
            "bash".to_owned()
        })
        .clone()
}

/// A fresh embedded brush shell with `$BASH` set to the host bash (a shell-state
/// assignment, not an exported variable).
pub(crate) async fn new_brush_phase_shell() -> Result<brush_core::Shell, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    shell
        .env_mut()
        .set_global("BASH", brush_core::ShellVariable::new(real_bash_path()))
        .map_err(|e| format!("setting $BASH failed: {e}"))?;
    Ok(shell)
}

/// Restores the process umask on drop: the embedded shell's `umask` builtin is
/// process-wide, while Portage runs every phase in its own process.
pub(crate) struct UmaskGuard(pub(crate) libc::mode_t);

impl UmaskGuard {
    pub(crate) fn save() -> Self {
        // No `umask(0)` dance: that would briefly make other threads' files world-writable.
        let from_proc = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Umask:"))
                    .and_then(|v| libc::mode_t::from_str_radix(v.trim(), 8).ok())
            });
        Self(from_proc.unwrap_or_else(|| {
            // SAFETY: `umask(2)` cannot fail.
            let old = unsafe { libc::umask(0) };
            unsafe { libc::umask(old) };
            old
        }))
    }
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        // SAFETY: `umask(2)` cannot fail.
        unsafe { libc::umask(self.0) };
    }
}
/// Arguments of [`run_one_phase_brush`].
pub(crate) struct RunOnePhaseBrushArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: PhaseLog<'a>,
}

pub(crate) async fn run_one_phase_brush(args: RunOnePhaseBrushArgs<'_>) -> Result<i32, String> {
    let RunOnePhaseBrushArgs {
        env,
        root,
        phase,
        debug,
        extra_env,
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
        ebuild_phase_value: phase,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    });
    // Shell must drop before pump (EOF then finish): capture the outcome, drop in
    // order, then return. No `?` may return between here and the drops.
    let outcome: Result<i32, String> = async {
        let setup_result = shell
            .run_string(&setup, &brush_core::SourceInfo::default(), &params)
            .await
            .map_err(|e| format!("environment setup failed: {e}"))?;
        // A non-zero setup is a hard stop: the environment never loaded, so the
        // ebuild's own functions must not run.
        if !setup_result.is_success() {
            return Ok(i32::from(u8::from(setup_result.exit_code)));
        }

        // `bin/ebuild.sh` already sources the ebuild itself; sourcing it again here
        // would fail on the now-`readonly` vars.
        let source_result = shell
            .source_script(
                bin_dir.join("ebuild.sh"),
                std::iter::empty::<String>(),
                &params,
            )
            .await
            .map_err(|e| format!("sourcing bin/ebuild.sh failed: {e}"))?;
        // A non-zero source means a top-level guard died (notably `source
        // "${T}"/environment`); `__ebuild_main` must not run half-loaded.
        if !source_result.is_success() {
            return Ok(i32::from(u8::from(source_result.exit_code)));
        }

        shell
            .invoke_function("__ebuild_main", [phase], params)
            .await
            .map_err(|e| format!("phase {phase} failed: {e}"))
            .map(|result| i32::from(u8::from(result.exit_code)))
    }
    .await;
    drop(shell);
    drop(pump);
    outcome
}

/// Arguments of [`run_one_phase_bash`].
pub(crate) struct RunOnePhaseBashArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: PhaseLog<'a>,
    pub(crate) iso: Isolation,
}

/// `Bash`: a genuine `bash <bin_dir>/ebuild.sh <phase>` subprocess (Portage's
/// `_doebuild_spawn` shape; `<phase>` arrives via argv like `EBUILD_SH_ARGS`).
/// Env vars are subprocess vars, so no quoting step and no expansion risk.
/// Blocking `Command`, like `fetch.rs`.
/// Portage: doebuild.py
pub(crate) fn run_one_phase_bash(args: RunOnePhaseBashArgs<'_>) -> Result<i32, String> {
    let RunOnePhaseBashArgs {
        env,
        root,
        phase,
        debug,
        extra_env,
        bin_dir,
        helpers_dir,
        config_root,
        log_file,
        iso,
    } = args;
    let vars = phase_env_vars(PhaseEnvVarsArgs {
        env,
        root,
        ebuild_phase_value: phase,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    });
    spawn_bash_script(SpawnBashScriptArgs {
        script: &bin_dir.join("ebuild.sh"),
        argv_word: phase,
        vars,
        iso,
        log_file,
        context: &format!("phase {phase}"),
    })
}
