//! `--prune` and `--clean`.

use crate::pretend::*;

/// Arguments of [`run_prune_pretend`].
pub(crate) struct RunPrunePretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    /// The `repos.conf` repos, for the post-removal GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

/// `emerge --prune`: unlike `--depclean` there is no `* ` advisory block
/// and no stats block, and the empty-cleanlist message gains a `To ignore
/// dependencies, use --nodeps` line. Without `--pretend` the shared
/// `run_unmerge_pretend` really removes the cleanlist. See
/// `portage_repo::prune_cleanlist` for the removal-set semantics.
/// Portage: _emerge/actions.py, action_depclean
pub(crate) fn run_prune_pretend(args: RunPrunePretendArgs<'_>) -> ExitCode {
    let RunPrunePretendArgs {
        targets,
        root,
        config_root,
        config,
        repos,
        verbose,
        lib_check,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
    } = args;
    let args = match resolve_cleanup_args(targets, root, "prune") {
        Ok(a) => a,
        Err(code) => return code,
    };

    let result = portage_repo::prune_cleanlist(root, &args, &[], &config.package_provided);
    // The `unresolved_deps()` safety halt applies here too (with the
    // prune-only trailer).
    // Portage: _emerge/actions.py, _calc_depclean
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, true, color) {
        return code;
    }
    // `--depclean-lib-check` applies to `--prune` too.
    // Portage: _emerge/actions.py, _calc_depclean
    // The inline reverse-dep display prints while the removal list is
    // built — once per `create_cleanlist()` pass, so twice when the lib
    // check fires a second graph.
    if verbose {
        print_prune_parents(&result);
    }
    let lib_check_phase = LibCheck {
        on: lib_check,
        preserve_libs: config_has_preserve_libs(config),
    };
    let (result, recomputed) =
        apply_depclean_lib_check(root, result, lib_check_phase, color, |providers| {
            portage_repo::prune_cleanlist(root, &args, providers, &config.package_provided)
        });
    if recomputed {
        if resolution_notice_shown(quiet || debug, false) {
            println!("\nCalculating dependencies ... done!");
        }
        if verbose {
            print_prune_parents(&result);
        }
    }

    if result.cleanlist.is_empty() {
        println!(">>> No packages selected for removal by prune");
        // Real `create_cleanlist`: `if "--verbose" not in myopts`.
        if !verbose {
            println!(">>> To see reverse dependencies, use --verbose");
        }
        println!(">>> To ignore dependencies, use --nodeps");
        return ExitCode::SUCCESS;
    }

    println!(">>> Calculating removal order...");
    let cpv_atoms: Vec<String> = result
        .cleanlist
        .iter()
        .map(|p| format!("={}", p.cpv()))
        .collect();
    let cpv_refs: Vec<&str> = cpv_atoms.iter().map(String::as_str).collect();
    // Without `--pretend`, the shared unmerge path removes them.
    // Portage: _emerge/actions.py, action_depclean
    run_unmerge_pretend(RunUnmergePretendArgs {
        targets: &cpv_refs,
        action: "unmerge",
        safety_note: false,
        root,
        config_root,
        config,
        repos,
        preserve_order: result.ordered,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
    })
}

/// The `--verbose` reverse-dependency list of a `--prune` pass.
// Portage: _emerge/actions.py, create_cleanlist
fn print_prune_parents(result: &portage_repo::DepcleanResult) {
    for (pkg, lines) in &result.kept_parents {
        println!("  {} pulled in by:", pkg.cpv());
        for line in lines {
            println!("    {line}");
        }
        println!();
    }
}

/// Arguments of [`run_prune_nodeps_pretend`].
pub(crate) struct RunPruneNodepsPretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config, for the post-removal
    /// GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

/// `emerge --prune --nodeps`: no dependency check at all. For every cp with
/// more than one version installed the highest is `protected` and the rest
/// are `selected`. Empty result exits 1. Without `--pretend` the selected
/// versions are removed after the display.
/// Portage: _emerge/actions.py; unmerge.py, _unmerge_display
pub(crate) fn run_prune_nodeps_pretend(args: RunPruneNodepsPretendArgs<'_>) -> ExitCode {
    let RunPruneNodepsPretendArgs {
        targets,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
        repos,
        config,
    } = args;
    run_prune_nodeps_or_clean(RunPruneNodepsOrCleanArgs {
        targets,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        is_clean: false,
        autoclean: false,
        quiet,
        noinfo,
        repos,
        config,
    })
}

/// Arguments of [`run_clean_pretend`].
pub(crate) struct RunCleanPretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config, for the post-removal
    /// GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

/// `emerge --clean`: identical to `--prune --nodeps` except the best/rest
/// split is per slot (see `portage_repo::clean_selection`), there is no
/// `sys-apps/portage` self-skip, and the empty-selection message names
/// `clean`. Individual ebuild-path args are rejected.
/// Portage: _emerge/actions.py, action_uninstall; unmerge.py
pub(crate) fn run_clean_pretend(args: RunCleanPretendArgs<'_>) -> ExitCode {
    let RunCleanPretendArgs {
        targets,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
        repos,
        config,
    } = args;
    run_prune_nodeps_or_clean(RunPruneNodepsOrCleanArgs {
        targets,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        is_clean: true,
        autoclean: false,
        quiet,
        noinfo,
        repos,
        config,
    })
}
/// Arguments of [`run_prune_nodeps_or_clean`].
pub(crate) struct RunPruneNodepsOrCleanArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) is_clean: bool,
    /// The post-merge auto-clean: no `CLEAN_DELAY` countdown (Portage
    /// skips it when `autoclean` is set) and no pretend header (the run
    /// is never `--pretend`; see `run_autoclean_after_merge`).
    /// Portage: _emerge/unmerge.py, unmerge
    pub(crate) autoclean: bool,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config, for the post-removal
    /// GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

