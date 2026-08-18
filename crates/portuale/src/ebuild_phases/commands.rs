//! Running a phase command list, with logging.

use crate::ebuild_phases::*;

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
