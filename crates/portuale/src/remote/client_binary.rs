//! Finding, checking and installing portuale on the client.

use crate::remote::*;

// --- Client portuale binary ----------------------------------------------------

/// Default client search directories, in D5 order.
pub(crate) fn default_bin_dirs() -> Vec<String> {
    vec!["/opt/bin".to_string(), "/usr/local/bin".to_string()]
}

/// Candidate directories: only the `--remote-portuale-dir` pair
/// when that option is given, else the default pair.
pub(crate) fn bin_search_dirs(ctx: &RemoteContext) -> Vec<String> {
    match &ctx.portuale_dir {
        Some(dir) => vec![dir.clone()],
        None => default_bin_dirs(),
    }
}

/// Candidate files in check order: per directory, `portuale` then
/// `portuale-<hash>`. The first file whose full digest matches is used.
pub(crate) fn bin_candidates(dirs: &[String], short_hash: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dir in dirs {
        out.push(format!("{dir}/portuale"));
        out.push(format!("{dir}/portuale-{short_hash}"));
    }
    out
}

/// The server-side identity of the binary to ship.
pub(crate) struct ServerBinInfo {
    /// The file to stream (`--remote-portuale-binary`, or `current_exe()`).
    pub path: std::path::PathBuf,
    /// Full SHA-256 hex of the file.
    pub digest_hex: String,
    /// First 16 hex digits of the digest (the `<hash>` in file names).
    pub short_hash: String,
    /// ELF `e_machine` of the file.
    pub machine: u16,
}

/// Read the shipped binary's ELF `e_machine`: bytes 18-19, in the
/// endianness byte 5 names. Header bytes are read directly; no new crate.
pub(crate) fn elf_machine(bytes: &[u8]) -> Result<u16, String> {
    if bytes.len() < 20 {
        return Err("not an ELF binary (shorter than the 20-byte header prefix)".to_string());
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err("not an ELF binary (bad magic)".to_string());
    }
    match bytes[5] {
        1 => Ok(u16::from_le_bytes([bytes[18], bytes[19]])),
        2 => Ok(u16::from_be_bytes([bytes[18], bytes[19]])),
        other => Err(format!("unknown ELF data encoding {other}")),
    }
}

/// Hash the binary the server will ship and read its ELF machine.
/// Errors name `--remote-portuale-binary` when that option selected
/// the file.
pub(crate) fn server_bin_info(ctx: &RemoteContext) -> Result<ServerBinInfo, String> {
    let (path, named) = match &ctx.portuale_binary {
        Some(custom) => (std::path::PathBuf::from(custom), true),
        None => (crate::self_exe::self_exe().to_path_buf(), false),
    };
    let bytes = std::fs::read(&path).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!(
                "mrg: cannot read the running binary {} for the client install: {e}",
                path.display()
            )
        }
    })?;
    use sha2::Digest as _;
    let digest_hex = format!("{:x}", sha2::Sha256::digest(&bytes));
    let machine = elf_machine(&bytes).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!("mrg: the running binary {}: {e}", path.display())
        }
    })?;
    Ok(ServerBinInfo {
        short_hash: digest_hex[..16].to_string(),
        digest_hex,
        machine,
        path,
    })
}

/// Server-side preflight facts for the binary gates: what to look for
/// and what to compare against. `None` for local transport without
/// `--remote-portuale-dir`, which keeps `current_exe()` and installs
/// nothing.
pub(crate) struct BinPreflight {
    /// `bin_search_dirs(ctx)`.
    pub dirs: Vec<String>,
    /// `bin_candidates(&dirs, &short)`, in D5 order.
    pub candidates: Vec<String>,
    /// Full expected SHA-256 hex.
    pub digest: String,
    /// The file the digest (and bytes) come from, for messages.
    pub bin_path: String,
    /// ELF `e_machine` of that file.
    pub machine: u16,
}

