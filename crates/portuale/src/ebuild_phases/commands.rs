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
    /// Portage's merge-path `EbuildExecuter`: skip the no-op default
    /// `unpack`/`prepare`/`configure`/`compile`/`test` bodies for a
    /// sourceless package (see [`can_skip_source_phases`]). The direct
    /// `ebuild` paths (and a direct `ebuild <file> package`) run every
    /// phase like `doebuild()`; the scheduler-driven `--buildpkgonly`
    /// build goes through `EbuildExecuter` and skips (backlog #458).
    pub(crate) skip_noop_src_phases: bool,
    /// The scheduler's up-front `pkg_pretend` pass already ran it: start
    /// the chain at `setup`, like `EbuildBuild` does after `_run_pkg_pretend`.
    pub(crate) skip_pretend: bool,
}

/// Portage's `EbuildExecuter._skippable_src_phases`: the phases whose
/// default bodies are no-ops for a package with no source to act on.
/// `src_install` always runs: it creates `${D}` and drives the
/// install-time QA checks.
// Portage: _emerge/EbuildExecuter.py
const SKIPPABLE_SRC_PHASES: &[&str] = &["unpack", "prepare", "configure", "compile", "test"];

/// Whether the merge path runs `unpack` and the `src_*` phases at all:
/// Portage's `EbuildExecuter._can_skip_source_phases`, evaluated after
/// `pkg_setup` (which records `.src_patches` when `PATCHES` is set).
/// Skipped phases never spawn, never log and never stamp.
// Portage: _emerge/EbuildExecuter.py
pub(crate) fn can_skip_source_phases(
    features: &str,
    use_flags: &str,
    env: &Environment,
    a_is_empty: bool,
) -> bool {
    // The `ebuild` command's manual mode relies on running each phase.
    if feature_token_present(features, "noauto") {
        return false;
    }
    // Live ebuilds fetch their source in `src_unpack` regardless of `SRC_URI`.
    let use_set: std::collections::HashSet<String> =
        use_flags.split_whitespace().map(String::from).collect();
    let (_, properties) = restrict_and_properties(env, &use_set);
    if properties.split_whitespace().any(|w| w == "live") {
        return false;
    }
    // Nothing to unpack, and `A` is what the fetch above resolved.
    if !a_is_empty {
        return false;
    }
    // An ebuild-defined `src_*` phase may do real work. `DEFINED_PHASES`
    // covers the eclass stack too; without metadata the phases cannot be
    // proven no-ops, so they run.
    let Some(defined) = repo_root_for(&env.pkg_dir)
        .and_then(|repo_root| {
            portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()
        })
        .and_then(|m| m.get("DEFINED_PHASES").cloned())
    else {
        return false;
    };
    if defined
        .split_whitespace()
        .any(|p| SKIPPABLE_SRC_PHASES.contains(&p))
    {
        return false;
    }
    // `pkg_setup` records `.src_patches` when `PATCHES` is set; the
    // default `src_prepare` applies it, so the skip must not fire.
    !env.portage_builddir().join(".src_patches").exists()
}

