//! Running a phase command list, with logging.

use crate::ebuild_phases::*;

/// Where one phase's stdout+stderr go.
// Portage: _emerge/EbuildPhase.py (background log vs foreground tee), doebuild.py `_spawn_phase`
#[derive(Clone, Copy)]
pub(crate) enum PhaseLog<'a> {
    /// The child inherits our stdio; nothing is written to a file.
    /// Direct-path `clean`, unmerge hooks, info probes.
    Inherit,
    /// Background/captured: the child writes to the file only, never the
    /// terminal (parallel builds must not interleave there).
    Capture(&'a Path),
    /// Derive `${T}/build.log` (or the `PORT_LOGDIR` placement) from the
    /// phase environment and tee there. Resolved to [`PhaseLog::Tee`] by
    /// `run_commands_async` and `run_single_phase` before any spawn, so it
    /// never reaches a leaf; leaves treat it as [`PhaseLog::Inherit`].
    Foreground,
    /// Foreground: the child writes to our stdout and the file, like
    /// Portage's `_spawn_phase` logging pipe. Only constructed by the
    /// `Foreground` resolution above.
    Tee(&'a Path),
}

impl<'a> PhaseLog<'a> {
    /// A background log path when present, else [`PhaseLog::Inherit`].
    pub(crate) fn capture(path: Option<&'a Path>) -> Self {
        match path {
            Some(path) => PhaseLog::Capture(path),
            None => PhaseLog::Inherit,
        }
    }
}

pub(crate) struct RunCommandsAsyncArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) commands: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) distdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: PhaseLog<'a>,
    /// Caller-supplied env, appended after portuale's own base vars so it
    /// overrides them. The build path passes the resolved `USE` flags here;
    /// `&[]` for a standalone `ebuild <file> <phase>` run.
    pub(crate) build_env: &'a [(String, String)],
}

