//! Running a phase command list, with logging.

use crate::ebuild_phases::*;

pub(crate) struct RunCommandsAsyncArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) commands: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) distdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: Option<&'a Path>,
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
    create_directories(&env)?;

    let chain: Vec<&str> = commands
        .iter()
        .flat_map(|&c| phase_prerequisites(c))
        .collect();
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
    let a = if chain.contains(&"unpack") {
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
    if (commands.iter().any(|&c| c != "setup")
        && !features.split_whitespace().any(|f| f == "noauto"))
        || chain.iter().any(|&p| p == "install" || p == "unpack")
    {
        let alist = match a {
            Some(a) => a,
            None => package_a(&env, &use_flags)?,
        };
        prepare_fake_distdir(&env, distdir, &alist)?;
    }

    for &command in commands {
        for phase in phase_prerequisites(command) {
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

pub struct RunCommandsArgs<'a> {
    pub ebuild_path: &'a Path,
    pub commands: &'a [&'a str],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub distdir: &'a Path,
    pub debug: bool,
    pub config_root: &'a Path,
    pub shell: ShellBackend,
    pub build_env: &'a [(String, String)],
}

/// Synchronous entry point for `ebuild.rs`: spins up a multi-threaded tokio
/// runtime for the call (a single-threaded one deadlocks mid-run).
/// `PORTAGE_TMPDIR` is read by the caller, so tests can pass it directly.
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
        log_file: None,
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
    pub log_file: Option<&'a Path>,
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
    let pipe = |what: &str| -> Result<(std::fs::File, std::fs::File), String> {
        let mut fds = [0; 2];
        // `O_CLOEXEC`: the pump's read ends must not leak into the phase child.
        // (EOF keys off the write ends, so this is hygiene only.)
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
    };
    let (out_r, out_w) = pipe("build-log stdout")?;
    let (err_r, err_w) = pipe("build-log stderr")?;
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

/// Arguments of [`spawn_bash_script`].
pub(crate) struct SpawnBashScriptArgs<'a> {
    pub(crate) script: &'a Path,
    pub(crate) argv_word: &'a str,
    pub(crate) vars: Vec<(String, String)>,
    pub(crate) iso: Isolation,
    pub(crate) log_file: Option<&'a Path>,
    /// Names the spawn in its failure message (`"phase setup"` reads
    /// `spawning real bash for phase setup failed: …`).
    pub(crate) context: &'a str,
}

/// Run a vendored bash helper script under the sandbox with a curated
/// environment and optional log capture: only whitelisted calling-env
/// vars pass through (`LC_ALL` and the IPC daemon never do), and the
/// log pump guard outlives the command so the gzip thread joins after
/// every write end closes. Shared by the phase and misc-functions
/// spawns so their setup cannot drift apart.
pub(crate) fn spawn_bash_script(args: SpawnBashScriptArgs<'_>) -> Result<i32, String> {
    let SpawnBashScriptArgs {
        script,
        argv_word,
        vars,
        iso,
        log_file,
        context,
    } = args;
    // The pump guard must outlive `cmd` (see `open_log_file`): declared first so
    // drop order destroys it after `cmd`, on every path.
    let mut pump_guard: Option<LogPump> = None;
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
    if let Some(path) = log_file {
        // Only the write ends move into `cmd`; the guard escapes outward so it drops
        // after `cmd`. `drop(cmd)` below closes the copies deterministically.
        let sink = open_log_file(path)?;
        cmd.stdout(sink.out).stderr(sink.err);
        pump_guard = sink.pump;
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for {context} failed: {e}"))?;
    drop(cmd);
    // Explicit: joins the gzip thread now that every write end is closed.
    drop(pump_guard);
    Ok(status.code().unwrap_or(1))
}
