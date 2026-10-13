//! The standalone actions: --resume, --config, --search, --check-news, --info, --clean, --rage-clean, --unmerge, --depclean, --prune.

use crate::ebuild_phases::ShellBackend;
use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;

/// The values [`dispatch_standalone_actions`] reads from its caller.
pub(crate) struct DispatchStandaloneActionsArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) config: &'a mut Config,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) color: &'a Colorizer,
    pub(crate) repos: &'a [RepoConfig],
}

/// The standalone actions: --resume, --config, --search, --check-news, --info, --clean, --rage-clean, --unmerge, --depclean, --prune.
pub(crate) fn dispatch_standalone_actions(
    args: DispatchStandaloneActionsArgs<'_>,
) -> Option<ExitCode> {
    let DispatchStandaloneActionsArgs {
        opts,
        config,
        root,
        config_root,
        color,
        repos,
    } = args;
    let &EmergeOpts {
        resume,
        skipfirst,
        pretend,
        color_opt,
        jobs,
        load_average,
        shell,
        debug,
        quiet,
        ask,
        read_news,
        config_action,
        ref atom_args,
        search_action,
        searchdesc,
        fuzzy_search,
        regex_search_auto,
        search_similarity,
        check_news,
        info_action,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
        misspell_suggestions,
        clean_action,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
        ..
    } = opts;
    // `--resume` replays the saved `mtimedb["resume"]` mergelist. No atoms.
    // `--resume --pretend` prints the saved list and stops (see `run_resume`).
    // Portage: _emerge/actions.py
    if resume {
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(config));
        return Some(run_resume(RunResumeArgs {
            root,
            config_root,
            config,
            portage_tmpdir: &portage_tmpdir,
            skipfirst,
            pretend,
            color_opt,
            jobs,
            load_average,
            shell,
            debug,
            quiet,
            ask,
            read_news,
        }));
    }

    // `--config <atom>`: run `pkg_config` for one installed package.
    // Needs nothing from the resolved `config`; ignores `--pretend`.
    // Portage: _emerge/actions.py, action_config
    dispatch_queries(DispatchQueriesArgs {
        config_action,
        atom_args,
        root,
        shell,
        debug,
        ask,
        color,
        search_action,
        repos,
        searchdesc,
        quiet,
        fuzzy_search,
        regex_search_auto,
        search_similarity,
        check_news,
        config_root,
        config,
        info_action,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
        misspell_suggestions,
        clean_action,
        pretend,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    })
}

/// The values [`dispatch_queries`] reads from its caller.
pub(crate) struct DispatchQueriesArgs<'a> {
    pub(crate) config_action: bool,
    pub(crate) atom_args: &'a Vec<&'a str>,
    pub(crate) root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) debug: bool,
    pub(crate) ask: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) search_action: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) searchdesc: bool,
    pub(crate) quiet: bool,
    pub(crate) fuzzy_search: bool,
    pub(crate) regex_search_auto: bool,
    pub(crate) search_similarity: f64,
    pub(crate) check_news: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) info_action: bool,
    pub(crate) usepkg: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) getbinpkgonly: bool,
    pub(crate) misspell_suggestions: bool,
    pub(crate) clean_action: bool,
    pub(crate) pretend: bool,
    pub(crate) rage_clean: bool,
    pub(crate) unmerge: bool,
    pub(crate) depclean: bool,
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    pub(crate) deselect_n: bool,
    pub(crate) prune: bool,
    pub(crate) nodeps: bool,
}

/// The query actions: config, search, news and info, then the cleaning and removal actions.
pub(crate) fn dispatch_queries(args: DispatchQueriesArgs<'_>) -> Option<ExitCode> {
    let DispatchQueriesArgs {
        config_action,
        atom_args,
        root,
        shell,
        debug,
        ask,
        color,
        search_action,
        repos,
        searchdesc,
        quiet,
        fuzzy_search,
        regex_search_auto,
        search_similarity,
        check_news,
        config_root,
        config,
        info_action,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
        misspell_suggestions,
        clean_action,
        pretend,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    } = args;
    if config_action {
        return Some(run_config_action(RunConfigActionArgs {
            atom_args,
            root,
            shell,
            debug,
            ask,
            color,
        }));
    }

    // `--search`/`-s` (`--searchdesc`/`-S` also matches DESCRIPTION).
    // The full `Latest version` / `Homepage` / `Description` block shows
    // by default and `-q` makes it terse; `-v` is irrelevant to search.
    // Portage: _emerge/actions.py, action_search
    if search_action {
        return Some(run_search(RunSearchArgs {
            terms: atom_args,
            repos,
            root,
            searchdesc,
            verbose: !quiet,
            fuzzy: fuzzy_search,
            regex_auto: regex_search_auto,
            search_similarity,
            color,
        }));
    }

    // `--check-news`: output gated on `--quiet` not being given.
    // Portage: _emerge/actions.py, count_unread_news
    if check_news {
        return Some(run_check_news(RunCheckNewsArgs {
            repos,
            root,
            config_root,
            config,
            quiet,
            color,
        }));
    }

    // `--info`: narrowed to its deterministic config block plus the
    // per-atom "Package Settings" block.
    // Portage: _emerge/actions.py, action_info
    if info_action {
        let info_usepkg = usepkg || usepkgonly || getbinpkg || getbinpkgonly;
        // Portage consults the local `$PKGDIR` pool for its `(bindb,
        // "binary")` search half whenever `--usepkg` is on, so scan
        // `$PKGDIR` here too, exactly as the ordinary `--usepkg` resolve
        // path does further down (that scan runs after this dispatch,
        // so `--info` needs its own).
        if info_usepkg {
            match crate::binpkg::populate_local_pkgdir(
                Path::new(&config.pkgdir),
                pkgdir_index_trusted(config),
            ) {
                Ok(entries) if !entries.is_empty() => config.scanned_binpkgs = Some(entries),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("emerge: scanning {}: {e}", config.pkgdir);
                    return Some(ExitCode::from(1));
                }
            }
        }
        return Some(run_info(RunInfoArgs {
            config,
            repos,
            root,
            atom_args,
            misspell_suggestions,
            usepkg: info_usepkg,
            config_root,
            shell,
            debug,
            color,
        }));
    }

    // `--clean`: keep only the newest version per slot.
    dispatch_maintenance(DispatchMaintenanceArgs {
        clean_action,
        atom_args,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        config,
        repos,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    })
}

