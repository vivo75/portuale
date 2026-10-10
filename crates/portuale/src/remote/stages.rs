//! The merge, regen, old-hook and phase stages.

use crate::remote::*;

/// Client vdb destination for this run: the placed path, plus whether
/// the driver runs stateless (server-side vdb placement, plan §7 -- the
/// driver merges files only: no vdb entry, no old hooks, fail-closed
/// collisions). Shared by the merge driver and the #171 env-regen
/// install (a stateless merge has no vdb entry to attach the regen'd
/// env to, so the install is skipped while postinst still runs).
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
/// the driver's own `STATUS=merged` trailer (plan §11), with exit 0 +
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

// --- Merge-time vdb environment regeneration (backlog #171) -------------------
//
// Real regenerates the vdb env from the live postinst environment of
// the merge (`vartree.py:5334` `PORTAGE_UPDATE_ENV=<dbpkgdir>/
// environment.bz2`; `bin/phase-functions.sh:1072-1082`), while `mrg`
// kept the binpkg's build-time env. The client regenerates
// uncompressed (no `bzip2` on the client, plan §6/§14.1) and the
// server compresses + installs; old-instance hooks get their env
// server-decompressed the same way.

/// Outcome of the regen postinst run: the phase rc (non-zero is
/// non-fatal, real `_postinst_failure`) plus whether the client left a
/// fresh `$UNIT/environment.regen` behind.
pub(crate) struct RegenReport {
    pub(crate) phase_rc: i32,
    pub(crate) regen_present: bool,
    pub(crate) skipped_no_hooks: bool,
}

/// Arguments of [`run_postinst_regen_stage`].
pub(crate) struct RunPostinstRegenStageArgs<'a> {
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) control: Option<&'a std::path::Path>,
    pub(crate) unit_dir: &'a str,
    pub(crate) staged: &'a crate::remote_bundle::StagedBundle,
    pub(crate) features: Option<&'a str>,
    pub(crate) locale: &'a [(String, String)],
}

/// Run the regen postinst script and print its log straight through.
/// `Err` is a transport/command failure only -- a non-zero phase rc or
/// a missing regen file is a warn-and-continue `RegenReport`, never a
/// unit failure (real `_postinst_failure`). `locale` is the server's
/// own locale pairs for the client phase env (see
/// `collect_server_locale`); the unit flow collects them from the
/// process environment, tests inject them explicitly.
pub(crate) fn run_postinst_regen_stage(
    args: RunPostinstRegenStageArgs<'_>,
) -> Result<RegenReport, String> {
    let RunPostinstRegenStageArgs {
        ctx,
        control,
        unit_dir,
        staged,
        features,
        locale,
    } = args;
    let colormap = crate::color::phase_colormap_export();
    let workdir_parent = std::path::Path::new(&ctx.workdir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/tmp".to_string());
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let script = postinst_regen_script(PostinstRegenScriptArgs {
        unit_dir,
        staged,
        root: &ctx.root,
        workdir_parent: &workdir_parent,
        colormap: &colormap,
        features,
        locale,
        portuale_bin: &portuale_bin,
    });
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local postinst regen command failed: {message}")
        } else {
            message
        }
    })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stderr.lines().chain(stdout.lines()) {
        println!("{line}");
    }
    let values = parse_kv(&stdout);
    let phase_rc: i32 = values
        .get("PHASE_postinst")
        .and_then(|rc| rc.parse().ok())
        .unwrap_or(-1);
    let regen = values
        .get("REGEN_ENV")
        .map(String::as_str)
        .unwrap_or("missing");
    Ok(RegenReport {
        phase_rc,
        regen_present: regen == "ok",
        skipped_no_hooks: regen == "skip:no-hooks",
    })
}

/// Pull one remote file's bytes to the server (`cat` remotely; a plain
/// read for the `local` transport, where client paths are server
/// paths). `Err` is the pull failure (the #171 regen warn path).
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
/// same level real's `PORTAGE_UPDATE_ENV` block uses (`-f9`).
pub(crate) fn bzip2_compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-c", "-f9"], bytes)
}

