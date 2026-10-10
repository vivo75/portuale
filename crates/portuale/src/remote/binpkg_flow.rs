//! Checking the binary plan and running the binpkg flow.

use crate::remote::*;

// --- Vdb shadow + pre-ship collision pre-check (slice 6) ----------------------
///
/// The server reasons about installed-file ownership *before* shipping a
/// unit's tarball (plan §7): a unit doomed by a foreign-owned collision
/// fails here, with zero client writes. The shadow is the placed vdb
/// read directly (`server:`) or pulled once at plan start (`client:`,
/// same `tar -c`/`tar -x` shape as the etc-portage pull).
///
/// Deliberately lenient: only ownership by a *different* package fails
/// the pre-check. Same-package ownership (any version/slot) passes --
/// the client driver refines those (same-slot replace vs. collision)
/// against the live vdb, or fail-closed without one. A passed pre-check
/// is therefore necessary but not sufficient; a failed one is final.
///
// Installed-file ownership: absolute path -> owning `(category, pf)`.
pub(crate) struct VdbShadow {
    pub(crate) owners: std::collections::HashMap<String, (String, String)>,
}

/// `obj`/`sym` paths out of one vdb `CONTENTS` file (real
/// `dblink`/`vartree` shape: `obj <path> <md5> <mtime>`,
/// `sym <path> -> <target> <mtime>`). `dir` lines merge freely
/// client-side (`mkdir -p`), so they never gate a pre-check.
pub(crate) fn contents_owned_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("obj"), Some(path)) | (Some("sym"), Some(path)) => Some(path.to_string()),
                _ => None,
            }
        })
        .collect()
}

impl VdbShadow {
    /// Read `<dir>/<cat>/<pf>/CONTENTS` (two-level walk, no new
    /// dependencies -- the vdb is exactly that deep). Missing/unreadable
    /// files are skipped, not fatal: an empty shadow pre-checks nothing,
    /// and the client driver still gates every collision for real.
    pub(crate) fn load(dir: &std::path::Path) -> Self {
        Self::from_db(&portage_vdb::FilesDb::open_vdb_dir(dir))
    }

    /// The shadow of any installed-package database: entries in listing
    /// order (in-progress `-MERGING-<pf>` entries are never listed), the
    /// first owner of a path wins. An unlistable database or an
    /// unreadable / non-UTF-8 `CONTENTS` contributes nothing.
    pub(crate) fn from_db(db: &dyn portage_vdb::InstalledDb) -> Self {
        let mut owners = std::collections::HashMap::new();
        let Ok(keys) = db.entries() else {
            return Self { owners };
        };
        for key in keys {
            let Ok(Some(bytes)) = db.read_file(&key, "CONTENTS") else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for path in contents_owned_paths(&text) {
                owners
                    .entry(path)
                    .or_insert_with(|| (key.category.clone(), key.pf.clone()));
            }
        }
        Self { owners }
    }

    /// Owner `(category, pf)` of an absolute image path, if recorded.
    pub(crate) fn owner(&self, abspath: &str) -> Option<&(String, String)> {
        self.owners.get(abspath)
    }

    /// True when `pf` belongs to package `pn` (real `_pkgsplit`
    /// longest-trailing-version rule via `portage_repo::split_pf`).
    pub(crate) fn same_package(pf: &str, pn: &str) -> bool {
        portage_repo::split_pf(pf).is_some_and(|(name, _)| name == pn)
    }
}

/// `(kind, rel)` image paths out of a staged `filemeta` file (the exact
/// bytes the client will gate on: `<kind> <md5> <mtime> <rel>
/// [<target>]`, whitespace-free by bundle-build construction).
pub(crate) fn parse_filemeta_paths(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let kind = words.next()?.to_string();
            if !matches!(kind.as_str(), "obj" | "sym" | "dir") {
                return None;
            }
            words.next()?;
            words.next()?;
            let rel = words.next()?.to_string();
            Some((kind, rel))
        })
        .collect()
}

/// Pre-ship gate for one unit: every `obj`/`sym` image path owned in the
/// shadow by a *different* package fails with a `not shipping` error
/// (zero client writes so far). Same-package ownership passes for the
/// client driver to refine.
pub(crate) fn shadow_precheck(
    shadow: &VdbShadow,
    cpv: &str,
    category: &str,
    pn: &str,
    staged_filemeta: &str,
) -> Result<(), String> {
    for (kind, rel) in parse_filemeta_paths(staged_filemeta) {
        if kind == "dir" {
            continue;
        }
        let abspath = format!("/{rel}");
        if let Some((cat, pf)) = shadow.owner(&abspath)
            && (cat != category || !VdbShadow::same_package(pf, pn))
        {
            return Err(format!(
                "mrg: not shipping {cpv}: {abspath} owned by {cat}/{pf} (vdb shadow)"
            ));
        }
    }
    Ok(())
}

