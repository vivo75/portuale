//! The remote entry points: resolve, plan, and run each unit.

use crate::remote::*;

/// mrg entry point for remote mode: `--remote-binpkg` runs the
/// single-file flow, target atoms run the resolve flow (which enforces
/// `--getbinpkgonly`). `--pretend` never reaches here (mrg routes it
/// locally with the remote options dropped).
pub fn run_remote_cli(matches: &ArgMatches, ctx: RemoteContext, argv: Vec<String>) -> ExitCode {
    if ctx.binpkg.is_some() {
        if matches.get_many::<String>("package").is_some() {
            eprintln!("mrg: --remote-binpkg cannot be combined with target atoms");
            return ExitCode::from(2);
        }
        return run_remote(&ctx);
    }
    // No binpkg and no atoms: the slice-1 preflight-only run (no payload,
    // no resolve) -- `--getbinpkgonly` is only forced once targets exist.
    if matches.get_many::<String>("package").is_none() {
        return run_remote(&ctx);
    }
    run_remote_resolve(matches, ctx, argv)
}

// --- Resolve-path support ----------------------------------------------------------

/// Repo position for one merged entry: `(repo name, commit hash)` from
/// the repo checkout the entry resolved from (`unknown` when the repo is
/// not git or git is unavailable).
pub(crate) fn repo_position(
    repos: &[portage_repo::RepoConfig],
    repo_name: &Option<String>,
) -> (String, String) {
    let name = repo_name
        .clone()
        .unwrap_or_else(|| "__unknown__".to_string());
    let commit = repos
        .iter()
        .find(|repo| repo.name == name)
        .and_then(|repo| {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(&repo.location)
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|hash| hash.trim().to_string())
                .filter(|hash| !hash.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    (name, commit)
}

/// mrg atoms-mode entry point: enforce `--getbinpkgonly`, place
/// `/etc/portage`, hand the resolve to `pretend::run` with the remote
/// execution published. Exit 2 = usage error, else pretend's own code.
pub fn run_remote_resolve(
    matches: &ArgMatches,
    mut ctx: RemoteContext,
    argv: Vec<String>,
) -> ExitCode {
    if !matches.get_flag("getbinpkgonly") {
        eprintln!("mrg: remote execution requires --getbinpkgonly (no source build on client)");
        return ExitCode::from(2);
    }
    if matches.get_flag("buildpkgonly") {
        eprintln!(
            "mrg: --remote-* cannot be combined with --buildpkgonly (remote runs install, not build)"
        );
        return ExitCode::from(2);
    }
    if let Some(action) = crate::mrg::action_selected(matches) {
        eprintln!(
            "mrg: --remote-hostname cannot be combined with --{action} (remote runs install, not actions)"
        );
        return ExitCode::from(2);
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Fail-early: client sanity before resolving.
    let (values, bin) = match run_preflight_inner(&ctx, control) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (mut failures, mut warnings) = evaluate_preflight(&ctx, &values);
    // Binary gates (read-only): the plan rides the handoff into
    // `run_remote_plan`, which installs only when units ship. Local
    // transport without the override skips the gates and keeps
    // `current_exe()`.
    match bin {
        Some(pre) => {
            let override_given = ctx.portuale_dir.is_some();
            let (plan, mut bin_failures, bin_notes) =
                evaluate_bin_gates(&values, &pre, override_given);
            failures.append(&mut bin_failures);
            for note in bin_notes {
                warnings.push(note);
            }
            ctx.bin_plan = plan;
        }
        None => {
            ctx.bin_plan = ClientBinPlan::Local(crate::self_exe::self_exe().display().to_string());
        }
    }
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(&ctx);
    if !failures.is_empty() {
        return print_preflight_report(&ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(&ctx, &failures, &warnings, first_contact);
    // Place etc-portage for the resolve; `_placed` stays alive across
    // it and its temp pull is removed best-effort on drop.
    let _placed = match place_config_root(&ctx, control) {
        Ok(placed) => placed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    set_remote_exec(ctx);
    let code = crate::pretend::run(&argv);
    // Defensive: the dispatch site takes the handoff, but an early return
    // (resolve error, unexpected action) must not leak it in-process.
    let _ = take_remote_exec();
    code
}

/// BFS-drop the transitive dependents of a failed package (Portage
/// `_calc_resume_list`): every skipped cp records the failed cpv that
/// doomed it, for the `skipped (… failed)` trailer.
pub(crate) fn drop_dependents(
    dependents: &std::collections::HashMap<(String, String), Vec<(String, String)>>,
    skip: &mut std::collections::HashMap<(String, String), String>,
    failed_cp: (String, String),
    failed_cpv: &str,
) {
    let mut queue = vec![failed_cp];
    while let Some(x) = queue.pop() {
        if let Some(deps) = dependents.get(&x) {
            for p in deps {
                if skip.insert(p.clone(), failed_cpv.to_string()).is_none() {
                    queue.push(p.clone());
                }
            }
        }
    }
}

/// Arguments of [`run_remote_plan`].
pub(crate) struct RunRemotePlanArgs<'a> {
    pub(crate) entries: &'a [portage_repo::GraphEntry],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a std::path::Path,
    pub(crate) pkgdir: &'a std::path::Path,
    pub(crate) portage_tmpdir: &'a std::path::Path,
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) keep_going: bool,
}

/// One resolved binary plan, executed remotely entry by entry in merge
/// order (sequential). Report is per-unit trailers plus a closing
/// `>>> Remote summary: …` line. Without `--keep-going` the first unit
/// error aborts the plan; with it, the error fails just that unit and
/// BFS-drops its transitive dependents, merging the rest.
/// `AlreadyInstalled` stays a silent no-op like the local plan. Anything
/// else non-`Binary` cannot occur (`check_binary_plan` gates first) and
/// fails loudly.
pub(crate) fn run_remote_plan(args: RunRemotePlanArgs<'_>) -> Result<(), String> {
    let RunRemotePlanArgs {
        entries,
        config,
        repos,
        root,
        pkgdir,
        portage_tmpdir,
        ctx,
        keep_going,
    } = args;
    use portage_repo::PretendOutcome;
    use std::collections::HashMap;
    // Every shipped unit carries the vendored phase runtime, so a binary
    // that cannot find it fails here -- before the vdb shadow, the ledger
    // gate, any ssh and any write -- instead of panicking after the plan.
    if entries.iter().any(|e| {
        !matches!(
            e.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        )
    }) {
        crate::ebuild_phases::require_phase_runtime()?;
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Vdb shadow for the pre-ship pre-check: pulled once for client
    // placement, read directly for server placement.
    let shadow = load_vdb_shadow(ctx, control)?;
    if matches!(ctx.vdb, ConfigPlacement::Server(_)) {
        println!(
            "mrg: note: --remote-vdb is server-side: client merges run stateless (no old hooks, fail-closed collisions)"
        );
    }
    // Server ledger base: `--remote-ledger-dir` wins, else the placed
    // config's own `<PKGDIR>/remote-ledger`.
    let server_ledger_base: std::path::PathBuf = match &ctx.ledger_dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => pkgdir.join("remote-ledger"),
    };
    // Strict-mode provenance gate: fail early, before anything ships,
    // when the client and server ledger record disagree.
    if ctx.require_ledger_match {
        check_ledger_match(ctx, control, &server_ledger_base)?;
    }
    // cp -> the cps that depend on it (each entry's own `required_by`).
    let dependents: HashMap<(String, String), Vec<(String, String)>> = entries
        .iter()
        .map(|e| {
            (
                (e.category.clone(), e.package.clone()),
                e.required_by.clone(),
            )
        })
        .collect();
    // Skipped cp -> the failed cpv that doomed it (for the trailer).
    let mut skip: HashMap<(String, String), String> = HashMap::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut merged: u32 = 0;
    // The placed config's resolved `INSTALL_MASK` + the `no{man,info,doc}`
    // fold, through the same resolver the local merge uses -- so a remote
    // merge masks exactly what a local one would.
    let (install_mask, install_mask_prunes_usr_share) = crate::pretend::config_install_mask(config);
    // The same placed config's resolved `FEATURES` list rides every
    // unit's regen postinst run, so the vdb env carries the merge-time
    // features, not the binpkg's build-time ones.
    let regen_features = crate::pretend::config_features_string(config);
    // The same placed config's resolved `PORTAGE_BZIP2_COMMAND` rides the
    // regen install too, so the scrubbed vdb env records the client's
    // configured compressor.
    let regen_bzip2 = crate::pretend::config_bzip2_command(config);
    // Client-binary install (a client mutation, so after the read-only
    // stages): one stage per invocation, only when at least one unit
    // ships. `--pretend` never reaches here.
    let ships = entries.iter().any(|e| {
        matches!(
            e.outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Reinstall { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        )
    });
    if ships {
        ensure_client_binary(ctx, control)?;
    }
    for entry in entries {
        let version = match &entry.outcome {
            // A removal is not remotely merged (execution is a non-goal).
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::Uninstall { .. } => continue,
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                version.clone()
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
            PretendOutcome::NoVisibleCandidate => {
                return Err(format!(
                    "no binary package available for {}/{}",
                    entry.category, entry.package
                ));
            }
        };
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let cp = (entry.category.clone(), entry.package.clone());
        if let Some(culprit) = skip.get(&cp) {
            skipped.push(cpv.clone());
            println!(">>> Remote {cpv}: skipped ({culprit} failed)");
            continue;
        }
        let unit = run_one_remote_unit(RunOneRemoteUnitArgs {
            entry,
            version: &version,
            cpv: &cpv,
            config,
            repos,
            root,
            pkgdir,
            ctx,
            control,
            shadow: &shadow,
            server_ledger_base: &server_ledger_base,
            install_mask: &install_mask,
            install_mask_prunes_usr_share,
            regen_features: Some(&regen_features),
            regen_bzip2: Some(&regen_bzip2),
        });
        match unit {
            Ok(()) => {
                // The flow's own `>>> Remote merged <cpv>` line is this
                // unit's trailer; only failed/skipped need plan-level ones.
                merged += 1;
            }
            Err(message) => {
                let short = message.lines().next().unwrap_or(&message).to_string();
                println!("!!! Remote {cpv}: failed: {short}");
                if !keep_going {
                    return Err(message);
                }
                failures.push((cpv.clone(), message));
                drop_dependents(&dependents, &mut skip, cp, &cpv);
            }
        }
    }
    println!(
        ">>> Remote summary: {merged} merged, {} failed, {} skipped",
        failures.len(),
        skipped.len()
    );
    let _ = portage_tmpdir;
    if failures.is_empty() {
        return Ok(());
    }
    let mut msg = format!(
        "Remote plan finished with {} failed unit(s) (--keep-going):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(cpv, e)| format!("  {cpv}: {}", e.lines().next().unwrap_or(e)))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if !skipped.is_empty() {
        msg.push_str(&format!(
            "\n{} dependent package(s) not merged:\n{}",
            skipped.len(),
            skipped
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Err(msg)
}

/// Arguments of [`run_one_remote_unit`].
pub(crate) struct RunOneRemoteUnitArgs<'a> {
    pub(crate) entry: &'a portage_repo::GraphEntry,
    pub(crate) version: &'a str,
    pub(crate) cpv: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a std::path::Path,
    pub(crate) pkgdir: &'a std::path::Path,
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) control: Option<&'a std::path::Path>,
    pub(crate) shadow: &'a VdbShadow,
    pub(crate) server_ledger_base: &'a std::path::Path,
    pub(crate) install_mask: &'a str,
    pub(crate) install_mask_prunes_usr_share: bool,
    pub(crate) regen_features: Option<&'a str>,
    pub(crate) regen_bzip2: Option<&'a str>,
}

/// One merge-bound plan entry end to end: locate the binpkg (binhost
/// download or local `$PKGDIR`), run it through the client
/// (bundle → unpack → phases → merge → postinst, shadow pre-check
/// before anything ships), then record the server ledger. `Err` is the
/// unit's failure; the caller decides abort vs. keep-going.
pub(crate) fn run_one_remote_unit(args: RunOneRemoteUnitArgs<'_>) -> Result<(), String> {
    let RunOneRemoteUnitArgs {
        entry,
        version,
        cpv,
        config,
        repos,
        root,
        pkgdir,
        ctx,
        control,
        shadow,
        server_ledger_base,
        install_mask,
        install_mask_prunes_usr_share,
        regen_features,
        regen_bzip2,
    } = args;
    let binpkg_path = if entry.remote_binary {
        let (binrepo, record) =
            portage_repo::find_remote_binpkg_instance(portage_repo::FindRemoteBinpkgInstanceArgs {
                binrepos: &config.binrepos,
                root,
                category: &entry.category,
                package: &entry.package,
                version,
                build_id: entry.build_id.as_deref(),
            })
            .ok_or_else(|| {
                format!(
                    "{}/{}-{version}: not found in any binhost `Packages` index",
                    entry.category, entry.package
                )
            })?;
        crate::emerge_getbinpkg::download_and_verify(
            crate::emerge_getbinpkg::DownloadAndVerifyArgs {
                sync_uri: &binrepo.sync_uri,
                record: &record,
                category: &entry.category,
                package: &entry.package,
                version,
                pkgdir: &crate::emerge_getbinpkg::binrepo_download_dir(
                    config,
                    binrepo,
                    &crate::emerge_getbinpkg::eprefix_from_env(),
                    pkgdir,
                ),
                config,
            },
        )
        .map_err(|e| e.to_string())?
    } else {
        crate::emerge_getbinpkg::resolve_local_binpkg(
            pkgdir,
            &entry.category,
            &entry.package,
            version,
            entry.build_id.as_deref(),
        )
        .ok_or_else(|| {
            format!(
                "{}/{}-{version}: no binpkg file under {}",
                entry.category,
                entry.package,
                pkgdir.display()
            )
        })?
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs())
        .unwrap_or(0);
    let (repo, commit) = repo_position(repos, &entry.repo_name);
    let ledger_entry = LedgerEntry {
        ts,
        repo,
        commit,
        cpv: cpv.to_string(),
    };
    let ledger = LedgerSpec {
        file: client_ledger_file(ctx),
        line: ledger_entry.line(),
    };
    run_binpkg_flow(RunBinpkgFlowArgs {
        ctx,
        control,
        binpkg_path: &binpkg_path,
        ledger: Some(&ledger),
        repo_override: Some(&ledger_entry.repo),
        shadow: Some(shadow),
        install_mask,
        install_mask_prunes_usr_share,
        regen_features,
        regen_bzip2,
    })?;
    record_server_ledger(server_ledger_base, &ctx.hostname, &ledger_entry)?;
    Ok(())
}

/// Entry point: connect, preflight, report. Exit 0 = every hard gate
/// passed; 1 = a gate failed or the client is unreachable. With
/// `--remote-binpkg`: preflight, then bundle, stream, unpack and verify
/// one explicit binpkg file.
pub fn run_remote(ctx: &RemoteContext) -> ExitCode {
    // An explicit `--remote-binpkg` bundles the phase runtime into the
    // unit, so a binary with no runtime fails here, with one message,
    // before the first connection -- not after the preflight.
    if ctx.binpkg.is_some()
        && let Err(message) = crate::ebuild_phases::require_phase_runtime()
    {
        eprintln!("mrg: {message}");
        return ExitCode::from(1);
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Before the first connection: afterwards the key is present even on
    // first contact, so this decision cannot be made later.
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(ctx);
    let (values, bin) = match run_preflight_inner(ctx, control) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (mut failures, mut warnings) = evaluate_preflight(ctx, &values);
    // The trial path ships one unit, so it enforces the binary gates
    // like the resolve path; the preflight-only run ships nothing and
    // skips them (like `--pretend`, which installs nothing).
    let mut ctx = ctx.clone();
    if ctx.binpkg.is_some() {
        match bin {
            Some(pre) => {
                let override_given = ctx.portuale_dir.is_some();
                let (plan, mut bin_failures, bin_notes) =
                    evaluate_bin_gates(&values, &pre, override_given);
                failures.append(&mut bin_failures);
                for note in bin_notes {
                    warnings.push(note);
                }
                ctx.bin_plan = plan;
            }
            None => {
                ctx.bin_plan =
                    ClientBinPlan::Local(crate::self_exe::self_exe().display().to_string());
            }
        }
    }
    if !failures.is_empty() {
        return print_preflight_report(&ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(&ctx, &failures, &warnings, first_contact);
    match &ctx.binpkg {
        None => ExitCode::from(0),
        Some(path) => {
            let path = Path::new(path).to_path_buf();
            run_bundle_stage(&ctx, control, &path)
        }
    }
}