/// Server-side decompression for an old instance's `environment.bz2`.
pub(crate) fn bzip2_decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-d", "-c", "--"], bytes)
}

/// Rewrite the `${PORTAGE_BZIP2_COMMAND}` line in a pulled regen'd
/// environment **only** when its value is the unit-local
/// `bzip2-passthrough` stand-in the regen postinst run used, replacing
/// it with the resolved client command (`bzip2_command` -- the
/// server-resolved client `PORTAGE_BZIP2_COMMAND`, `make.globals`'
/// `bzip2` when nothing is configured). Real's merge-time save records
/// its live value (verified on this host's own
/// `/var/db/pkg/*/environment.bz2`, which all carry
/// `declare -x PORTAGE_BZIP2_COMMAND="bzip2"`), while ours ran with a
/// per-unit workdir path that must not leak into the vdb (and would
/// keep the L1 `environment` rows red: `normalize.py` compares the
/// content verbatim). Any other value -- real's own spelling, a
/// foreign path, anything -- is copied verbatim; a missing line stays
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

/// Install `bytes` at `dest` on the client atomically (temp name +
/// `mv`, so a concurrent unmerge never reads a half-written env). A
/// failed `mv` best-effort removes the remote temp name (review #4 --
/// no client tmp litter).
pub(crate) fn install_file_atomic(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    bytes: &[u8],
    dest: &str,
) -> Result<(), String> {
    let tmp = format!("{dest}.portuale-regen-tmp");
    match ctx.transport {
        RemoteTransport::Local => {
            if let Some(parent) = std::path::Path::new(&tmp).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&tmp, bytes).map_err(|e| format!("mrg: writing {tmp} failed: {e}"))?;
            std::fs::rename(&tmp, dest).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("mrg: installing {dest} failed: {e}")
            })?;
            Ok(())
        }
        RemoteTransport::Ssh => {
            // Backlog #262: `TempDir::new` creates a directory (backlog
            // #260), so the bytes must ride a file *inside* it --
            // staging straight into its path fails every ssh install
            // with `Is a directory`, and the vdb keeps the build-time
            // env (with its stray `declare -- x=""`) instead of the
            // regen'd one.
            let server_dir = TempDir::new("portuale-regen-install").keep();
            let server_tmp = server_dir.join("regen.env");
            std::fs::write(&server_tmp, bytes)
                .map_err(|e| format!("mrg: staging the regen install failed: {e}"))?;
            let result = send_file(ctx, control, &server_tmp, &tmp).and_then(|()| {
                let output = run_raw_command(
                    ctx,
                    control,
                    &["mv".to_string(), tmp.clone(), dest.to_string()],
                )?;
                if output.status.success() {
                    Ok(())
                } else {
                    // Best-effort: don't litter the client tmp name.
                    let _ = run_raw_command(
                        ctx,
                        control,
                        &["rm".to_string(), "-f".to_string(), tmp.clone()],
                    );
                    Err(format!(
                        "mrg: moving {tmp} into place failed (exit {})",
                        output.status.code().unwrap_or(-1)
                    ))
                }
            });
            let _ = std::fs::remove_dir_all(&server_dir);
            result
        }
    }
}

/// Pull the regen'd env, scrub it, compress it server-side and install
/// it over the vdb's build-time `environment.bz2`. `Err` is
/// `(failing step, detail)` -- `pull`, `compress` or `install` -- for
/// the caller's one-`!!!` warn-and-continue line. `regen_bzip2` is the
/// resolved client `PORTAGE_BZIP2_COMMAND` for the scrub (`None` on
/// paths that resolve no config -- the `make.globals` default).
pub(crate) fn install_regenerated_env(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    vdb_env_bz2: &str,
    regen_bzip2: Option<&str>,
) -> Result<(), (String, String)> {
    let regen = format!("{unit_dir}/environment.regen");
    let bytes = pull_file(ctx, control, &regen).map_err(|message| ("pull".to_string(), message))?;
    let scrubbed = scrub_bzip2_command(&bytes, regen_bzip2.unwrap_or("bzip2"));
    let compressed =
        bzip2_compress(&scrubbed).map_err(|message| ("compress".to_string(), message))?;
    install_file_atomic(ctx, control, &compressed, vdb_env_bz2)
        .map_err(|message| ("install".to_string(), message))?;
    Ok(())
}

