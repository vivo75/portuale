//! The merge, regen, old-hook and phase stages.

use crate::remote::*;

/// Pull one remote file's bytes to the server (`cat` remotely; a plain
/// read for the `local` transport, where client paths are server
/// paths). `Err` is the pull failure.
pub(crate) fn pull_file(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    match ctx.transport {
        RemoteTransport::Local => std::fs::read(remote_path)
            .map_err(|e| format!("mrg: pulling {remote_path} from the client failed: {e}")),
        RemoteTransport::Ssh => {
            let output =
                run_raw_command(ctx, control, &["cat".to_string(), remote_path.to_string()])?;
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
                    "mrg: pulling {remote_path} from the client failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            Ok(output.stdout)
        }
    }
}

/// Server-side `bzip2` over piped bytes (the server has bzip2; the
/// client must not need it). The local merge shells the same binary
/// (`seed_saved_environment`, `remote_bundle::build_bundle`) -- there
/// is no Rust bzip2 crate in the tree, so this pipes through the
/// system one instead of adding a dependency.
pub(crate) fn bzip2_pipe(args: &[&str], bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("bzip2")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("mrg: cannot spawn server bzip2: {e}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: bzip2 stdin unavailable".to_string())?
        .write_all(bytes)
        .map_err(|e| format!("mrg: feeding server bzip2: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for server bzip2: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "mrg: server bzip2 {} failed (exit {}): {}",
            args.join(" "),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// `OLDPF=<pf> SLOT=<slot>` lines out of the old-version probe
/// (anything else ignored -- the driver owns discovery, this only
/// stages envs).
pub(crate) fn parse_oldpf_probe(stdout: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let rest = match line.trim().strip_prefix("OLDPF=") {
            Some(rest) => rest,
            None => continue,
        };
        let (pf, slot) = match rest.split_once(" SLOT=") {
            Some((pf, slot)) => (pf, slot),
            None => continue,
        };
        if pf.is_empty() {
            continue;
        }
        out.push((pf.to_string(), slot.to_string()));
    }
    out
}

/// The resolved absolute client binary for generated scripts:
/// every script exports this as `PORTUALE_BIN`.
pub(crate) fn client_bin_abs(ctx: &RemoteContext) -> Result<&str, String> {
    ctx.bin_plan.abs().ok_or_else(|| {
        "mrg: internal error: the client binary was never resolved (preflight did not run)"
            .to_string()
    })
}
