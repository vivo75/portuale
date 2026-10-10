//! Running a phase command list, with logging.

use crate::ebuild_phases::*;

/// Arguments of [`run_commands_async`].
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
    /// overrides them -- the `emerge <atom>` build/merge path passes the
    /// resolved `USE` flags here (`bin/ebuild.sh`'s own `use()` reads the
    /// `USE` var), which `phase_env_vars` otherwise leaves `""`. `&[]` for
    /// a standalone `ebuild <file> <phase>` (no resolved graph entry).
    pub(crate) build_env: &'a [(String, String)],
}

/// Drives `commands` against `ebuild_path` for real: computes the
/// environment once, then for each of `commands` in order, runs its own
/// `phase_prerequisites` chain, each phase in its own fresh embedded
/// brush shell (see `run_one_phase`'s own doc comment for why a fresh
/// shell per phase, not one shared across the whole invocation, is the
/// real, not simplified, model here) sourcing real `bin/ebuild.sh` (see
/// this module's own doc comment) and the ebuild file itself.
///
/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check (see
/// `fetch_sources`'s own doc comment) runs exactly once here, before
/// the phase loop, whenever the combined prerequisite chain includes
/// `unpack` -- matching real portage's own ordering (real `pkg_pretend`
/// explicitly runs *before* fetching, PMS's whole point for that phase
/// being a fast sanity check that shouldn't need network access at
/// all; `setup` likewise precedes it). The resulting `A`/`AA` are
/// exported into *every* phase this call runs, not just `unpack`
/// itself, matching real portage's own environment (every phase sees
/// the same `A`/`AA`, whether or not it happens to reference them).
/// `DISTDIR` itself is always exported too (regardless of whether
/// `unpack` is in the chain at all), matching real portage's own
/// unconditional environment -- real `unpack` (a `bin/ebuild-helpers/`
/// script, not reimplemented) resolves `${A}`'s own files relative to
/// `${DISTDIR}` itself, not anything this Rust code passes it
/// directly, so omitting the export here would silently break real
/// unpacking even after a real, successful fetch (caught empirically:
/// a real fetched-and-verified distfile still made `unpack` report
/// `"either does not exist or is not a regular file"` before this was
/// added).
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
    // The real distdir goes in as `DISTDIR`; `phase_env_vars` remaps it the
    // way real `config.environ()` does (`config.py:3403-3409`): the real
    // directory becomes `PORTAGE_ACTUAL_DISTDIR` (`doins` passes it as
    // `--distdir`, so absolute symlinks under `doins -r` stay symlinks) and
    // `DISTDIR` the per-package fake `${PORTAGE_BUILDDIR}/distdir`, filled
    // by `prepare_fake_distdir` below (#330).
    let mut extra_env = vec![("DISTDIR".to_string(), distdir.display().to_string())];
    extra_env.extend(build_env.iter().cloned());
    // The resolved `USE` threaded by the caller (last `USE` pair, the value
    // the phases themselves see), so `A` names exactly the distfiles
    // `use()` will expect.
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
        // Real `config.environ()` exports `A` but pops `AA` for every
        // EAPI >= 4 (`config.py:3331-3333`, `eapi_exports_AA`); the
        // EAPI floor here is 5+, so `AA` is never exported (S0 finding
        // `l2-env-aa-exported`).
        extra_env.push(("A".to_string(), a.join(" ")));
        Some(a)
    } else {
        None
    };
    // Real `doebuild.py:1463-1469`: the fake distdir is prepared when the
    // requested `mydo` is not `setup` (unless `FEATURES=noauto`), or is
    // `install`/`unpack`. The first test is over the *command*, not the
    // chain: `setup`'s own prerequisite chain starts with `pretend`. The
    // second looks at the whole chain, because portuale's `noauto` still
    // runs the prerequisites and `unpack` needs `${DISTDIR}/<A>`.
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
            // Real `_post_phase_cmds["install"]` (`EbuildPhase.py:424`/
            // `442-461`): real, unconditional `bin/misc-functions.sh
            // install_qa_check install_symlink_html_docs install_hooks`,
            // run once right after a successful real `install` phase --
            // not gated on any `FEATURES` flag, and (unlike `ebuild
            // <file> package`'s own separate `__dyn_package` misc-
            // functions call) never itself part of `phase_prerequisites`'
            // own chain, so this is the one place it can run. `EBUILD_
            // PHASE` stays `"install"` for this call, matching real
            // portage's own behavior (`_PostPhaseCommands` reuses the
            // exact same `settings` the install phase itself already
            // used, never resetting it). Real `bin/misc-functions.sh`'s
            // own `MISC_FUNCTIONS_ARGS="$@"` then unquoted `for x in
            // ${MISC_FUNCTIONS_ARGS}` re-splits on whitespace regardless
            // of how many real argv entries this arrived as, so passing
            // all three names as one space-joined string here is exactly
            // equivalent to real portage's own three separate positional
            // args -- `run_misc_functions` needs no changes at all.
            if phase == "install" {
                // Real `EbuildPhase._ebuild_exit_unlocked` order
                // (`EbuildPhase.py:424-438`): `_post_src_install_write_
                // metadata` and `_post_src_install_uid_fix` run right
                // after `bin/ebuild.sh install` returns and **before**
                // the `install_qa_check` post-phase commands below. That
                // ordering is load-bearing for `SIZE`: it is the
                // *pre-transform* image (`ecompress`/`estrip` run later),
                // real's own 807 bytes for `porttest/docs` against the
                // 372 the compressed image would give.
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
                // Real `EbuildPhase._commands_exit` runs
                // `_post_src_install_soname_symlinks` immediately after
                // that same misc-function sequence (see its own doc
                // comment): the 6-field `NEEDED.ELF.2` (from the
                // post-`estrip` image), generated `PROVIDES`/
                // `REQUIRES`, and `_inject_libc_dep`'s own implicit libc
                // dependency (#39).
                write_post_install_soname_deps(&env, root)?;
            }
        }
    }
    Ok(0)
}