/// Drives `commands` against `ebuild_path`: environment once, then each
/// command's `phase_prerequisites` chain in a fresh shell per phase.
/// Fetch runs once up front when the chain includes `unpack`; a successful
/// `install` is followed by post-install metadata, the `install_qa_check ...`
/// sequence (`EBUILD_PHASE` still `"install"`), and the soname deps.
// Portage: doebuild.py, EbuildPhase.py, config.py, bin/ebuild.sh
pub(crate) async fn run_commands_async(args: RunCommandsAsyncArgs<'_>) -> Result<i32, String> {
    let RunCommandsAsyncArgs {
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        log_file,
        build_env,
    } = args;
    let env = compute_environment(ebuild_path, portage_tmpdir)?;

    // The actual distdir goes in as `DISTDIR`; `phase_env_vars` remaps it the
    // way Portage's `config.environ` does: the actual directory becomes
    // `PORTAGE_ACTUAL_DISTDIR` and `DISTDIR` the per-package fake.
    let mut extra_env = vec![("DISTDIR".to_string(), distdir.display().to_string())];
    extra_env.extend(build_env.iter().cloned());
    // The resolved `USE` the caller threaded through, so `A` names exactly the
    // distfiles `use()` will expect.
    let use_flags = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "USE")
        .map_or("", |(_, v)| v.as_str())
        .to_string();
    let features = features_string(&extra_env);

    // A foreground run tees every phase through `${T}/build.log` (or the
    // `PORT_LOGDIR` placement) like Portage's `_spawn_phase` logging pipe:
    // the path is known only once the environment exists, so it resolves
    // here, and `PORTAGE_LOG_FILE` rides the phase env so `die` prints
    // its build-log line. `Capture` (background) and `Inherit` pass
    // through untouched.
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
                    features: &features,
                    logdir: logdir.as_deref(),
                    sep: &sep,
                },
            ));
            PhaseLog::Tee(foreground_path.as_deref().unwrap())
        }
        resolved => resolved,
    };
    if let PhaseLog::Tee(path) = log_file {
        extra_env.push(("PORTAGE_LOG_FILE".to_string(), path.display().to_string()));
    }
    // A direct `ebuild <file> <phase>` run is a source merge
    // (`EMERGE_FROM=ebuild` rides in `phase_env_vars` already):
    // `MERGE_TYPE=source`, like `doebuild()` sets for `tree == "porttree"`.
    // `exports_merge_type` (EAPI 4+) is enforced centrally in
    // `apply_eapi_phase_env`, which also drops it for older EAPIs.
    // Portage: doebuild.py, doebuild
    if !extra_env.iter().any(|(k, _)| k == "MERGE_TYPE") {
        extra_env.push(("MERGE_TYPE".to_string(), "source".to_string()));
    }
    // Portage: doebuild.py, doebuild — the fake distdir is prepared unless the
    // command is bare `setup` (FEATURES=noauto excepted); `install`/`unpack`
    // chains always need it. The first test is over the command, not the chain.
    for &command in commands {
        let chain_here = phase_prerequisites(command);
        // Portage: doebuild.py, the WORKDIR block — one check per command
        // (`bin/ebuild` calls `doebuild` once per arg), before any of its
        // directories exist, and only when its chain contains `unpack`.
        if chain_here.contains(&"unpack") {
            let alist = package_a(&env, &use_flags)?;
            if let WorkdirCheck::Lines(lines) =
                check_workdir(env.portage_builddir(), &env.workdir(), distdir, &alist)
            {
                for line in &lines {
                    emit_phase_line(line, log_file);
                }
            }
        }
        create_directories(&env)?;
        let a = if chain_here.contains(&"unpack") {
            let (a, _aa) = fetch_sources(FetchSourcesArgs {
                env: &env,
                root,
                distdir,
                debug,
                config_root,
                shell,
                features: &features,
                use_flags: &use_flags,
            })
            .await?;
            // Portage exports `A` for EAPI 4+ but never `AA`
            // (`exports_AA` ends at 3); the floor here is EAPI 5+.
            // `AA`'s absence is enforced centrally in `apply_eapi_phase_env`.
            extra_env.push(("A".to_string(), a.join(" ")));
            Some(a)
        } else {
            None
        };
        if (command != "setup" && !features.split_whitespace().any(|f| f == "noauto"))
            || chain_here.iter().any(|&p| p == "install" || p == "unpack")
        {
            let alist = match a {
                Some(a) => a,
                None => package_a(&env, &use_flags)?,
            };
            prepare_fake_distdir(&env, distdir, &alist)?;
        }

        for phase in chain_here {
            // Portage: doebuild.py, spawnebuild — a stamped phase prints the
            // skip pair and never spawns (`install` re-runs under `noauto`).
            if is_stamped(env.portage_builddir(), phase, &features) {
                for line in stamp_skip_lines(phase, &env.split.pf, env.portage_builddir()) {
                    emit_phase_line(&line, log_file);
                }
                continue;
            }
            let status = run_one_phase(RunOnePhaseArgs {
                env: &env,
                root,
                phase,
                debug,
                extra_env: &extra_env,
                config_root,
                shell,
                log_file,
            })
            .await?;
            if status != 0 {
                return Ok(status);
            }
            // Portage: EbuildPhase.py, _post_phase_cmds — after a successful
            // `install`, run `install_qa_check install_symlink_html_docs
            // install_hooks` here: unconditional, and never part of
            // `phase_prerequisites`' own chain. `misc-functions.sh`
            // re-splits its args on whitespace, so one space-joined string
            // equals three argv entries.
            if phase == "install" {
                // Portage: EbuildPhase.py, _ebuild_exit_unlocked — metadata and the
                // uid fix run before the QA sequence, so `SIZE` describes the
                // pre-transform image.
                write_post_install_metadata(&env, root, build_env)?;
                let qa_status = run_misc_functions(RunMiscFunctionsArgs {
                    env: &env,
                    root,
                    ebuild_phase_value: "install",
                    dyn_command: "install_qa_check install_symlink_html_docs install_hooks",
                    extra_env: &extra_env,
                    debug,
                    config_root,
                    shell,
                    log_file,
                })
                .await?;
                if qa_status != 0 {
                    return Ok(qa_status);
                }
                // Portage: EbuildPhase.py, _commands_exit — soname symlinks,
                // `PROVIDES`/`REQUIRES` and the implicit libc dep run right
                // after the QA sequence above.
                write_post_install_soname_deps(&env, root)?;
            }
        }
    }
    Ok(0)
}