/// Load the shadow for this run: read directly for `server:` placement,
/// pull once for `client:` placement (plan §7's pull-once-per-run).
pub(crate) fn load_vdb_shadow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<VdbShadow, String> {
    match &ctx.vdb {
        ConfigPlacement::Server(path) => Ok(VdbShadow::load(std::path::Path::new(path))),
        ConfigPlacement::Client(path) => {
            let tmp = TempDir::new("portuale-remote-vdb").keep();
            // Like the etc-portage pull, the tmp dir is left for
            // forensics; it carries no secrets beyond file lists.
            pull_dir(ctx, control, path, &tmp)?;
            Ok(VdbShadow::load(&tmp))
        }
    }
}

/// Every merge-bound entry must be `Binary` in a remote plan (the
/// `--getbinpkgonly` resolve guarantees it); anything else is a usage
/// error naming the offenders (exit 2 at the call site).
pub(crate) fn check_binary_plan(entries: &[portage_repo::GraphEntry]) -> Result<(), String> {
    use portage_repo::{CandidateSource, PretendOutcome};
    let mut offenders = Vec::new();
    for entry in entries {
        let merge_bound = !matches!(
            entry.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        );
        if merge_bound && entry.source != CandidateSource::Binary {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version.clone()
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    to.clone()
                }
                _ => String::new(),
            };
            offenders.push(format!("{}/{}-{version}", entry.category, entry.package));
        }
    }
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "remote plan is not binary-only (needs --getbinpkgonly): {}",
            offenders.join(" ")
        ))
    }
}

