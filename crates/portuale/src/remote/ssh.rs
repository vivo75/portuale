//! The ssh transport: argv, control sockets, running scripts, copying files.

use crate::remote::*;

// --- Shell quoting ---------------------------------------------------------

/// Quote one argv word for `bash -s` payloads: single-quote, escaping
/// embedded quotes as `'\''`. Client paths are never interpolated raw.
pub fn sh_quote(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('\'');
    for chunk in word.split('\'') {
        // Rejoin with the close-escape-reopen sequence; the first split
        // boundary needs no opener (already inside quotes).
        if out.len() > 1 {
            out.push_str("'\\''");
        }
        out.push_str(chunk);
    }
    out.push('\'');
    out
}

// --- SSH transport ---------------------------------------------------------

/// Directory for ControlPersist sockets (`0700`). Candidates are
/// tried shortest-first and a base that cannot fit the ~108-byte
/// socket cap is skipped; `None` disables multiplexing (slower,
/// still correct).
pub(crate) fn control_dir() -> Option<std::path::PathBuf> {
    control_dir_for(
        std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from),
        std::env::var_os("HOME").map(std::path::PathBuf::from),
    )
}

/// Worst-case `%C` expansion plus ssh's own multiplex suffix, with
/// margin under the 108-byte `sun_path` cap.
pub(crate) const CONTROL_PATH_BUDGET: usize = 64;

pub(crate) fn control_dir_for(
    xdg_runtime: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(xdg) = xdg_runtime {
        candidates.push(xdg.join(".portuale-cp"));
    }
    if let Some(home) = home {
        candidates.push(home.join(".portuale/cp"));
    }
    candidates.into_iter().find(|dir| {
        if dir.to_string_lossy().len() + CONTROL_PATH_BUDGET >= 108 {
            return false;
        }
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        true
    })
}

/// Build the `ssh` argv (without the remote command): multiplexed,
/// non-interactive (`BatchMode=yes` fails fast instead of prompting).
pub(crate) fn ssh_argv(ctx: &RemoteContext, control: Option<&std::path::Path>) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-p".to_string(), ctx.port.to_string()];
    if let Some(key) = &ctx.key_file {
        argv.push("-i".to_string());
        argv.push(key.clone());
    }
    argv.push("-o".to_string());
    argv.push("BatchMode=yes".to_string());
    argv.push("-o".to_string());
    argv.push(format!("ConnectTimeout={}", ctx.timeout_secs));
    argv.push("-o".to_string());
    argv.push(format!(
        "StrictHostKeyChecking={}",
        ctx.strict_host_key_checking.as_ssh_value()
    ));
    if let Some(dir) = control {
        // `%C` hashes %l%h%p%r -- safe under the ~108-byte unix-socket
        // limit no matter how long the hostname is.
        let socket = dir.join("%C");
        argv.push("-o".to_string());
        argv.push("ControlMaster=auto".to_string());
        argv.push("-o".to_string());
        argv.push("ControlPersist=60s".to_string());
        argv.push("-o".to_string());
        argv.push(format!("ControlPath={}", socket.display()));
    }
    if let Some(extra) = &ctx.ssh_args {
        argv.extend(extra.split_whitespace().map(String::from));
    }
    if let Some(user) = &ctx.user {
        argv.push("-l".to_string());
        argv.push(user.clone());
    }
    argv.push(ctx.hostname.clone());
    argv
}

/// True when an ssh failure is a *transport* problem rather than a
/// remote command failure: exit 255, spawn failure, or classic
/// fatal signatures on stderr.
pub(crate) fn is_transport_error(code: Option<i32>, stderr: &str) -> bool {
    if code == Some(255) {
        return true;
    }
    code.is_none()
        && (stderr.contains("Connection refused")
            || stderr.contains("Connection timed out")
            || stderr.contains("No route to host")
            || stderr.contains("Could not resolve hostname")
            || stderr.contains("Permission denied"))
}

/// The local transport's `bash -s`: the child would inherit the
/// server process's environment, so the server's own VDB selection
/// (the `PORTUALE_VDB_*` vars and the VDB IPC var) is removed and
/// `PORTUALE_BIN` is set. Only this transport needs it; over ssh
/// nothing is forwarded.
pub(crate) fn local_client_bash() -> std::process::Command {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-s");
    for var in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        crate::vdb_ipc::IPC_VAR,
    ] {
        cmd.env_remove(var);
    }
    cmd.env("PORTUALE_BIN", crate::self_exe::self_exe());
    cmd
}