/// Arguments of [`run_commands`].
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

/// Synchronous entry point for `ebuild.rs` (which is not itself async --
/// `emerge`'s own dispatch never needs an async runtime at all, so this
/// portuale doesn't pay for one there; only this one code path does). Spins
/// up a tokio runtime for the duration of the call -- MUST be
/// multi-threaded (`new_multi_thread`, not `new_current_thread`):
/// confirmed empirically (a single-threaded runtime deadlocks partway
/// through a real multi-phase run -- brush-core's own `Cargo.toml`
/// requires tokio's `rt-multi-thread` feature under unix, not just
/// `rt`, which is the same thing portuale rediscovered the hard way).
///
/// `PORTAGE_TMPDIR` (real portage's own `make.globals` default:
/// `/var/tmp`; the builddir adds `portage/<cat>/<pf>` on top) is read
/// by the caller, not internally here --
/// deliberately, so tests can pass a distinct value directly rather than
/// mutating process-global environment state (`std::env::set_var` is
/// unsound to call from parallel test threads), the same "env var read
/// once at the CLI boundary" shape `emerge`'s own `pretend.rs` already
/// uses for `ROOT`/`PORTAGE_CONFIGROOT` via `root_from_env`/
/// `config_root_from_env`.
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

/// Arguments of [`run_commands_logged`].
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

/// Like `run_commands`, but when `log_file` is `Some`, every phase (and
/// its post-phase `misc-functions.sh`) has its stdout+stderr captured to
/// that file (append) instead of the terminal -- real portage's
/// `PORTAGE_LOG_FILE` (default `${T}/build.log`). `run_build_scheduler`
/// passes it so a parallel `--jobs` build's output doesn't interleave;
/// the scheduler dumps the file on a build failure.
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

/// Opens `log_file` for append (creating it and its parent dir),
/// returning the two write ends a phase's stdout+stderr go to plus the
/// gzip pump guard when the path ends in `.gz`
/// (`FEATURES=compress-build-logs`, whose `.gz`-suffixed paths
/// `emerge_build::build_log_path` produces).
///
/// Real `EbuildPhase._open_log` (`_emerge/EbuildPhase.py`): the log
/// file is opened `ab` and, when its name ends `.gz`, wrapped in a
/// `gzip.GzipFile(mode="ab")` -- every phase appends one gzip member to
/// the same file, and the failure-tail / QA readers wrap it in
/// `gzip.GzipFile(mode="rb")` back. The pump thread below is that
/// wrapper: the phase (a real bash child, or the embedded brush shell
/// writing its fd table) writes plain bytes into two pipes, and the
/// thread gzip-encodes them into the `.gz` file -- one member per
/// `open_log_file` call, exactly like real's one `GzipFile` per
/// `_open_log` call. A phase that writes nothing appends no member at
/// all (real's header is deferred to the first write too).
///
/// Lifetime: the guard MUST drop after the phase's write ends close
/// (child waited + parent `Command` dropped, or brush `Shell`
/// dropped), otherwise its join hangs waiting for EOF. Declare the
/// sink early and `drop` the writer holder explicitly -- see the call
/// sites. Joining first also guarantees the `.gz` member is finished
/// before any reader (failure tail, next phase's append, QA scan)
/// touches the file.
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

/// One pipe's read end drained into the shared gzip encoder; returns
/// bytes forwarded. Both pipes' pumps share one `GzEncoder` (under a
/// mutex -- stdout/stderr chunk interleaving was never deterministic),
/// and the member is finished only when at least one byte flowed.
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
        // `O_CLOEXEC`: the pump's read ends must not leak into the
        // phase child across fork/exec (hygiene only -- EOF keys off
        // the write ends).
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
            // Trailer for this phase's member; the next phase
            // appends a fresh member (real's per-`_open_log`
            // `GzipFile` does the same).
            let _ = enc.try_finish();
            let _ = enc.get_mut().sync_all();
        }
        // Else: drop the encoder unfinished -- nothing was ever
        // written, so no header went out either, and the file is left
        // exactly as found (real's deferred header behaves the same).
    });
    Ok(LogSink {
        out: out_w,
        err: err_w,
        pump: Some(LogPump {
            thread: Some(thread),
        }),
    })
}