/// The #171 failure line (coordinator decision): names the package and
/// the failing step (`postinst`, `pull`, `compress`, `install`); the
/// vdb keeps the build-time `environment.bz2` and the merge continues.
pub(crate) fn regen_warn_message(cpv: &str, step: &str) -> String {
    format!(
        "!!! Remote {cpv}: merge-time environment regen failed at {step}, keeping build-time environment.bz2"
    )
}

/// A staged old-hook environment: which replaced-version pfs got a
/// server-decompressed env under `$UNIT/old-env/`, plus per-pf failure
/// details for the report (the driver falls back to the client vdb for
/// those -- the legacy plain file, then client `bzip2`).
pub(crate) struct OldHookShipment {
    pub(crate) shipped: Vec<String>,
    pub(crate) warnings: Vec<String>,
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

/// Arguments of [`ship_old_hook_envs`].
pub(crate) struct ShipOldHookEnvsArgs<'a> {
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) control: Option<&'a std::path::Path>,
    pub(crate) unit_dir: &'a str,
    pub(crate) vdb: &'a str,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) new_mainslot: &'a str,
}

/// Stage every same-slot installed version's saved env for the merge
/// driver's `run_old_hook` (backlog #171): probe the client vdb for
/// same-package versions, pull each `environment.bz2`, decompress it
/// **server-side**, and ship the plain text to `$UNIT/old-env/<pf>`.
/// A version with no bz2 (older `mrg` entries carry only a plain
/// `environment`) ships that file as-is; a version with neither
/// warns with both pull errors (and still hits the driver's own
/// warn-and-skip). Best-effort throughout:
/// every failure lands in `warnings` and the merge continues.
pub(crate) fn ship_old_hook_envs(args: ShipOldHookEnvsArgs<'_>) -> OldHookShipment {
    let ShipOldHookEnvsArgs {
        ctx,
        control,
        unit_dir,
        vdb,
        category,
        pn,
        new_pf,
        new_mainslot,
    } = args;
    let mut shipment = OldHookShipment {
        shipped: Vec::new(),
        warnings: Vec::new(),
    };
    let vdbroot = format!("{vdb}/{category}");
    // Same discovery shape as the driver's own replace loop
    // (`$PKG-<digit...>` names, SLOT first field); the driver still
    // decides, this only stages envs.
    let script = format!(
        concat!(
            "UNIT={unit}\n",
            "VDBROOT={vdbroot}\n",
            "PKG={pkg}\n",
            "mkdir -p \"$UNIT/old-env\" 2>/dev/null || true\n",
            "for d in \"$VDBROOT/\"$PKG-*/; do\n",
            "  [ -d \"$d\" ] || continue\n",
            "  cpf=${{d%/}}; cpf=${{cpf##*/}}\n",
            "  rest=${{cpf#\"$PKG-\"}}\n",
            "  case \"$rest\" in [0-9]*) ;; *) continue;; esac\n",
            "  [ \"$cpf\" = {newpf} ] && continue\n",
            "  slot=$(cat \"$d/SLOT\" 2>/dev/null | cut -d/ -f1)\n",
            "  echo \"OLDPF=$cpf SLOT=${{slot:-unknown}}\"\n",
            "done\n",
        ),
        unit = sh_quote(unit_dir),
        vdbroot = sh_quote(&vdbroot),
        pkg = sh_quote(pn),
        newpf = sh_quote(new_pf),
    );
    let output = match run_script_stdin(ctx, control, &script) {
        Ok(output) => output,
        Err(message) => {
            shipment.warnings.push(format!("probe: {message}"));
            return shipment;
        }
    };
    if !output.status.success() {
        shipment.warnings.push(format!(
            "probe exited {}",
            output.status.code().unwrap_or(-1)
        ));
        return shipment;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    for (pf, slot) in parse_oldpf_probe(&stdout) {
        if pf == new_pf {
            continue;
        }
        // Unknown slots ship anyway (fail-open toward staging; the
        // driver decides what to replace).
        if slot != "unknown" && slot != new_mainslot {
            continue;
        }
        let old_bz2 = format!("{vdbroot}/{pf}/environment.bz2");
        match pull_file(ctx, control, &old_bz2) {
            Ok(compressed) => match bzip2_decompress(&compressed) {
                Ok(plain) => {
                    let dest = format!("{unit_dir}/old-env/{pf}");
                    match send_bytes(ctx, control, &plain, &dest) {
                        Ok(()) => shipment.shipped.push(pf),
                        Err(message) => shipment.warnings.push(format!("{pf}: send: {message}")),
                    }
                }
                Err(message) => shipment
                    .warnings
                    .push(format!("{pf}: decompress: {message}")),
            },
            Err(bz2_message) => {
                // No bz2 (older `mrg` entries carry only a plain file,
                // or nothing at all): ship the plain text as-is. When
                // the plain pull fails too, the original bz2 error goes
                // into `warnings` (review #3 -- silent staging failures
                //); a version with neither file warns here and hits the
                // driver's own warn-and-skip as well.
                let old_plain = format!("{vdbroot}/{pf}/environment");
                match pull_file(ctx, control, &old_plain) {
                    Ok(plain) => {
                        let dest = format!("{unit_dir}/old-env/{pf}");
                        if let Err(message) = send_bytes(ctx, control, &plain, &dest) {
                            shipment.warnings.push(format!("{pf}: send: {message}"));
                        } else {
                            shipment.shipped.push(pf);
                        }
                    }
                    Err(plain_message) => shipment.warnings.push(format!(
                        "{pf}: env pull: {bz2_message}; plain fallback: {plain_message}"
                    )),
                }
            }
        }
    }
    shipment
}

/// The S8.1-resolved absolute client binary for generated scripts
/// (#326 D5/S8.3): every script exports this as `PORTUALE_BIN`.
pub(crate) fn client_bin_abs(ctx: &RemoteContext) -> Result<&str, String> {
    ctx.bin_plan.abs().ok_or_else(|| {
        "mrg: internal error: the client binary was never resolved (preflight did not run)"
            .to_string()
    })
}

/// Run the staged phases in order, stopping at the first non-zero
/// (pretend/setup/preinst are all fatal -- local rule kept). Returns the
/// per-phase `ok` markers for the report line.
pub(crate) fn run_phases_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
) -> Result<Vec<String>, String> {
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let colormap = crate::color::phase_colormap_export();
    let workdir_parent = std::path::Path::new(&ctx.workdir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/tmp".to_string());
    let mut done = Vec::new();
    for phase in &staged.phases {
        let script = phase_script(PhaseScriptArgs {
            unit_dir,
            staged,
            phase,
            root: &ctx.root,
            workdir_parent: &workdir_parent,
            colormap: &colormap,
            portuale_bin: &portuale_bin,
        });
        let output = run_script_stdin(ctx, control, &script).map_err(|message| {
            if ctx.transport == RemoteTransport::Local {
                format!("mrg: local phase {phase} command failed: {message}")
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
        let reported: i32 = parse_kv(&stdout)
            .get(&format!("PHASE_{phase}"))
            .and_then(|rc| rc.parse().ok())
            .unwrap_or(-1);
        if output.status.success() && reported == 0 {
            done.push(format!("{phase} ok"));
        } else {
            let mut message = format!(
                "mrg: client phase {phase} failed (exit {}, PHASE_{phase}={reported}):",
                code.unwrap_or(-1)
            );
            for line in stderr.lines().chain(stdout.lines()).take(5) {
                message.push_str(&format!("\nmrg:   {line}"));
            }
            return Err(message);
        }
    }
    Ok(done)
}