/// One binpkg end to end (bundle, stream, unpack, phases, merge,
/// postinst): shared by the `--remote-binpkg` path (no ledger, no
/// shadow) and the resolve path (ledger per merged entry, shadow
/// pre-check before anything ships). `install_mask` /
/// `install_mask_prunes_usr_share` are the resolve's own
/// `config_install_mask` values for the placed config -- both paths
/// resolve the client config now (backlog #170/#171b: the trial path's
/// `run_bundle_stage` places and loads it like `run_remote_resolve`
/// does). `regen_features` is the resolve's own
/// `config_features_string` for the placed config (the merge-time
/// `FEATURES` the regen'd vdb env carries, backlog #171);
/// `regen_bzip2` is the resolve's own `config_bzip2_command` for the
/// placed config (the merge-time `PORTAGE_BZIP2_COMMAND` the scrubbed
/// vdb env records). Prints the
/// stage report lines; `Ok(cpv)` is the merged `category/package-version`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn run_binpkg_flow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
    ledger: Option<&LedgerSpec>,
    repo_override: Option<&str>,
    shadow: Option<&VdbShadow>,
    install_mask: &str,
    install_mask_prunes_usr_share: bool,
    regen_features: Option<&str>,
    regen_bzip2: Option<&str>,
) -> Result<String, String> {
    let staging = TempDir::new("portuale-remote-bundle").keep();
    if let Err(message) = std::fs::create_dir_all(&staging) {
        return Err(format!("mrg: staging dir {}: {message}", staging.display()));
    }
    let staged = match crate::remote_bundle::build_bundle(
        binpkg_path,
        &staging,
        repo_override,
        &crate::binpkg::GpgVerify::from_env(),
        install_mask,
        install_mask_prunes_usr_share,
    ) {
        Ok(staged) => staged,
        Err(message) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("mrg: bundle build failed: {message}"));
        }
    };
    let pf = staged
        .manifest
        .cpv
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    // Pre-ship gate (slice 6): the shadow fails a foreign-owned unit
    // before its tarball streams anywhere -- zero client writes so far.
    // The staged `filemeta` is the exact bytes the client will gate on.
    if let Some(shadow) = shadow {
        let filemeta_path = staging.join(&pf).join("filemeta");
        match std::fs::read_to_string(&filemeta_path) {
            Ok(text) => {
                if let Err(message) = shadow_precheck(
                    shadow,
                    &staged.manifest.cpv,
                    &staged.category,
                    &staged.pn,
                    &text,
                ) {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(message);
                }
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!("mrg: reading staged filemeta: {e}"));
            }
        }
    }
    let dest_tar = format!("{}/{pf}/bundle.tar", ctx.workdir);
    // Fail-early: the stream lands (or fails) before the driver runs.
    if ctx.transport == RemoteTransport::Ssh {
        // The unit dir must exist for `cat >` (no `mkdir -p` hiding a
        // wrong workdir -- preflight already proved creatability).
        let mkdir = format!("mkdir -p {}", sh_quote(&format!("{}/{pf}", ctx.workdir)));
        match run_script_stdin(ctx, control, &mkdir) {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!(
                    "mrg: creating unit dir failed (exit {}):\n{stderr}",
                    output.status.code().unwrap_or(-1)
                ));
            }
            Err(message) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(message);
            }
        }
    }
    if let Err(message) = send_file(ctx, control, &staged.tarball, &dest_tar) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(message);
    }
    let script = unpack_script(&ctx.workdir, &pf, staged.byte_count);
    let output = match run_script_stdin(ctx, control, &script) {
        Ok(output) => output,
        Err(message) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(message);
        }
    };
    let _ = std::fs::remove_dir_all(&staging);
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        println!("{line}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let unpack = parse_kv(&stdout).get("UNPACK").cloned().unwrap_or_default();
    if !(output.status.success() && unpack == "ok") {
        let mut message = format!(
            "mrg: bundle unpack failed (exit {}, UNPACK={unpack}):",
            code.unwrap_or(-1)
        );
        for line in stderr.lines().take(5) {
            message.push_str(&format!("\nmrg:   {line}"));
        }
        return Err(message);
    }
    println!(
        ">>> Remote bundle {}: unpacked ({} bytes, slot {}, repo {})",
        staged.manifest.cpv, staged.byte_count, staged.manifest.slot, staged.manifest.repo,
    );
    let (vdb, stateless) = client_vdb_placement(ctx);
    // Backlog #171: stage the replaced instance's saved env for the
    // merge driver's old hooks (server-decompressed, so the client
    // needs no bzip2). Best-effort: failures warn and the driver falls
    // back to the client vdb. Stateless merges run no old hooks.
    if !stateless {
        let unit_dir = format!("{}/{pf}", ctx.workdir);
        let mainslot = staged.manifest.slot.split('/').next().unwrap_or("0");
        let shipment = ship_old_hook_envs(
            ctx,
            control,
            &unit_dir,
            &vdb,
            &staged.category,
            &staged.pn,
            &staged.pf,
            mainslot,
        );
        for pf in &shipment.shipped {
            println!(
                ">>> Remote old-env {}: staged for {pf}",
                staged.manifest.cpv
            );
        }
        for warning in &shipment.warnings {
            println!("{}", old_hook_warn_message(&staged.manifest.cpv, warning));
        }
    }
    // Slice-3 phases (pretend/setup/preinst, DEFINED_PHASES-gated at
    // bundle time). Postinst waits for the slice-4 merge. An empty phase
    // list (no hooks, or no ebuild/env shipped) is a note, not a failure
    // -- same degrade as the local merge -- and, crucially, not a skip:
    // the copy+vdb merge below runs regardless (a hookless binpkg still
    // installs files; returning early here silently unmerged it).
    if staged.phases.is_empty() {
        println!(
            ">>> Remote phases {}: none defined, skipped",
            staged.manifest.cpv
        );
    } else {
        let unit_dir = format!("{}/{pf}", ctx.workdir);
        match run_phases_stage(ctx, control, &unit_dir, &staged) {
            Ok(done) => {
                println!(
                    ">>> Remote phases {}: {}",
                    staged.manifest.cpv,
                    done.join(", ")
                );
            }
            Err(message) => {
                return Err(message);
            }
        }
    }
    let unit_dir = format!("{}/{pf}", ctx.workdir);
    // Slice-4 merge (copy+vdb+replace); the regen postinst below stays
    // non-fatal like the local merge's own `_postinst_failure` rule.
    match run_merge_stage(ctx, control, &unit_dir, &staged, ledger) {
        Ok(markers) => {
            for marker in &markers {
                println!(">>> Remote merge {}: {marker}", staged.manifest.cpv);
            }
        }
        Err(message) => {
            return Err(message);
        }
    }
    // Backlog #171: the postinst phase runs for every merged unit --
    // defined or not -- as the merge-time environment regeneration
    // (real `vartree.py:5334` + `phase-functions.sh:1072-1082`; an
    // undefined `pkg_postinst` is a no-op that still re-saves the env).
    // Any regen/pull/compress/install failure keeps the build-time env
    // already in the vdb with one `!!!` warning and continues (real
    // `_postinst_failure`: "It's stupid to bail out here").
    let cpv = staged.manifest.cpv.clone();
    // Backlog #171b: the calling environment for the client phase is
    // the server process -- forward its set locale variables (real's
    // `environ_whitelist` rule; never invented defaults).
    let server_locale = collect_server_locale(|name| std::env::var(name).ok());
    match run_postinst_regen_stage(
        ctx,
        control,
        &unit_dir,
        &staged,
        regen_features,
        &server_locale,
    ) {
        Ok(report) => {
            if report.skipped_no_hooks {
                println!(">>> Remote postinst {cpv}: none to run from, skipped");
            } else {
                if !staged.postinst_defined {
                    println!(">>> Remote postinst {cpv}: no pkg_postinst defined, regen only");
                }
                if report.phase_rc == 0 {
                    println!(">>> Remote postinst {cpv}: ok");
                } else {
                    println!(
                        ">>> Remote postinst {cpv}: FAILED (exit {}) -- merge kept (as Portage's _postinst_failure)",
                        report.phase_rc
                    );
                }
                if report.regen_present {
                    if stateless {
                        println!(">>> Remote env-regen {cpv}: skipped (stateless, no client vdb)");
                    } else {
                        let vdb_env =
                            format!("{vdb}/{}/{}/environment.bz2", staged.category, staged.pf);
                        match install_regenerated_env(
                            ctx,
                            control,
                            &unit_dir,
                            &vdb_env,
                            regen_bzip2,
                        ) {
                            Ok(()) => println!(
                                ">>> Remote env-regen {cpv}: vdb environment.bz2 regenerated"
                            ),
                            Err((step, detail)) => {
                                println!("{}", regen_warn_message(&cpv, &step));
                                for line in detail.lines().take(3) {
                                    println!("mrg:   {line}");
                                }
                            }
                        }
                    }
                } else {
                    println!("{}", regen_warn_message(&cpv, "postinst"));
                }
            }
        }
        Err(message) => {
            println!(">>> Remote postinst {cpv}: transport failed, merge kept: {message}");
            println!("{}", regen_warn_message(&cpv, "postinst"));
        }
    }
    println!(">>> Remote merged {cpv}");
    Ok(cpv)
}