/// One Rust-side phase line: stdout on the direct path, appended to the
/// build log when the merge Scheduler owns the run (Portage's child
/// stdout is the log there). A foreground tee prints to stdout only:
/// Portage's own skip/WORKDIR lines go through `writemsg_stdout`, never
/// the phase logging pipe (doebuild.py).
/// Portage: doebuild.py (writemsg_stdout), _emerge/EbuildPhase.py
pub(crate) fn emit_phase_line(line: &str, log: PhaseLog<'_>) {
    match log {
        PhaseLog::Inherit | PhaseLog::Foreground | PhaseLog::Tee(_) => {
            println!("{line}");
        }
        PhaseLog::Capture(path) => {
            // The phase log's own append machinery, so `.gz` logs stay
            // valid members too. The write end drops (and the pump joins)
            // before this returns.
            if let Ok(sink) = open_log_file(path) {
                use std::io::Write as _;
                let mut out = sink.out;
                let _ = writeln!(out, "{line}");
            }
        }
    }
}

pub struct RunCommandsArgs<'a> {
    pub ebuild_path: &'a Path,
    pub commands: &'a [&'a str],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub distdir: &'a Path,
    pub debug: bool,
    pub config_root: &'a Path,
    pub shell: ShellBackend,
    pub log_file: PhaseLog<'a>,
    pub build_env: &'a [(String, String)],
}

/// Synchronous entry point for `ebuild.rs`: spins up a multi-threaded tokio
/// runtime for the call (a single-threaded one deadlocks mid-run).
/// `PORTAGE_TMPDIR` is read by the caller, so tests can pass it directly.
/// `Foreground` tees through `${T}/build.log`; the merge Scheduler owns
/// `Capture`, everything else inherits.
pub fn run_commands(args: RunCommandsArgs<'_>) -> Result<i32, String> {
    let RunCommandsArgs {
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        log_file,
        build_env,
    } = args;
    run_commands_logged(RunCommandsLoggedArgs {
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        log_file,
        build_env,
    })
}

pub struct RunCommandsLoggedArgs<'a> {
    pub ebuild_path: &'a Path,
    pub commands: &'a [&'a str],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub distdir: &'a Path,
    pub debug: bool,
    pub config_root: &'a Path,
    pub shell: ShellBackend,
    pub log_file: PhaseLog<'a>,
    pub build_env: &'a [(String, String)],
}

/// Like `run_commands`, but every phase appends stdout+stderr to `log_file`
/// instead of the terminal, so parallel builds don't interleave.
// Portage: _emerge/EbuildPhase.py (`PORTAGE_LOG_FILE`)
pub fn run_commands_logged(args: RunCommandsLoggedArgs<'_>) -> Result<i32, String> {
    let RunCommandsLoggedArgs {
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        log_file,
        build_env,
    } = args;
    let runtime = shared_runtime()?;
    runtime.block_on(run_commands_async(RunCommandsAsyncArgs {
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        log_file,
        build_env,
    }))
}