pub(crate) fn run_prune_nodeps_or_clean(args: RunPruneNodepsOrCleanArgs<'_>) -> ExitCode {
    let RunPruneNodepsOrCleanArgs {
        targets,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        is_clean,
        autoclean,
        quiet,
        noinfo,
        repos,
        config,
    } = args;
    let action = if is_clean { "clean" } else { "prune" };
    let args = match resolve_cleanup_args(targets, root, action) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut selection = if is_clean {
        portage_repo::clean_selection(root, &args)
    } else {
        portage_repo::prune_nodeps_selection(root, &args)
    };

    // The header is `--pretend`/`--ask`-only, preceded by the non-`/` ROOT
    // notice; both carry Portage's leading blank line.
    // Portage: unmerge.py, _unmerge_display
    print_unmerge_display_header(UnmergeDisplayHeaderArgs {
        root,
        pretend,
        quiet,
        color,
    });

    // `sys-apps/portage` self-skip: selected versions move to `protected`
    // with the eerror. Dead code in practice (never installed twice), kept
    // for fidelity. `--clean` does not self-skip.
    // Portage: unmerge.py, _unmerge_display
    if !is_clean {
        for cp in &mut selection {
            if (cp.category.as_str(), cp.package.as_str()) == ("sys-apps", "portage") {
                for v in std::mem::take(&mut cp.other_versions) {
                    eprintln!(
                        "!!! Not unmerging package sys-apps/portage-{v} since there is no valid \
                         reason for Portage to prune itself."
                    );
                }
            }
        }
    }

    let total_selected: usize = selection.iter().map(|cp| cp.other_versions.len()).sum();
    if total_selected == 0 {
        if args.is_empty() {
            println!("\n>>> No outdated packages were found on your system.");
        } else {
            println!("\n>>> No packages selected for removal by {action}");
        }
        return ExitCode::from(1);
    }

    // "Still listed in package sets" warning; same machinery as the
    // unmerge path.
    // Portage: unmerge.py, _unmerge_display
    let installed_sets: Vec<(String, Vec<String>)> = match collect_installed_sets(config_root, root)
    {
        Ok(sets) => sets,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let mut selected_flat: Vec<(String, String, String)> = Vec::new();
    for cp in &selection {
        for v in &cp.other_versions {
            selected_flat.push((cp.category.clone(), cp.package.clone(), v.clone()));
        }
    }
    selected_flat.sort();
    for (cat, pkg, version) in &selected_flat {
        let mut parents = still_listed_parents(root, &installed_sets, cat, pkg, version);
        if !parents.is_empty() {
            parents.sort_unstable();
            println!(
                "{}",
                color.c(
                    "WARN",
                    &format!("Package {cat}/{pkg}-{version} is going to be unmerged,")
                )
            );
            println!(
                "{}",
                color.c("WARN", "but still listed in the following package sets:")
            );
            println!("    {}\n", parents.join(", "));
        }
    }

    // Per-cp blocks (already cp-sorted by `prune_nodeps_selection`).
    let mut all_selected_display: Vec<String> = Vec::new();
    let mut removal_list: Vec<(String, String, String)> = Vec::new();
    for cp in &selection {
        if cp.other_versions.is_empty() {
            continue;
        }
        println!("\n {}/{}", cp.category, cp.package);
        print_unmerge_row("selected", &cp.other_versions, color);
        print_unmerge_row("protected", std::slice::from_ref(&cp.best_version), color);
        print_unmerge_row("omitted", &[], color);
        for v in &cp.other_versions {
            all_selected_display.push(format!("={}/{}-{v}", cp.category, cp.package));
            removal_list.push((cp.category.clone(), cp.package.clone(), v.clone()));
        }
    }

    all_selected_display.sort();
    println!(
        "\nAll selected packages: {}",
        all_selected_display.join(" ")
    );
    println!(
        "\n>>> {} packages are slated for removal.",
        color.c("UNMERGE_WARN", "'Selected'")
    );
    // Portage's legend block ends with a blank line (unmerge.py:563-569).
    println!(
        ">>> {} and {} packages will not be removed.\n",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );

    if !pretend {
        if ask && !ask_confirm(color, "Would you like to unmerge these packages?") {
            return ExitCode::from(130);
        }
        // The post-merge auto-clean never counts down, whatever
        // `CLEAN_DELAY` says; a manual `--clean`/`--prune` does.
        // Portage: _emerge/unmerge.py, unmerge
        if !autoclean {
            clean_delay_countdown(Some(config));
        }
        return execute_unmerge(ExecuteUnmergeArgs {
            removal_list: &removal_list,
            root,
            shell,
            debug,
            color,
            quiet,
            noinfo,
            repos,
            config,
            autoclean,
        });
    }
    ExitCode::SUCCESS
}