pub(crate) fn run_bundle_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
) -> ExitCode {
    if !binpkg_path.is_file() {
        eprintln!("mrg: --remote-binpkg {}: not found", binpkg_path.display());
        return ExitCode::from(1);
    }
    // Backlog #171b: the trial path resolves the client config exactly
    // like the plan path does -- place `/etc/portage` per
    // `ConfigPlacement` through the shared `place_config_root` helper
    // (same shape as `run_remote_resolve`), then load the same repos +
    // resolved config `pretend::run` merges with
    // (`load_repos_and_config`: one shared resolver call, no duplicate
    // logic). The placed config's `INSTALL_MASK` (+ the
    // `no{man,info,doc}` fold), resolved `FEATURES`, and
    // `PORTAGE_BZIP2_COMMAND` feed `build_bundle` / the env regen
    // exactly as `run_remote_plan`'s own values do. Both placements
    // always name a path, so there is no config-less mode left here to
    // keep the old empty values for; a pull or resolve failure is a
    // hard error like the resolve path's own. `_placed` stays alive
    // across the flow; its temp pull is removed best-effort on drop.
    let _placed = match place_config_root(ctx, control) {
        Ok(placed) => placed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let config_dir = _placed.dir.to_path_buf();
    let (_repos, config) =
        match crate::pretend::load_repos_and_config(&config_dir, &portage_repo::root_from_env()) {
            Ok(loaded) => loaded,
            Err(message) => {
                eprintln!("mrg: --remote-binpkg: cannot resolve the client config: {message}");
                return ExitCode::from(1);
            }
        };
    let (install_mask, install_mask_prunes_usr_share) =
        crate::pretend::config_install_mask(&config);
    let regen_features = crate::pretend::config_features_string(&config);
    let regen_bzip2 = crate::pretend::config_bzip2_command(&config);
    // S8.2 install, like the resolve path: the trial ships exactly one
    // unit, so the stage always runs here (never under `--pretend`,
    // which never reaches this executor).
    if let Err(message) = ensure_client_binary(ctx, control) {
        eprintln!("{message}");
        return ExitCode::from(1);
    }
    match run_binpkg_flow(
        ctx,
        control,
        binpkg_path,
        None,
        None,
        None,
        &install_mask,
        install_mask_prunes_usr_share,
        Some(&regen_features),
        Some(&regen_bzip2),
    ) {
        Ok(_) => ExitCode::from(0),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}