/// Opens `log_file` for append (creating parents): directly, or through a gzip
/// pump thread (one member per call; an empty phase appends nothing) for `.gz`.
/// The guard must drop after the phase's write ends close, or the join hangs
/// on EOF; joining first also finishes the member before any reader sees it.
// Portage: _emerge/EbuildPhase.py, _open_log
pub(crate) struct LogSink {
    pub(crate) out: std::fs::File,
    pub(crate) err: std::fs::File,
    pub(crate) pump: Option<LogPump>,
}

pub(crate) struct LogPump {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LogPump {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Drains one pipe's read end into the shared gzip encoder; returns bytes forwarded.
pub(crate) fn pump_one_pipe(
    mut read_end: std::fs::File,
    encoder: &std::sync::Arc<std::sync::Mutex<flate2::write::GzEncoder<std::fs::File>>>,
) -> u64 {
    use std::io::{Read, Write};
    let mut forwarded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        match read_end.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let Ok(mut enc) = encoder.lock() else {
                    break;
                };
                if enc.write_all(&buf[..n]).is_err() {
                    break;
                }
                forwarded += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    forwarded
}

/// A `pipe2` pair with `O_CLOEXEC`: the pump's read ends must not leak
/// into the phase child (EOF keys off the write ends, so this is hygiene
/// only).
fn cloexec_pipe(what: &str) -> Result<(std::fs::File, std::fs::File), String> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!(
            "pipe for {what}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe {
        (
            std::fs::File::from_raw_fd(fds[0]),
            std::fs::File::from_raw_fd(fds[1]),
        )
    })
}

pub(crate) fn open_log_file(log_file: &Path) -> Result<LogSink, String> {
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let is_gz = log_file.extension().is_some_and(|ext| ext == "gz");
    if !is_gz {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| format!("{}: {e}", log_file.display()))?;
        let g = f
            .try_clone()
            .map_err(|e| format!("{}: {e}", log_file.display()))?;
        return Ok(LogSink {
            out: f,
            err: g,
            pump: None,
        });
    }
    let target = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .map_err(|e| format!("{}: {e}", log_file.display()))?;
    let (out_r, out_w) = cloexec_pipe("build-log stdout")?;
    let (err_r, err_w) = cloexec_pipe("build-log stderr")?;
    let thread = std::thread::spawn(move || {
        let encoder = std::sync::Arc::new(std::sync::Mutex::new(flate2::write::GzEncoder::new(
            target,
            flate2::Compression::default(),
        )));
        let encoder_err = encoder.clone();
        let encoder_out = encoder.clone();
        let out_pump = std::thread::spawn(move || pump_one_pipe(out_r, &encoder_out));
        let err_bytes = pump_one_pipe(err_r, &encoder_err);
        let out_bytes = out_pump.join().unwrap_or(0);
        if out_bytes + err_bytes > 0
            && let Ok(mut enc) = encoder.lock()
        {
            // Trailer for this phase's member; the next phase appends a fresh one.
            let _ = enc.try_finish();
            let _ = enc.get_mut().sync_all();
        }
        // Else: drop the encoder unfinished — nothing was ever written, so no
        // header went out either, and the file is left exactly as found.
    });
    Ok(LogSink {
        out: out_w,
        err: err_w,
        pump: Some(LogPump {
            thread: Some(thread),
        }),
    })
}

/// A foreground build-log tee: one pipe shared by the child's stdout and
/// stderr, drained by a thread that writes every chunk to our stdout and
/// appends it to the log file. Both streams merge (like Portage's
/// `_spawn_phase` logging pipe), so their relative order is preserved.
/// The thread joins before the `.gz` pump does: it owns the only write
/// ends feeding the encoder, so dropping it delivers EOF first.
// Portage: doebuild.py, _spawn_phase; portage/util/_async/BuildLogger.py
pub(crate) struct TeePump {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) pump: Option<LogPump>,
}