/// The values [`dispatch_maintenance`] reads from its caller.
pub(crate) struct DispatchMaintenanceArgs<'a> {
    pub(crate) clean_action: bool,
    pub(crate) atom_args: &'a Vec<&'a str>,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) quiet: bool,
    pub(crate) config: &'a mut Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) rage_clean: bool,
    pub(crate) unmerge: bool,
    pub(crate) depclean: bool,
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    pub(crate) deselect_n: bool,
    pub(crate) prune: bool,
    pub(crate) nodeps: bool,
}

/// The cleaning actions: clean and rage-clean, then the removal actions.
pub(crate) fn dispatch_maintenance(args: DispatchMaintenanceArgs<'_>) -> Option<ExitCode> {
    let DispatchMaintenanceArgs {
        clean_action,
        atom_args,
        root,
        config_root,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        config,
        repos,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    } = args;
    if clean_action {
        return Some(run_clean_pretend(RunCleanPretendArgs {
            targets: atom_args,
            root,
            config_root,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
            repos,
            config,
        }));
    }
    // `--rage-clean`: a fast `--unmerge` (identical `--pretend` display).
    if rage_clean {
        return Some(run_unmerge_pretend(RunUnmergePretendArgs {
            targets: atom_args,
            action: "rage-clean",
            root,
            config_root,
            config,
            repos,
            preserve_order: false,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
        }));
    }

    // `--unmerge`/`-C`: the `@system` target support and system-profile
    // check both need the resolved config, so dispatch before the
    // ordinary resolve-graph path below.
    dispatch_removal(DispatchRemovalArgs {
        unmerge,
        atom_args,
        root,
        config_root,
        config,
        repos,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    })
}

/// The values [`dispatch_removal`] reads from its caller.
pub(crate) struct DispatchRemovalArgs<'a> {
    pub(crate) unmerge: bool,
    pub(crate) atom_args: &'a Vec<&'a str>,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) quiet: bool,
    pub(crate) depclean: bool,
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    pub(crate) deselect_n: bool,
    pub(crate) prune: bool,
    pub(crate) nodeps: bool,
}

/// The removal actions: unmerge, depclean and prune.
pub(crate) fn dispatch_removal(args: DispatchRemovalArgs<'_>) -> Option<ExitCode> {
    let DispatchRemovalArgs {
        unmerge,
        atom_args,
        root,
        config_root,
        config,
        repos,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    } = args;
    if unmerge {
        // `pretend` last: without it, `run_unmerge_pretend` really
        // removes the selected packages after the display.
        return Some(run_unmerge_pretend(RunUnmergePretendArgs {
            targets: atom_args,
            action: "unmerge",
            root,
            config_root,
            config,
            repos,
            preserve_order: false,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
        }));
    }
    if depclean {
        return Some(run_depclean_pretend(RunDepcleanPretendArgs {
            targets: atom_args,
            root,
            config_root,
            config,
            repos,
            verbose,
            lib_check,
            deselect: !deselect_n,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
        }));
    }
    if prune {
        if nodeps {
            return Some(run_prune_nodeps_pretend(RunPruneNodepsPretendArgs {
                targets: atom_args,
                root,
                config_root,
                pretend,
                ask,
                shell,
                debug,
                color,
                quiet,
                noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
                repos,
                config,
            }));
        }
        return Some(run_prune_pretend(RunPrunePretendArgs {
            targets: atom_args,
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
            noinfo: config_features_list(config).iter().any(|t| t == "noinfo"),
        }));
    }
    None
}
