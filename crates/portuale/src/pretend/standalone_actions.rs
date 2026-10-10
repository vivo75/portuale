//! The standalone actions: --resume, --config, --search, --check-news, --info, --clean, --rage-clean, --unmerge, --depclean, --prune.

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
    // `--resume` / `-r`: a standalone action -- replay the saved
    // `mtimedb["resume"]` mergelist (real `_emerge/actions.py`). No atoms.
    // `--resume --pretend` prints the saved list and stops (see
    // `run_resume`).
    if resume {
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(config));
        return Some(run_resume(
            root,
            config_root,
            config,
            &portage_tmpdir,
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
        ));
    }

    // `--config <atom>`: a standalone action -- run `pkg_config` for one
    // installed package (real `action_config`). Needs nothing from the
    // resolved `config`; ignores `--pretend`.
    if config_action {
        return Some(run_config_action(atom_args, root, shell, debug, ask, color));
    }

    // `--search`/`-s` (`--searchdesc`/`-S` also matches DESCRIPTION): a
    // standalone read-only query action (real `action_search`). Real
    // `action_search` passes `search`'s `verbose` as `"--quiet" not in
    // myopts` -- so the full `Latest version` / `Homepage` /
    // `Description` block shows by default and `-q` makes it terse; `-v`
    // is irrelevant to search.
    if search_action {
        return Some(run_search(
            atom_args,
            repos,
            root,
            searchdesc,
            !quiet,
            fuzzy_search,
            regex_search_auto,
            search_similarity,
            color,
        ));
    }

    // `--check-news`: a standalone read-only query action (real
    // `actions.py`'s `count_unread_news` block). Real gates its output on
    // `"--quiet" not in myopts`.
    if check_news {
        return Some(run_check_news(
            repos,
            root,
            config_root,
            config,
            quiet,
            color,
        ));
    }

    // `--info`: a standalone read-only query action (real
    // `action_info`), narrowed to its deterministic config block plus
    // the per-atom "Package Settings" block.
    if info_action {
        let info_usepkg = usepkg || usepkgonly || getbinpkg || getbinpkgonly;
        // Real `action_info` consults `bindb` (the local `$PKGDIR` pool)
        // for its `(bindb, "binary")` search half whenever `--usepkg` is
        // on -- so scan `$PKGDIR` here too, exactly as the ordinary
        // `--usepkg` resolve path does further down (that scan runs after
        // this standalone-action dispatch, so `--info` needs its own).
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
        return Some(run_info(
            config,
            repos,
            root,
            atom_args,
            misspell_suggestions,
            info_usepkg,
            config_root,
            shell,
            debug,
            color,
        ));
    }

    // `--clean`: a standalone removal action -- keep only the newest
    // version per slot.
    if clean_action {
        return Some(run_clean_pretend(
            atom_args,
            root,
            config_root,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            config_features_list(config).iter().any(|t| t == "noinfo"),
            repos,
            config,
        ));
    }
    // `--rage-clean`: a fast `--unmerge` (identical `--pretend` display).
    if rage_clean {
        return Some(run_unmerge_pretend(
            atom_args,
            "rage-clean",
            root,
            config_root,
            config,
            repos,
            false,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            config_features_list(config).iter().any(|t| t == "noinfo"),
        ));
    }

    // `--unmerge`/`-C`: a standalone action -- resolved config in hand
    // (its `@system` target support and system-profile check both need
    // it), dispatch before the ordinary resolve-graph path below.
    if unmerge {
        // `pretend` last: without it, `run_unmerge_pretend` really
        // removes the selected packages after the display.
        return Some(run_unmerge_pretend(
            atom_args,
            "unmerge",
            root,
            config_root,
            config,
            repos,
            false,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            config_features_list(config).iter().any(|t| t == "noinfo"),
        ));
    }
    if depclean {
        return Some(run_depclean_pretend(
            atom_args,
            root,
            config_root,
            config,
            repos,
            verbose,
            lib_check,
            !deselect_n,
            pretend,
            ask,
            shell,
            debug,
            color,
            quiet,
            config_features_list(config).iter().any(|t| t == "noinfo"),
        ));
    }
    if prune {
        if nodeps {
            return Some(run_prune_nodeps_pretend(
                atom_args,
                root,
                config_root,
                pretend,
                ask,
                shell,
                debug,
                color,
                quiet,
                config_features_list(config).iter().any(|t| t == "noinfo"),
                repos,
                config,
            ));
        }
        return Some(run_prune_pretend(
            atom_args,
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
            config_features_list(config).iter().any(|t| t == "noinfo"),
        ));
    }
    None
}