/// Compute the binary preflight facts, unless this run skips the gates
/// by design (local transport without the override).
pub(crate) fn bin_preflight(ctx: &RemoteContext) -> Result<Option<BinPreflight>, String> {
    if ctx.transport == RemoteTransport::Local && ctx.portuale_dir.is_none() {
        return Ok(None);
    }
    let info = server_bin_info(ctx)?;
    let dirs = bin_search_dirs(ctx);
    let candidates = bin_candidates(&dirs, &info.short_hash);
    Ok(Some(BinPreflight {
        dirs,
        candidates,
        digest: info.digest_hex,
        bin_path: info.path.display().to_string(),
        machine: info.machine,
    }))
}

/// Run the install stage when the plan says so: `Use`/`Local` are
/// no-ops (no status line -- a reuse prints nothing); `Unresolved`
/// is an internal error (preflight never ran).
pub(crate) fn ensure_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<(), String> {
    match &ctx.bin_plan {
        ClientBinPlan::Install(target) => {
            let pre = bin_preflight(ctx)?.ok_or_else(|| {
                "mrg: internal error: the client binary plan needs an install but preflight skipped the binary gates".to_string()
            })?;
            install_client_binary(ctx, control, target, &pre.digest, &pre.bin_path)
        }
        ClientBinPlan::Use(_) | ClientBinPlan::Local(_) => Ok(()),
        ClientBinPlan::Unresolved => Err(
            "mrg: internal error: the client binary was never resolved (preflight did not run)"
                .to_string(),
        ),
    }
}

/// Digest of a client path, if it exists: `None` when missing. Only a
/// transport failure is an `Err`; a missing file is not one.
pub(crate) fn client_file_digest(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    path: &str,
) -> Result<Option<String>, String> {
    match ctx.transport {
        RemoteTransport::Local => {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(format!("mrg: reading {path}: {e}")),
            };
            use sha2::Digest as _;
            Ok(Some(format!("{:x}", sha2::Sha256::digest(&bytes))))
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &["sha256sum".to_string(), "--".to_string(), path.to_string()],
            )?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                if is_transport_error(code, &stderr) {
                    return Err(format!(
                        "mrg: client {} unreachable:\n{stderr}",
                        ctx.hostname
                    ));
                }
                return Ok(None);
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout
                .split_whitespace()
                .next()
                .map(String::from)
                .filter(|hex| hex.len() == 64))
        }
    }
}

/// Best-effort removal of a client temp path (the install's failure
/// cleanup).
pub(crate) fn remove_client_path(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    path: &str,
) {
    match ctx.transport {
        RemoteTransport::Local => {
            let _ = std::fs::remove_file(path);
        }
        RemoteTransport::Ssh => {
            let _ = run_raw_command(
                ctx,
                control,
                &[
                    "rm".to_string(),
                    "-f".to_string(),
                    "--".to_string(),
                    path.to_string(),
                ],
            );
        }
    }
}

