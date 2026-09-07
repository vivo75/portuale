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