impl Drop for TeePump {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Opens `log_file` for append (creating parents, `.gz` through the same
/// pump as [`open_log_file`]) and returns the single write end the child
/// lends to both its stdout and its stderr, plus the tee guard. The guard
/// must drop after the child's write ends close, or the join hangs on EOF.
pub(crate) fn open_tee_log(log_path: &Path) -> Result<(std::fs::File, TeePump), String> {
    let sink = open_log_file(log_path)?;
    let (read_end, write_end) = cloexec_pipe("foreground build-log tee")?;
    let mut log_out = sink.out;
    let thread = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        let mut read_end = read_end;
        let mut buf = [0u8; 65536];
        loop {
            match read_end.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = stdout.write_all(&buf[..n]);
                    if log_out.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = stdout.flush();
    });
    Ok((
        write_end,
        TeePump {
            thread: Some(thread),
            pump: sink.pump,
        },
    ))
}
/// Arguments of [`spawn_bash_script`].
pub(crate) struct SpawnBashScriptArgs<'a> {
    pub(crate) script: &'a Path,
    pub(crate) argv_word: &'a str,
    pub(crate) vars: Vec<(String, String)>,
    pub(crate) iso: Isolation,
    pub(crate) log_file: PhaseLog<'a>,
    /// Names the spawn in its failure message (`"phase setup"` reads
    /// `spawning real bash for phase setup failed: …`).
    pub(crate) context: &'a str,
}

/// Run a vendored bash helper script under the sandbox with a curated
/// environment and optional log capture: only whitelisted calling-env
/// vars pass through (`LC_ALL` and the IPC daemon never do), and the
/// log pump guard outlives the command so the gzip thread joins after
/// every write end closes. A foreground tee lends one pipe to both the
/// child's stdout and its stderr, so their relative order survives.
/// Shared by the phase and misc-functions spawns so their setup cannot
/// drift apart.
pub(crate) fn spawn_bash_script(args: SpawnBashScriptArgs<'_>) -> Result<i32, String> {
    let SpawnBashScriptArgs {
        script,
        argv_word,
        vars,
        iso,
        log_file,
        context,
    } = args;
    // The guards must outlive `cmd` (see `open_log_file`): declared first so
    // drop order destroys them after `cmd`, on every path.
    let mut pump_guard: Option<LogPump> = None;
    let mut tee_guard: Option<TeePump> = None;
    let mut cmd = sandbox_wrapped_command(script, argv_word, iso);
    // Curated env only, like Portage: only whitelisted calling-env vars pass through;
    // the caller-computed `vars` set the rest on top.
    cmd.env_clear();
    // `LC_ALL` never reaches a phase (see `phase_env_vars`); no IPC daemon either.
    cmd.envs(
        std::env::vars()
            .filter(|(k, _)| environ_whitelisted(k) && k != "LC_ALL" && k != "PORTAGE_IPC_DAEMON"),
    );
    cmd.envs(vars);
    match log_file {
        PhaseLog::Capture(path) => {
            // Only the write ends move into `cmd`; the guard escapes outward so it drops
            // after `cmd`. `drop(cmd)` below closes the copies deterministically.
            let sink = open_log_file(path)?;
            cmd.stdout(sink.out).stderr(sink.err);
            pump_guard = sink.pump;
        }
        PhaseLog::Tee(path) => {
            let (write_end, pump) = open_tee_log(path)?;
            let err_end = write_end
                .try_clone()
                .map_err(|e| format!("dup for {context} stderr failed: {e}"))?;
            cmd.stdout(write_end).stderr(err_end);
            tee_guard = Some(pump);
        }
        // `Foreground` resolves upstream (`run_commands_async`,
        // `run_single_phase`); reaching here would tee nowhere, so
        // inherit like `Inherit` rather than misrouting the stream.
        PhaseLog::Inherit | PhaseLog::Foreground => {}
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for {context} failed: {e}"))?;
    drop(cmd);
    // Explicit: joins the pump threads now that every write end is closed.
    drop(pump_guard);
    drop(tee_guard);
    Ok(status.code().unwrap_or(1))
}
