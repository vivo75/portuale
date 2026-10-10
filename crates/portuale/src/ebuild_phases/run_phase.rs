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
    pub(crate) log_file: Option<&'a Path>,
}

/// Builds one fresh embedded brush shell for a single phase: real
/// `doebuild()` spawns a brand new `bin/ebuild.sh` *process* per phase
/// (`spawnebuild()`), which matters for more than just isolation --
/// `bin/ebuild.sh`'s own tail makes `EBUILD_PHASE` (among other
/// variables) `readonly` (`declare -r`), so a real fresh process is the
/// only way a *second* phase can `export EBUILD_PHASE=<next>` at all.
/// Portuale mirrors that exactly with a fresh `Shell` per phase rather
/// than trying to reuse one across phases (an earlier version of this
/// function did share one shell across a whole command's own prerequisite
/// chain -- confirmed empirically to fail with "cannot mutate readonly
/// variable" on the second phase, exactly the real readonly-variable
/// mechanism working as designed). Real `PORTAGE_BUILDDIR`-relative
/// resume markers (`.pretended`/`.setuped`/`.unpacked`/etc., written by
/// `__dyn_*` themselves) still make a prerequisite phase that's already
/// run cheap to "re-run" from a fresh shell, exactly the way real
/// `doebuild()` itself relies on across its own separate `spawnebuild()`
/// calls -- this isn't a new mechanism invented for portuale.
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

    // `FEATURES={network,ipc,mount,pid}-sandbox` / `FEATURES=sandbox`:
    // the isolated `src_*` phases run as a real subprocess -- wrapped in
    // `unshare` and/or the `sandbox` binary -- regardless of the
    // requested backend. Neither an `unshare(2)` namespace nor an
    // LD_PRELOAD `libsandbox.so` can confine the in-process `Brush`
    // interpreter without taking the whole `portuale` process with it.
    // See this module's own doc comment.
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

/// The host's real `bash`, as a phase's own `$BASH`.
///
/// `__filter_readonly_variables` (`bin/phase-functions.sh:101`) lists
/// bash's special variables by running `env -i -- "${BASH}" -c '…'`; a
/// real bash sets `$BASH` to itself, while an embedded brush `Shell` is
/// built without a `shell_name`, so brush never sets it and the command
/// comes back empty (the `env: '': No such file or directory` in the L2
/// smoke log). Resolve the first `bash` on `PATH` -- the same one the
/// `Bash` backend spawns -- once per process.
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

/// Builds one fresh embedded brush shell for a phase or misc-functions
/// run, with `$BASH` pointed at the host's real bash (`real_bash_path`)
/// so the real `bin/*.sh` construct of listing bash's special variables
/// works. `BASH` is set by a shell itself, not taken from the
/// environment, so this is a shell-state assignment, not an exported
/// variable (`phase_setup_script` is not involved).
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

/// Restores the process umask on drop. The embedded `Brush` shell's
/// `umask` builtin (`bin/ebuild.sh` runs `umask 022`, `phase-functions.sh`
/// `umask 002`) is the process-wide `umask(2)`, whereas real runs every
/// phase in its own process and the parent's umask never moves. Without
/// this, one in-process phase leaves its umask behind for every later
/// file the host process (or a parallel test) creates (#308).
pub(crate) struct UmaskGuard(pub(crate) libc::mode_t);