/// Drives `commands` against `ebuild_path`: environment once, then each
/// command's `phase_prerequisites` chain in a fresh shell per phase.
/// Fetch runs once up front when the chain includes `unpack`; a successful
/// `install` is followed by post-install metadata, the `install_qa_check ...`
/// sequence (with `EBUILD_PHASE` popped, like Portage's `MiscFunctionsProcess`),
/// and the soname deps.
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
        skip_noop_src_phases,
        skip_pretend,
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
    if let PhaseLog::Tee(path) | PhaseLog::Capture(path) = log_file {
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
        // `A`-emptiness for the no-op src-phase skip below: the fetched
        // list when the chain unpacks (a chain holding a skippable phase
        // always does, so `None` only means "not computed" and runs the
        // phases).
        let a_is_empty = a.as_ref().is_some_and(|a| a.is_empty());
        if (command != "setup" && !features.split_whitespace().any(|f| f == "noauto"))
            || chain_here.iter().any(|&p| p == "install" || p == "unpack")
        {
            let alist = match a {
                Some(a) => a,
                None => package_a(&env, &use_flags)?,
            };
            prepare_fake_distdir(&env, distdir, &alist)?;
        }

        // Portage's merge-path `EbuildExecuter` gate, decided once at the
        // first skippable phase (after `pkg_setup`, which records
        // `.src_patches`): a sourceless package defining no `src_*`
        // phase skips the no-op defaults outright.
        let mut skip_src: Option<bool> = None;
        for phase in chain_here {
            // The scheduler pre-ran `pkg_pretend` up front and replays its
            // output itself: the build starts at `setup`, like `EbuildBuild`.
            if skip_pretend && phase == "pretend" {
                continue;
            }
            if skip_noop_src_phases
                && SKIPPABLE_SRC_PHASES.contains(&phase)
                && *skip_src.get_or_insert_with(|| {
                    can_skip_source_phases(&features, &use_flags, &env, a_is_empty)
                })
            {
                continue;
            }
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
            // Portage: _emerge/EbuildPhase.py, _post_phase_cmds — after a
            // successful `install`, run `install_qa_check
            // install_symlink_html_docs install_hooks` here, on every path:
            // `doebuild()` reaches the same `_PostPhaseCommands` through its
            // synchronous `EbuildPhase` (`_spawn_phase`), so a direct
            // `ebuild` run QA-checks exactly like a merge. `misc-functions.sh`
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
                    // Portage: _emerge/EbuildPhase.py,
                    // MiscFunctionsProcess._spawn — a misc spawn is not a
                    // real phase, so `EBUILD_PHASE` is popped and `use()`'s
                    // `IUSE`-membership gate stays off (a repo-local check
                    // like `60noop-testsuites`' `use test` must not die for
                    // an ebuild without that flag).
                    drop_ebuild_phase: true,
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
    /// Portage's merge-path `EbuildExecuter`: skip the no-op default
    /// `unpack`/`prepare`/`configure`/`compile`/`test` bodies for a
    /// sourceless package. The direct `ebuild` and `package` paths run
    /// every phase like `doebuild()`.
    pub skip_noop_src_phases: bool,
    /// The scheduler's up-front `pkg_pretend` pass already ran it: start
    /// the chain at `setup`. Only the scheduler (and the serial loop /
    /// `--buildpkgonly` once they run the same pass) sets this; every
    /// other caller runs the whole chain from `pretend`.
    /// Portage: _emerge/Scheduler.py, _run_pkg_pretend
    pub skip_pretend: bool,
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
        skip_noop_src_phases,
        skip_pretend,
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
        skip_noop_src_phases,
        skip_pretend,
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
    /// Portage's merge-path `EbuildExecuter`: skip the no-op default
    /// `unpack`/`prepare`/`configure`/`compile`/`test` bodies for a
    /// sourceless package. The direct `ebuild` and `package` paths run
    /// every phase like `doebuild()`.
    pub skip_noop_src_phases: bool,
    /// The scheduler's up-front `pkg_pretend` pass already ran it: start
    /// the chain at `setup`. Only the scheduler sets this; every other
    /// caller runs the whole chain from `pretend`.
    pub skip_pretend: bool,
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
        skip_noop_src_phases,
        skip_pretend,
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
        skip_noop_src_phases,
        skip_pretend,
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
    /// The phase's `CATEGORY` and `PF`: the spawn names itself
    /// `[<cat>/<pf>] bash|sandbox` after Portage's `spawn` `opt_name`.
    pub(crate) category: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) log_file: PhaseLog<'a>,
    /// Names the spawn in its failure message (`"phase setup"` reads
    /// `spawning real bash for phase setup failed: …`).
    pub(crate) context: &'a str,
    /// The package's raw `PROPERTIES` names `interactive`.
    pub(crate) interactive: bool,
    /// Demote the child to this identity before exec (`None` keeps root).
    /// Only the gated phase spawns set it; `misc_functions` never demotes.
    /// Portage: doebuild.py, spawn (`droppriv`, :2171-2181)
    pub(crate) droppriv: Option<PhaseIds>,
}

