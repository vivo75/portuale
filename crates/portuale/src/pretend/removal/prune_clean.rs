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

    // The header is `--pretend`/`--ask`-only.
    // Portage: unmerge.py, _unmerge_display
    if pretend {
        println!(
            "{}",
            color.c(
                "darkgreen",
                ">>> These are the packages that would be unmerged:"
            )
        );
    }

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
    println!(
        ">>> {} and {} packages will not be removed.",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );

    if !pretend {
        if ask && !ask_confirm(color, "Would you like to unmerge these packages?") {
            return ExitCode::from(130);
        }
        clean_delay_countdown();
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
        });
    }
    ExitCode::SUCCESS
}
