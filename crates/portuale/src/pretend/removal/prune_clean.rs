//! `--prune` and `--clean`.

use crate::pretend::*;

/// Arguments of [`run_prune_pretend`].
pub(crate) struct RunPrunePretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    /// The `repos.conf` repos -- threaded into the post-removal GLEP 42
    /// count notice (backlog #196 fix round 1).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Threaded into the post-removal info-regen block (real `--quiet`
    /// suppression + resolved-config `noinfo`); computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

/// Real `emerge --prune` / `-P` (real `action_depclean` with
/// `action="prune"`). Unlike `--depclean`, real `action_depclean`
/// returns right after `unmerge()` (`actions.py:888`), so there is
/// **no** `* ` advisory block (only `action == "depclean"` prints it,
/// `:840`) and **no** `Packages installed:` / `Required packages:` /
/// `Number ...` stats block. The empty-cleanlist message gains a `>>>
/// To ignore dependencies, use --nodeps` line (`create_cleanlist`,
/// `:1348`). Without `--pretend` the shared `run_unmerge_pretend` really
/// removes the cleanlist (`execute_unmerge`). See
/// `portage_repo::prune_cleanlist`'s own doc comment for the removal-set
/// semantics.
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
    // Real `_calc_depclean`'s `unresolved_deps()` safety halt -- serves
    // `action in ("depclean", "prune")`, so it applies here too (with the
    // prune-only `use --nodeps` trailer).
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, true, color) {
        return code;
    }
    // Real `_calc_depclean` serves `action in ("depclean", "prune")`, so
    // `--depclean-lib-check` applies to `--prune` too.
    let result = apply_depclean_lib_check(root, result, lib_check, color, |providers| {
        portage_repo::prune_cleanlist(root, &args, providers, &config.package_provided)
    });

    // Real `create_cleanlist`'s prune branch prints `show_parents(pkg)`
    // inline while it builds the removal list -- before the removal-order
    // line / empty message.
    if verbose {
        for (pkg, lines) in &result.kept_parents {
            println!("  {} pulled in by:", pkg.cpv());
            for line in lines {
                println!("    {line}");
            }
            println!();
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
    // Real `action_depclean` (`action == "prune"`): `unmerge(...,
    // "unmerge", cleanlist, ordered=ordered)`. Without `--pretend`,
    // `run_unmerge_pretend` removes them.
    run_unmerge_pretend(RunUnmergePretendArgs {
        targets: &cpv_refs,
        action: "unmerge",
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
    /// Threaded into the post-removal info-regen block (real `--quiet`
    /// suppression + resolved-config `noinfo`); computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config -- threaded into the
    /// post-removal GLEP 42 count notice (backlog #196 fix round 1).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

/// Real `emerge --pretend --prune --nodeps` (`actions.py:2684-2697`):
/// `--nodeps` routes prune to `unmerge()`'s own `_unmerge_display` prune
/// branch (`unmerge.py:245-272`) *instead of* `_calc_depclean` -- so
/// there is NO dependency/reachability check at all, no `>>>
/// Calculating removal order...`, and no `show_parents` (`--verbose` is
/// inert here). For every cp with more than one version installed the
/// best (highest) version is `protected` and every other version is
/// `selected` for removal (see `portage_repo::prune_nodeps_selection`).
/// `_unmerge_display` renders it unordered: the header, per-cp
/// `selected:`/`protected:`/`omitted:` blocks (`omitted` is always
/// `none` here -- every version is either selected or protected), the
/// `sys-apps/portage` self-skip, the "still listed in package sets"
/// warning, and the footer. Empty result: `>>> No outdated packages
/// were found on your system.` with no args (real `global_unmerge`),
/// else `>>> No packages selected for removal by prune` -- both exit 1
/// (real `_unmerge_display` returns `(1, {})`, unlike plain `--prune`'s
/// exit 0). Without `--pretend`, `execute_unmerge` removes the selected
/// versions after the display -- real `actions.py:2684` routes `prune
/// --nodeps` through the very same `unmerge()` `-C` uses.
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
    /// Threaded into the post-removal info-regen block (real `--quiet`
    /// suppression + resolved-config `noinfo`); computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config -- threaded into the
    /// post-removal GLEP 42 count notice (backlog #196 fix round 1).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

/// Real `emerge --clean` (`action_uninstall` -> `unmerge` with
/// `unmerge_action == "clean"`): identical to `--prune --nodeps` except
/// the best/rest split is **per slot** (see
/// `portage_repo::clean_selection`), there is no `sys-apps/portage`
/// self-skip (real `unmerge.py:368`), and the empty-selection message
/// names `clean`. Individual ebuild-path args are rejected (real
/// `unmerge.py:131`); `resolve_cleanup_args` already only produces
/// `cat/pkg` atoms here.
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
    /// Threaded into the post-removal info-regen block (real `--quiet`
    /// suppression + resolved-config `noinfo`); computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config -- threaded into the
    /// post-removal GLEP 42 count notice (backlog #196 fix round 1).
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

    // Real `_unmerge_display` (`unmerge.py:195`): the header is
    // `--pretend`/`--ask`-only.
    if pretend {
        println!(
            "{}",
            color.c(
                "darkgreen",
                ">>> These are the packages that would be unmerged:"
            )
        );
    }

    // Real `sys-apps/portage` self-skip (`unmerge.py:368-391`): move any
    // selected `sys-apps/portage` version into `protected` with the
    // eerror. Realistically dead code -- `sys-apps/portage` is never
    // installed at more than one version -- kept for fidelity. Real
    // `unmerge.py:368`: `if unmerge_action != "clean"` -- `--clean` does
    // NOT self-skip.
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

    // "still listed in package sets" warning (real `_unmerge_display`,
    // `unmerge.py:393-447`) -- same machinery `run_unmerge_pretend` uses.
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