/// Maps `PORTUALE_TEST_LOOPBACK_FAIL` to the sender's `fail_at` step code
/// (`""|"getlink"|"linkup"|"v4"|"v6"`, unset/empty/garbage → `0`, normal).
/// Parent-side only: the child (`pre_exec`) cannot allocate to read env,
/// so the value rides into the closure as a `u8` alongside `has_ipv6` and
/// the warning table.
#[cfg(target_os = "linux")]
fn loopback_fail_at_env() -> u8 {
    match std::env::var("PORTUALE_TEST_LOOPBACK_FAIL").as_deref() {
        Ok("getlink") => 1,
        Ok("linkup") => 2,
        Ok("v4") => 3,
        Ok("v6") => 4,
        _ => 0,
    }
}

/// Portage's loopback-setup warning lines, identical by design: the step is
/// deliberately unnamed (Portage names none either), so only the errno
/// varies run to run. Parent-built (never `pre_exec`): index `errno - 1`,
/// with [`LOOPBACK_WARN_FALLBACK`] past the end. Each body is the bare
/// `strerror` text (never `io::Error`'s Display, which appends
/// " (os error N)"); the `strerror` body is locale-dependent, so tests pin
/// only the prefix and the absence of the " (os error" suffix.
// Portage: process.py, _configure_loopback_interface (:867-871).
#[cfg(target_os = "linux")]
pub(crate) fn loopback_warn_table() -> Vec<Vec<u8>> {
    (1..=135)
        .map(|errno: i32| {
            // `strerror_r` (XSI, `__xpg_strerror_r` on glibc) writes into a
            // caller-owned stack buffer: thread-safe, unlike `strerror`'s
            // shared static buffer, and the table is built per spawn.
            let mut buf = [0u8; 256];
            // SAFETY: `buf` is a live 256-byte stack buffer and the length
            // passed is its exact size.
            let rc = unsafe {
                libc::strerror_r(errno, buf.as_mut_ptr() as *mut libc::c_char, buf.len())
            };
            let text = if rc == 0 {
                std::ffi::CStr::from_bytes_until_nul(&buf)
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| format!("Unknown error {errno}"))
            } else {
                format!("Unknown error {errno}")
            };
            format!("Unable to configure loopback interface: {text}\n").into_bytes()
        })
        .collect()
}

/// The child's loopback-warning lookup: `errno` (a `u8` table index, `1`-based)
/// maps to its [`loopback_warn_table`] line; `0` or any errno past the table
/// yields [`LOOPBACK_WARN_FALLBACK`]. Pure and allocation-free, so the
/// `pre_exec` closure calls it directly.
// Portage: process.py, _configure_loopback_interface (:867-871).
#[cfg(target_os = "linux")]
pub(crate) fn loopback_warn_line(table: &[Vec<u8>], errno: u8) -> &[u8] {
    errno
        .checked_sub(1)
        .and_then(|idx| table.get(idx as usize))
        .map(Vec::as_slice)
        .unwrap_or(LOOPBACK_WARN_FALLBACK)
}

/// The out-of-range loopback warning: a `const` byte string (no allocation,
/// so the child can borrow it) for errnos past the table. Same Portage
/// prefix. It intentionally elides the errno number and the `strerror` body:
/// the arm is unreachable in practice (real Linux errnos are < 134 and the
/// `u8` index covers them), and building the text would allocate in `pre_exec`.
// Portage: process.py, _configure_loopback_interface (:867-871).
#[cfg(target_os = "linux")]
pub(crate) const LOOPBACK_WARN_FALLBACK: &[u8] =
    b"Unable to configure loopback interface: Unknown error\n";