/// Run `bash -s` with `script` on stdin; stdout/stderr captured
/// separately (machine log vs. human log). Over ssh, or as a local
/// subprocess when the transport is `local` (same driver, no network).
pub(crate) fn run_script_stdin(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    script: &str,
) -> Result<std::process::Output, String> {
    use std::io::Write as _;
    let mut child = match ctx.transport {
        RemoteTransport::Ssh => {
            let mut argv = ssh_argv(ctx, control);
            argv.push("bash".to_string());
            argv.push("-s".to_string());
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))?
        }
        RemoteTransport::Local => local_client_bash()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("mrg: cannot spawn local bash: {e}"))?,
    };
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: child stdin unavailable".to_string())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("mrg: writing to child stdin: {e}"))?;
    child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for child: {e}"))
}

/// Run an arbitrary remote command (`tar`, …), not just `bash -s`.
/// stdout bytes come back to the caller (used by the config pull).
/// `remote_argv` is an argv, never shell text: over ssh each word is
/// `sh_quote`d, so a client path with spaces stays one word.
pub(crate) fn run_raw_command(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_argv: &[String],
) -> Result<std::process::Output, String> {
    match ctx.transport {
        RemoteTransport::Local => {
            let (head, tail) = remote_argv
                .split_first()
                .ok_or_else(|| "mrg: empty remote command".to_string())?;
            std::process::Command::new(head)
                .args(tail)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("mrg: cannot spawn local command: {e}"))
        }
        RemoteTransport::Ssh => {
            let mut argv = ssh_argv(ctx, control);
            argv.extend(remote_argv.iter().map(|word| sh_quote(word)));
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))
        }
    }
}

/// Pull a remote directory to a local one (`tar -c` remotely, `tar -x`
/// locally). Used for `/etc/portage` under the `client:` placement.
pub(crate) fn pull_dir(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_dir: &str,
    local_dir: &std::path::Path,
) -> Result<(), String> {
    let output = run_raw_command(
        ctx,
        control,
        &[
            "tar".to_string(),
            "-c".to_string(),
            "-C".to_string(),
            remote_dir.to_string(),
            ".".to_string(),
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "mrg: pulling {remote_dir} from {} failed (exit {})",
            ctx.hostname,
            output.status.code().unwrap_or(-1)
        ));
    }
    std::fs::create_dir_all(local_dir).map_err(|e| format!("{}: {e}", local_dir.display()))?;
    let mut child = std::process::Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(local_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("mrg: cannot spawn local tar: {e}"))?;
    use std::io::Write as _;
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: tar stdin unavailable".to_string())?
        .write_all(&output.stdout)
        .map_err(|e| format!("mrg: feeding local tar: {e}"))?;
    let unpack = child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for local tar: {e}"))?;
    if !unpack.status.success() {
        return Err("mrg: unpacking pulled config failed".to_string());
    }
    Ok(())
}

/// Ship local file bytes to `<workdir>/<name>` on the client: `sh -c
/// 'cat > …'` over ssh (the `>` must be remote-side, hence the explicit
/// `sh -c` with one pre-quoted word), plain `fs::copy` for local.
pub(crate) fn send_file(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    local_path: &std::path::Path,
    dest: &str,
) -> Result<(), String> {
    let bytes = std::fs::read(local_path).map_err(|e| format!("{}: {e}", local_path.display()))?;
    send_bytes(ctx, control, &bytes, dest)
}

/// Ship in-memory bytes to a client path: the `send_file` transport
/// for bytes the server never wrote to disk (old-hook envs staged
/// for the merge driver).
pub(crate) fn send_bytes(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    bytes: &[u8],
    dest: &str,
) -> Result<(), String> {
    match ctx.transport {
        RemoteTransport::Local => {
            if let Some(parent) = std::path::Path::new(dest).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(dest, bytes).map_err(|e| format!("mrg: writing {dest} failed: {e}"))?;
            Ok(())
        }
        RemoteTransport::Ssh => {
            use std::io::Write as _;
            let mut argv = ssh_argv(ctx, control);
            argv.push("sh".to_string());
            argv.push("-c".to_string());
            argv.push(format!("cat > {}", sh_quote(dest)));
            let mut child = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))?;
            child
                .stdin
                .take()
                .ok_or_else(|| "mrg: ssh stdin unavailable".to_string())?
                .write_all(bytes)
                .map_err(|e| format!("mrg: writing to ssh stdin: {e}"))?;
            let output = child
                .wait_with_output()
                .map_err(|e| format!("mrg: waiting for ssh: {e}"))?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() && is_transport_error(code, &stderr) {
                return Err(format!(
                    "mrg: client {} unreachable:\n{stderr}",
                    ctx.hostname
                ));
            }
            if !output.status.success() {
                return Err(format!(
                    "mrg: sending {dest} failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            Ok(())
        }
    }
}
