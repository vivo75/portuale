//! The merge, regen, old-hook and phase stages.

use crate::remote::*;

/// Client vdb destination for this run: the placed path, plus whether
/// the driver runs stateless (server-side vdb placement -- the driver
/// merges files only: no vdb entry, no old hooks, fail-closed
/// collisions). A stateless merge has no vdb entry to attach the regen'd
/// env to, so the install is skipped while postinst still runs.
pub(crate) fn client_vdb_placement(ctx: &RemoteContext) -> (String, bool) {
    match &ctx.vdb {
        ConfigPlacement::Client(path) => (path.clone(), false),
        ConfigPlacement::Server(_) => (
            format!("{}/var/db/pkg", ctx.root.trim_end_matches('/')),
            true,
        ),
    }
}

/// Human tail of a merge failure: the `MERGE_FAIL` line plus a few log
/// lines, prefixed for the report.
pub(crate) fn merge_failure_tail(stdout: &str, stderr: &str, code: Option<i32>) -> String {
    let mut message = format!("mrg: client merge failed (exit {}):", code.unwrap_or(-1));
    let mut shown = 0;
    for line in stdout
        .lines()
        .filter(|l| l.starts_with("MERGE_FAIL=") || l.starts_with("MERGE_WARN="))
        .chain(stderr.lines())
        .take(6)
    {
        message.push_str(&format!("\nmrg:   {line}"));
        shown += 1;
        if shown >= 6 {
            break;
        }
    }
    message
}

/// Run the merge driver; `Ok(markers)` are the `MERGE_<STEP>=ok` lines
/// for the report, `Err(message)` the failure tail. The success gate is
/// the driver's own `STATUS=merged` trailer, with exit 0 +
/// `MERGE_DONE=ok` kept as the backstop.
pub(crate) fn run_merge_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    ledger: Option<&LedgerSpec>,
) -> Result<Vec<String>, String> {
    let (vdb, stateless) = client_vdb_placement(ctx);
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let script = merge_script(MergeScriptArgs {
        unit_dir,
        staged,
        root: &ctx.root,
        vdb: &vdb,
        stateless,
        protect_list: &ctx.config_protect,
        mask_list: &ctx.config_protect_mask,
        ledger,
        portuale_bin: &portuale_bin,
    })?;
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local merge command failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stderr.lines().chain(stdout.lines()) {
        println!("{line}");
    }
    let values = parse_kv(&stdout);
    if output.status.success()
        && values.get("MERGE_DONE").map(String::as_str) == Some("ok")
        && values.get("STATUS").map(String::as_str) == Some("merged")
    {
        let mut markers = Vec::new();
        for step in [
            "MERGE_PRERM",
            "MERGE_COPY",
            "MERGE_VDB",
            "MERGE_REMOVE",
            "MERGE_POSTRM",
            "MERGE_ENVUPDATE",
        ] {
            if let Some(value) = values.get(step) {
                markers.push(format!("{step}={value}"));
            }
        }
        Ok(markers)
    } else {
        Err(merge_failure_tail(&stdout, &stderr, code))
    }
}

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

/// Server-side compression for the pulled regen'd env: `bzip2 -9`, the
/// same level Portage's `PORTAGE_UPDATE_ENV` block uses.
pub(crate) fn bzip2_compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-c", "-f9"], bytes)
}

/// Server-side decompression for an old instance's `environment.bz2`.
pub(crate) fn bzip2_decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-d", "-c", "--"], bytes)
}

/// Rewrite the `${PORTAGE_BZIP2_COMMAND}` line in a pulled regen'd
/// environment **only** when its value is the unit-local
/// `bzip2-passthrough` stand-in, replacing it with the resolved client
/// command. Any other value is copied verbatim; a missing line stays
/// missing (never invent it).
pub(crate) fn scrub_bzip2_command(bytes: &[u8], bzip2_command: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let is_passthrough = match line.iter().position(|b| *b == b'=') {
            Some(eq) => {
                let mut name = &line[..eq];
                for prefix in ["declare -x ".as_bytes(), "declare -- ".as_bytes()] {
                    if let Some(rest) = name.strip_prefix(prefix) {
                        name = rest;
                        break;
                    }
                }
                name == b"PORTAGE_BZIP2_COMMAND"
                    && line
                        .windows(b"bzip2-passthrough".len())
                        .any(|w| w == b"bzip2-passthrough")
            }
            None => false,
        };
        if is_passthrough {
            let escaped = bzip2_command.replace('\\', "\\\\").replace('"', "\\\"");
            out.extend_from_slice(
                format!("declare -x PORTAGE_BZIP2_COMMAND=\"{escaped}\"\n").as_bytes(),
            );
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

/// The failure line: names the package and the failing step (`postinst`,
/// `pull`, `compress`, `install`); the vdb keeps the build-time
/// `environment.bz2` and the merge continues.
pub(crate) fn regen_warn_message(cpv: &str, step: &str) -> String {
    format!(
        "!!! Remote {cpv}: merge-time environment regen failed at {step}, keeping build-time environment.bz2"
    )
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

/// The old-hook staging warning: names the package and the failing
/// detail; old hooks for that pf fall back to the client vdb.
pub(crate) fn old_hook_warn_message(cpv: &str, detail: &str) -> String {
    format!(
        "!!! Remote {cpv}: could not stage the saved env for an old instance ({detail}); old hooks fall back to the client vdb"
    )
}

/// The resolved absolute client binary for generated scripts:
/// every script exports this as `PORTUALE_BIN`.
pub(crate) fn client_bin_abs(ctx: &RemoteContext) -> Result<&str, String> {
    ctx.bin_plan.abs().ok_or_else(|| {
        "mrg: internal error: the client binary was never resolved (preflight did not run)"
            .to_string()
    })
}