impl UmaskGuard {
    pub(crate) fn save() -> Self {
        // `/proc/self/status` reads without the `umask(0)` dance, which
        // would briefly make files created by other threads world-writable.
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
    pub(crate) log_file: Option<&'a Path>,
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
    // The shell owns the pipe write ends (its fd table) and must drop
    // before `pump` joins (EOF then finish) -- see `open_log_file`'s
    // lifetime contract. No `?` may return between here and the drops
    // (an early return would drop `pump`, declared after `shell`,
    // first and hang the join), so the outcome is captured, the
    // holders dropped in order, and only then returned.
    let outcome: Result<i32, String> = async {
        let setup_result = shell
            .run_string(&setup, &brush_core::SourceInfo::default(), &params)
            .await
            .map_err(|e| format!("environment setup failed: {e}"))?;
        // A non-zero result here is a real setup failure, not a phase
        // outcome: carry on only when the environment actually loaded. The
        // exit code (not just a printed error) is what makes this a hard
        // stop -- a broken saved environment must fail the phase, never
        // silently skip the ebuild's own functions (see B2 of
        // `history/backlog_tier_1_sliced.opus.md`). `Ok(nonzero)`, like
        // `run_one_phase_bash`'s own child status: a phase failure, not a
        // spawn failure.
        if !setup_result.is_success() {
            return Ok(i32::from(u8::from(setup_result.exit_code)));
        }

        // Real bin/ebuild.sh's own top-level code (unconditional, not gated
        // on EBUILD_SH_ARGS at all -- confirmed empirically, not just by
        // reading it: `bin/ebuild.sh:681`'s own `source "${EBUILD}" || die`
        // sits in ebuild.sh's own main body) ALREADY sources the ebuild file
        // itself as part of being sourced -- a second, separate
        // `source_script` call on the ebuild file here would be genuinely
        // redundant, not just wasteful: it re-runs the ebuild's own
        // top-level code a second time against variables ebuild.sh's own
        // tail has *already* made `readonly` from the first pass, which
        // fails outright ("cannot mutate readonly variable") -- confirmed
        // empirically by removing this line and watching that error
        // disappear.
        let source_result = shell
            .source_script(
                bin_dir.join("ebuild.sh"),
                std::iter::empty::<String>(),
                &params,
            )
            .await
            .map_err(|e| format!("sourcing bin/ebuild.sh failed: {e}"))?;
        // A non-zero exit from sourcing ebuild.sh means its own top-level
        // guard died -- most importantly the `source "${T}"/environment ||
        // die "error sourcing environment"` at `bin/ebuild.sh:580` -- and
        // `__ebuild_main` must not run on a half-loaded environment. Same
        // phase-failure shape as above.
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
    pub(crate) log_file: Option<&'a Path>,
    pub(crate) iso: Isolation,
}

/// `--shell bash`: spawns a genuine `bash <bin_dir>/ebuild.sh <phase>`
/// subprocess instead of the embedded `brush_core::Shell` `run_one_
/// phase_brush` above uses -- see `ShellBackend`'s own doc comment for
/// why this mirrors real portage's own `_doebuild_spawn()` invocation
/// shape (`EBUILD_SH_ARGS="$*"` picking `<phase>` up from real argv)
/// far more directly than the brush path's own two-step "source, then
/// separately `invoke_function`" dance does. Environment variables are
/// real subprocess env vars (`phase_env_vars`), not shell `export`
/// source text, so there's no shell-quoting step -- and so no `$`/
/// backtick-expansion risk -- at all, unlike `phase_setup_script`'s own
/// Rust-Debug escaping. A blocking `std::process::Command`, not a
/// `tokio::process` one: matches `fetch.rs`'s own precedent for
/// spawning a real subprocess (`wget`) from inside an `async fn`
/// without pulling in tokio's own "process" feature.
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
    // The gzip pump guard must outlive `cmd` (see `open_log_file`'s
    // lifetime contract + the sink comment below): it joins the gzip
    // thread, which needs EOF, which needs `cmd`'s stored parent
    // write-end copies closed. Declared BEFORE `cmd` so reverse drop
    // order destroys it AFTER `cmd` on every path (normal return and
    // `?` early return alike); `drop(cmd)` below closes the copies
    // deterministically right after the wait.
    let mut pump_guard: Option<LogPump> = None;
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("ebuild.sh"), phase, iso);
    // Real `doebuild` spawns a phase with a curated `config.environ()`,
    // never the inherited process env wholesale -- so `EMERGE_DEFAULT_OPTS`
    // and portuale-only vars don't leak into the phase (and, since L1-c,
    // into the regenerated vdb `environment`). Keep only the calling-env
    // vars real's `environ_whitelist` keeps; `phase_env_vars` sets the
    // rest on top. (L1-f.)
    cmd.env_clear();
    // `LC_ALL` never reaches a real phase (see `phase_env_vars`' own
    // locale split): real `split_LC_ALL` blanks it, so even a
    // whitelisted process-env entry is dropped here and the `LC_*`
    // category exports stand alone. #326 D6 drops `PORTAGE_IPC_DAEMON`
    // the same way: there is no IPC daemon, and a leaked value would
    // send `has_version` down the `ebuild-ipc` branch.
    cmd.envs(
        std::env::vars()
            .filter(|(k, _)| environ_whitelisted(k) && k != "LC_ALL" && k != "PORTAGE_IPC_DAEMON"),
    );
    cmd.envs(vars);
    if let Some(path) = log_file {
        // `sink.pump` MUST outlive `cmd`: the guard joins the gzip
        // thread, which needs EOF on the pipes, which needs `cmd`'s
        // stored parent copies of the write ends closed (plus the
        // waited child's). A guard scoped to this block would drop
        // (join) here -- before the child is even spawned, with `cmd`
        // still holding its copies -- and hang forever. So only the
        // write ends move into `cmd`; the guard escapes to the outer
        // scope, where reverse declaration order drops it after `cmd`
        // on every path (normal return and `?` early return alike).
        // `drop(cmd)` below closes the copies deterministically right
        // after the wait; see `open_log_file`.
        let sink = open_log_file(path)?;
        cmd.stdout(sink.out).stderr(sink.err);
        pump_guard = sink.pump;
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for phase {phase} failed: {e}"))?;
    drop(cmd);
    // Explicit (a pure-drop guard reads as unused otherwise): joins the
    // gzip thread now that every write end is closed. Scope-end order
    // would do the same, including on the `?` early return above.
    drop(pump_guard);
    Ok(status.code().unwrap_or(1))
}