/// Bring `lo` up in the fresh netns via raw netlink: `RTM_GETLINK` for the
/// name `lo`, then link-up (`IFF_UP`), then `10.0.0.1/8`, then `fd::1/8`
/// when the host stack does IPv6. Returns the first failure as
/// `(step, errno)`: `0` ok, `1` GETLINK, `2` link-up, `3` v4, `4` v6 — the
/// first failure aborts the rest (Portage warn-and-abandon). The caller then
/// writes the parent-built table line for that errno to fd 2 with a single
/// `write(2)`. `errno` is truncated to a `u8` table index (real Linux errnos
/// stay below 134, so no bits are lost). Best-effort: the phase still runs
/// with `lo` down, like a host without
/// `ip`. One ACK/`NLMSG_ERROR` is drained per request; a nonzero error
/// there is that step's failure.
/// Portage: process.py, _configure_loopback_interface (:838-871);
/// lib/portage/util/netlink.py, RtNetlink (:63-114).
#[cfg(target_os = "linux")]
fn configure_loopback_netlink(has_ipv6: bool, fail_at: u8) -> (u8, u8) {
    use std::ffi::c_void;
    // SAFETY: runs in the child after fork, before exec (called from
    // `pre_exec`); only raw syscalls on stack buffers and owned fds, no
    // allocation, no locks, no print. Every failure path closes the socket
    // (when open) and returns.
    unsafe {
        // `fail_at` is the test hook (`PORTUALE_TEST_LOOPBACK_FAIL`, mapped
        // by the parent): the named step is skipped and reported failed.
        let trunc = |errno: i32| errno as u8;
        let errno_now = || {
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO)
        };
        if fail_at == 1 {
            return (1, trunc(libc::EIO));
        }
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM, libc::NETLINK_ROUTE);
        if fd < 0 {
            return (1, trunc(errno_now()));
        }
        let (req, reqlen) = lo_getlink_req();
        if libc::send(fd, req.as_ptr() as *const c_void, reqlen, 0) < 0 {
            let errno = errno_now();
            libc::close(fd);
            return (1, trunc(errno));
        }
        let mut reply = [0u8; 512];
        let got = libc::recv(fd, reply.as_mut_ptr() as *mut c_void, reply.len(), 0);
        if got < 0 {
            // EINTR included: a signal won the race, and degrading to a
            // warning plus no-lo is the safe direction.
            let errno = errno_now();
            libc::close(fd);
            return (1, trunc(errno));
        }
        // Anything but a well-formed `RTM_NEWLINK` carrying an ifindex is a
        // protocol error: unexpected reply bytes are a protocol error,
        // unless the kernel named an errno itself via `NLMSG_ERROR`.
        let ty = u16::from_ne_bytes([reply[4], reply[5]]);
        if got < 32 || ty != lo_nl::RTM_NEWLINK {
            let errno = if ty == lo_nl::NLMSG_ERROR && got >= 20 {
                let raw = i32::from_ne_bytes([reply[16], reply[17], reply[18], reply[19]]);
                if -raw != 0 { -raw } else { libc::EPROTO }
            } else {
                libc::EPROTO
            };
            libc::close(fd);
            return (1, trunc(errno));
        }
        let ifindex = u32::from_ne_bytes([reply[20], reply[21], reply[22], reply[23]]);
        if ifindex == 0 {
            libc::close(fd);
            return (1, trunc(libc::EPROTO));
        }
        let mut ack = [0u8; 64];
        // One `NLM_F_ACK` request: a nonzero `NLMSG_ERROR` is that step's
        // failure (Portage `parse_message` raises there too). `Some` aborts
        // the rest via the early returns below.
        let mut roundtrip = |step: u8, buf: &[u8]| -> Option<(u8, u8)> {
            if fail_at == step {
                return Some((step, trunc(libc::EIO)));
            }
            if libc::send(fd, buf.as_ptr() as *const c_void, buf.len(), 0) < 0 {
                return Some((step, trunc(errno_now())));
            }
            let got = libc::recv(fd, ack.as_mut_ptr() as *mut c_void, ack.len(), 0);
            if got < 0 {
                return Some((step, trunc(errno_now())));
            }
            if got >= 20 && u16::from_ne_bytes([ack[4], ack[5]]) == lo_nl::NLMSG_ERROR {
                let raw = i32::from_ne_bytes([ack[16], ack[17], ack[18], ack[19]]);
                if -raw != 0 {
                    return Some((step, trunc(-raw)));
                }
            }
            None
        };
        let (up, n) = lo_link_up_msg(ifindex);
        if let Some(failure) = roundtrip(2, &up[..n]) {
            libc::close(fd);
            return failure;
        }
        let (v4, n4) = lo_add_v4_msg(ifindex);
        if let Some(failure) = roundtrip(3, &v4[..n4]) {
            libc::close(fd);
            return failure;
        }
        if has_ipv6 {
            let (v6, n6) = lo_add_v6_msg(ifindex);
            if let Some(failure) = roundtrip(4, &v6[..n6]) {
                libc::close(fd);
                return failure;
            }
        }
        libc::close(fd);
        (0, 0)
    }
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
        category,
        pf,
        log_file,
        context,
        interactive,
        droppriv,
    } = args;
    // The guards must outlive `cmd` (see `open_log_file`): declared first so
    // drop order destroys them after `cmd`, on every path.
    let mut pump_guard: Option<LogPump> = None;
    let mut tee_guard: Option<TeePump> = None;
    // Portage: doebuild.py, spawn (:2233-2261) — `free` when demoted
    // without `usersandbox` (no sandbox wrapper here), ` bash` when free
    // or already inside one (`SANDBOX_ACTIVE` in the parent env),
    // ` sandbox` otherwise. No fakeroot wrapper, so no third suffix.
    // `FEATURES` rides in `vars` (phase_env always sets it), which is the
    // only `usersandbox` signal this shared spawn sees.
    let usersandbox = vars
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .is_some_and(|(_, v)| feature_token_present(v, "usersandbox"));
    let free = droppriv.is_some() && !usersandbox;
    let sandboxed = iso.fs_sandbox && !free && std::env::var_os("SANDBOX_ACTIVE").is_none();
    let opt_name = phase_opt_name(category, pf, sandboxed);
    // SAFETY: `geteuid(2)` cannot fail.
    let is_root = unsafe { libc::geteuid() } == 0;
    // A demoted phase never takes the `unshare(1)` shim: the shim would demote
    // the wrapper process instead of the phase. The spawn is direct — with the
    // same fully-built argv (`libsandbox` prefix included) — and namespaces
    // happen in the child's own `pre_exec` (raw `unshare(2)` while still
    // root), below.
    let mut cmd = if droppriv.is_some() {
        direct_command(&phase_argv(script, argv_word, &iso), &opt_name)
    } else {
        sandbox_wrapped_command(script, argv_word, iso, &opt_name, is_root)
    };
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
    // Only `config` and `interactive` packages keep the terminal as stdin.
    // Portage: AbstractEbuildProcess.py, _start
    if argv_word != "config" && !interactive {
        cmd.stdin(std::process::Stdio::null());
    }
    // The loopback warning table (Linux demoted+net only): parent-built —
    // the child (`pre_exec`) cannot allocate — and moved into the closure
    // below alongside `hostname`/`has_ipv6`/`fail_at`, which reports the
    // first netlink failure with a single child-side `write(2)` to fd 2.
    #[cfg(target_os = "linux")]
    let loopback_warn = loopback_warn_table();
    #[cfg(target_os = "linux")]
    let loopback_fail_at: u8 = loopback_fail_at_env();
    if let Some(ids) = &droppriv {
        let uid = ids.uid;
        let gid = ids.gid;
        let groups = ids.groups.clone();
        // Parent-built: the child (`pre_exec`) cannot allocate. `localhost`
        // is what Portage sets in the new UTS namespace (process.py:1246-1261).
        #[cfg(target_os = "linux")]
        let hostname = std::ffi::CString::new("localhost").expect("the literal carries no NUL");
        // Parent-built: whether the v6 loopback address is even attempted
        // (Portage `has_ipv6`, process.py:823-835). The child cannot probe:
        // `UdpSocket::bind` allocates.
        #[cfg(target_os = "linux")]
        let has_ipv6 = host_has_ipv6();
        // SAFETY: runs in the child after fork, before exec; only
        // async-signal-safe libc calls, and any failure fails the spawn.
        // Portage: process.py, _exec (:1265-1274) — setgid, setgroups,
        // then setuid, so the supplementary groups land before root is
        // dropped; the umask follows last.
        use std::os::unix::process::CommandExt as _;
        unsafe {
            cmd.pre_exec(move || {
                // Portage: process.py, _exec (:1084, validated first) + _exec2
                // mount/hostname. Raw namespaces while still root: no userns
                // appears on this path because no uid mapping is needed.
                #[cfg(target_os = "linux")]
                {
                    let clone_flags = clone_flags_for(&iso);
                    if clone_flags != 0 && libc::unshare(clone_flags) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // Flags 0 (Linux degraded combo) skips namespace setup and
                    // proceeds straight to demotion — today's behavior.
                    if iso.mount || iso.pid {
                        // Portage _exec2: mark / slave so mounts cannot escape
                        // (:1215-1225). Failure is ignored with continuation:
                        // Portage warns; `pre_exec` stays silent here (its
                        // one reporting exception is the loopback `write(2)`
                        // below).
                        // SAFETY: the literal carries its own NUL; NULL source,
                        // fstype, and data request a pure propagation change.
                        let slash = b"/\0";
                        let _ = libc::mount(
                            std::ptr::null(),
                            slash.as_ptr() as *const libc::c_char,
                            std::ptr::null(),
                            libc::MS_SLAVE | libc::MS_REC,
                            std::ptr::null(),
                        );
                    }
                    if iso.net {
                        // Portage _exec2: the hostname in the new UTS ns
                        // (:1246-1261). The result is ignored (benign either way).
                        // SAFETY: pointer and length come from the live
                        // parent-built CString moved into this closure.
                        let _ = libc::sethostname(hostname.as_ptr(), hostname.as_bytes().len());
                        // Portage _exec2: loopback in the new net ns via
                        // netlink (:838-871) — link up, 10.0.0.1, fd::1 iff
                        // the parent saw IPv6. No subprocess may run in
                        // `pre_exec`, so raw syscalls on stack buffers, plus
                        // one raw `write(2)` below — the single reporting
                        // exception, and no allocation, stdio, or subprocess.
                        // Best-effort: the first failure aborts the rest
                        // (Portage warn-and-abandon) and the parent-built
                        // table line for its errno goes straight to fd 2,
                        // which IS the phase stderr here: `dup2` runs before
                        // hooks on this toolchain, so the warning lands in
                        // the log file, the gzip pump member, the tee pipe,
                        // or the terminal — whichever stderr is (S0
                        // `s0-fd2-preexec.log`). On a toolchain that reorders
                        // `dup2` after hooks the line still lands on the
                        // parent stderr: visible, never lost. The phase
                        // itself still runs.
                        // Pre-fork: the shared netns covers the pid
                        // grandchild below, so it inherits lo-up.
                        // SAFETY: `write(2)` on fd 2 with the live table
                        // line's pointer/length; best-effort, never fails
                        // the spawn.
                        let (loopback_step, loopback_errno) =
                            configure_loopback_netlink(has_ipv6, loopback_fail_at);
                        if loopback_step != 0 {
                            let line = loopback_warn_line(&loopback_warn, loopback_errno);
                            let _ = libc::write(
                                2,
                                line.as_ptr() as *const std::ffi::c_void,
                                line.len(),
                            );
                        }
                    }
                    // Portage: pid-ns-init re-exec shape without the re-exec —
                    // the grandchild IS pid 1 by fork order, so it execs
                    // directly while the middle supervises.
                    // process.py:1111-1145.
                    if iso.pid {
                        // Block the forward set before forking so no signal
                        // lands between fork and the supervise loop
                        // (process.py:1132). Stack-local `sigset_t`: no
                        // `static` slot, so parallel demoted+pid spawns under
                        // `JOBS>1` cannot cross-forward.
                        // SAFETY: `forward` is a live stack slot; the set
                        // functions only touch it.
                        let mut forward: libc::sigset_t = std::mem::zeroed();
                        libc::sigemptyset(&mut forward);
                        for sig in [
                            libc::SIGINT,
                            libc::SIGTERM,
                            libc::SIGHUP,
                            libc::SIGTSTP,
                            libc::SIGCONT,
                        ] {
                            libc::sigaddset(&mut forward, sig);
                        }
                        if libc::pthread_sigmask(libc::SIG_BLOCK, &forward, std::ptr::null_mut())
                            != 0
                        {
                            return Err(std::io::Error::last_os_error());
                        }
                        match libc::fork() {
                            -1 => return Err(std::io::Error::last_os_error()),
                            0 => {
                                // Grandchild: pid 1 by fork order. Fresh
                                // `/proc` over the new pid ns — make-slave
                                // first, like Portage (process.py:1228), then
                                // the mount; either failing is fatal there
                                // (:1226-1245), `Err` here.
                                // SAFETY: the literals carry their own NULs;
                                // NULL source/fstype/data request a pure
                                // propagation change and a proc mount.
                                let proc = b"/proc\0";
                                if libc::mount(
                                    std::ptr::null(),
                                    proc.as_ptr() as *const libc::c_char,
                                    std::ptr::null(),
                                    libc::MS_SLAVE,
                                    std::ptr::null(),
                                ) != 0
                                {
                                    return Err(std::io::Error::last_os_error());
                                }
                                let source = b"proc\0";
                                let fstype = b"proc\0";
                                if libc::mount(
                                    source.as_ptr() as *const libc::c_char,
                                    proc.as_ptr() as *const libc::c_char,
                                    fstype.as_ptr() as *const libc::c_char,
                                    0,
                                    std::ptr::null(),
                                ) != 0
                                {
                                    return Err(std::io::Error::last_os_error());
                                }
                                // `execve` resets handlers, never the mask: the
                                // phase must not inherit the blocked set.
                                // SAFETY: `forward` is still a live stack slot.
                                if libc::pthread_sigmask(
                                    libc::SIG_UNBLOCK,
                                    &forward,
                                    std::ptr::null_mut(),
                                ) != 0
                                {
                                    return Err(std::io::Error::last_os_error());
                                }
                                // Falls through to demotion below, then execs.
                            }
                            grandchild => {
                                // Middle: the pid-1 reaper re-expressed without
                                // Python — owns the session the registry kills
                                // by pid (`ParentGroup`), forwards each caught
                                // signal to the grandchild, polls its exit,
                                // and never returns.
                                // SAFETY: a fresh fork is never a group leader.
                                if libc::setsid() < 0 {
                                    // SAFETY: plain ints; the grandchild is ours.
                                    libc::kill(grandchild, libc::SIGKILL);
                                    return Err(std::io::Error::last_os_error());
                                }
                                // Close the spawn error pipe after the fallible
                                // setup: `spawn()` reads EOF off it, and only an
                                // exec-or-exit delivers that — without this the
                                // spawn would block for the whole phase and the
                                // scheduler could never kill the run mid-phase. One
                                // async-signal-safe syscall; pre-5.9 kernels
                                // answer ENOSYS and degrade to the late
                                // return, still correct.
                                // SAFETY: at `pre_exec` time fds above 2 are
                                // only std's CLOEXEC error pipe — stdio is 0,
                                // 1, 2 and the grandchild keeps its own copies.
                                let _ = libc::close_range(3, u32::MAX, 0);
                                loop {
                                    // Forward every pending forward-set signal
                                    // (zero timeout: poll, never sleep here).
                                    // SAFETY: `forward`/`info`/`timeout` are
                                    // live stack slots.
                                    let mut info: libc::siginfo_t = std::mem::zeroed();
                                    let timeout = libc::timespec {
                                        tv_sec: 0,
                                        tv_nsec: 0,
                                    };
                                    while libc::sigtimedwait(&forward, &mut info, &timeout) >= 0 {
                                        // The grandchild may have just exited;
                                        // a failed forward is benign.
                                        // SAFETY: plain ints.
                                        libc::kill(grandchild, info.si_signo);
                                    }
                                    // Poll the grandchild's exit without
                                    // blocking (WNOHANG); the reaped code is
                                    // the phase's own.
                                    // SAFETY: `status` is a live stack slot.
                                    let mut status = 0;
                                    if libc::waitpid(grandchild, &mut status, libc::WNOHANG)
                                        == grandchild
                                    {
                                        if libc::WIFEXITED(status) {
                                            libc::_exit(libc::WEXITSTATUS(status));
                                        }
                                        if libc::WIFSIGNALED(status) {
                                            let sig = libc::WTERMSIG(status);
                                            // Re-raise like pid-ns-init:140-144:
                                            // reset to default, unblock (the
                                            // forward set is still blocked),
                                            // then signal self. The `_exit`
                                            // only runs if the raise somehow
                                            // returns.
                                            // SAFETY: plain ints; the mask
                                            // slot is live.
                                            libc::signal(sig, libc::SIG_DFL);
                                            libc::pthread_sigmask(
                                                libc::SIG_UNBLOCK,
                                                &forward,
                                                std::ptr::null_mut(),
                                            );
                                            libc::kill(libc::getpid(), sig);
                                            libc::_exit(127);
                                        }
                                    }
                                    // Ten milliseconds between polls.
                                    // SAFETY: both slots live; no old timeout wanted.
                                    let pause = libc::timespec {
                                        tv_sec: 0,
                                        tv_nsec: 10_000_000,
                                    };
                                    libc::nanosleep(&pause, std::ptr::null_mut());
                                }
                            }
                        }
                    }
                }
                // With `pid` the grandchild above already mounted a fresh
                // `/proc` over the new pid ns; without `pid` this path never
                // remounts, so `/proc` keeps the host view. `lo` came up
                // pre-fork via netlink (#636) when `iso.net`, so the pid-1
                // grandchild inherits it through the shared netns.
                if libc::setgid(gid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if !groups.is_empty() && libc::setgroups(groups.len(), groups.as_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setuid(uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Portage: process.py, _exec — umask after setuid (:1273-1274);
                // keywords carry "umask": 0o22 on the droppriv path (doebuild.py:2178).
                // umask(2) cannot fail: it returns the previous mask.
                libc::umask(0o22);
                Ok(())
            });
        }
    }
    // A demoted+pid phase runs the fork dance in `pre_exec` (its middle owns
    // a session via its own `setsid`), so it stays in the parent's group:
    // `spawn_trackable` chains `pre_exec` hooks in registration order — the
    // green demotion+egress suite runs both the demote and the `NewSession`
    // `setsid` closures — so a `NewSession` hook here would run *in the
    // grandchild*, splitting it out of the middle's process group and making
    // a group-kill miss the phase. `ParentGroup` adds no second hook, and its
    // kills are by pid, never `kill(-pgid)` (scheduler_registry.rs:157-160).
    // Linux-gated: elsewhere `iso.pid` sets no namespaces (the clone stub
    // is 0).
    let demoted_pid = cfg!(target_os = "linux") && droppriv.is_some() && iso.pid;
    let placement = if demoted_pid {
        PhasePlacement::ParentGroup
    } else {
        PhasePlacement::for_phase(argv_word)
    };
    let status = spawn_trackable(&mut cmd, placement)
        .map_err(|e| format!("spawning real bash for {context} failed: {e}"))?;
    drop(cmd);
    // Explicit: joins the pump threads now that every write end is closed.
    drop(pump_guard);
    drop(tee_guard);
    Ok(status.code().unwrap_or(1))
}