/// Run `"<abs>" __helper ping` on the client and require the D8 token on
/// stdout: the installed binary really executes there.
pub(crate) fn ping_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    abs: &str,
) -> Result<(), String> {
    let output = run_raw_command(
        ctx,
        control,
        &[abs.to_string(), "__helper".to_string(), "ping".to_string()],
    )
    .map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local client binary ping failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if ctx.transport == RemoteTransport::Ssh && is_transport_error(code, &stderr) {
            return Err(format!(
                "mrg: client {} unreachable:\n{stderr}",
                ctx.hostname
            ));
        }
        return Err(format!(
            "mrg: client binary {abs} failed its __helper ping (exit {}):\n{stderr}",
            code.unwrap_or(-1)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != crate::helpers::PING_TOKEN {
        return Err(format!(
            "mrg: client binary {abs} did not answer the __helper ping (no D8 token)"
        ));
    }
    Ok(())
}

/// The install: `mktemp <dir>/.portuale.XXXXXX`, stream the server
/// binary over the existing connection (the way bundles stream), `chmod
/// 0755`, re-check the digest, `mv` to `<dir>/portuale-<hash>`, then
/// `"<abs>" __helper ping` (which must print the ping token). On any
/// failure the temp file is removed and the run fails before any unit
/// is consumed. Prints the status line `portuale-remote: install-bin
/// <rc>`.
pub(crate) fn install_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    target: &str,
    expected: &str,
    bin_path: &str,
) -> Result<(), String> {
    // Idempotent: a matching target only needs its ping (a reuse, so no
    // status line -- the second run reports no `install-bin` line).
    match client_file_digest(ctx, control, target)? {
        Some(digest) if digest == expected => {
            ping_client_binary(ctx, control, target)?;
            return Ok(());
        }
        _ => {}
    }
    let bytes = std::fs::read(bin_path)
        .map_err(|e| format!("mrg: reading the shipped binary {bin_path}: {e}"))?;
    let dir = std::path::Path::new(target)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| format!("mrg: install target {target} has no parent directory"))?;
    // `mktemp <dir>/.portuale.XXXXXX`.
    let tmp: String = match ctx.transport {
        RemoteTransport::Local => {
            let mut attempt = 0;
            loop {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|t| t.as_nanos())
                    .unwrap_or(0);
                let candidate = format!(
                    "{dir}/.portuale.{}.{attempt}.{nanos}.tmp",
                    std::process::id()
                );
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(_) => break candidate,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => {
                        attempt += 1;
                    }
                    Err(e) => {
                        return Err(format!("mrg: creating the client temp file: {e}"));
                    }
                }
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &["mktemp".to_string(), format!("{dir}/.portuale.XXXXXX")],
            )?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                if is_transport_error(code, &stderr) {
                    return Err(format!(
                        "mrg: client {} unreachable:\n{stderr}",
                        ctx.hostname
                    ));
                }
                return Err(format!(
                    "mrg: client mktemp in {dir} failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if name.is_empty() || !name.starts_with(&dir) {
                remove_client_path(ctx, control, &name);
                return Err(format!(
                    "mrg: client mktemp answered unexpectedly: {name:?}"
                ));
            }
            name
        }
    };
    let fail = |step: &str, detail: String| {
        remove_client_path(ctx, control, &tmp);
        format!("mrg: client binary install failed at {step}: {detail}")
    };
    // Write from stdin (streamed over the existing connection).
    if let Err(message) = send_bytes(ctx, control, &bytes, &tmp) {
        return Err(fail("stream", message));
    }
    match ctx.transport {
        RemoteTransport::Local => {
            use std::os::unix::fs::PermissionsExt as _;
            if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)) {
                return Err(fail("chmod", e.to_string()));
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &[
                    "chmod".to_string(),
                    "0755".to_string(),
                    "--".to_string(),
                    tmp.clone(),
                ],
            )
            .map_err(|message| fail("chmod", message))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(fail(
                    "chmod",
                    format!("exit {}:\n{stderr}", output.status.code().unwrap_or(-1)),
                ));
            }
        }
    }
    // Re-check the digest before the rename (never an in-place write).
    match client_file_digest(ctx, control, &tmp)? {
        Some(digest) if digest == expected => {}
        other => {
            return Err(fail(
                "digest",
                format!("the streamed bytes do not match (got {other:?})"),
            ));
        }
    }
    match ctx.transport {
        RemoteTransport::Local => {
            if let Err(e) = std::fs::rename(&tmp, target) {
                return Err(fail("mv", e.to_string()));
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &[
                    "mv".to_string(),
                    "--".to_string(),
                    tmp.clone(),
                    target.to_string(),
                ],
            )
            .map_err(|message| fail("mv", message))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(fail(
                    "mv",
                    format!("exit {}:\n{stderr}", output.status.code().unwrap_or(-1)),
                ));
            }
        }
    }
    // The install must execute: `"<abs>" __helper ping` must print the ping token.
    if let Err(message) = ping_client_binary(ctx, control, target) {
        remove_client_path(ctx, control, target);
        return Err(message);
    }
    println!("portuale-remote: install-bin 0");
    Ok(())
}
