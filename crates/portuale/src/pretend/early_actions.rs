//! Option checks that end the run early, and the actions that need no resolve.

use crate::pretend::*;

pub(crate) fn validate_and_early_actions(opts: &mut EmergeOpts<'_>) -> Option<ExitCode> {
    // `--tree` + `--columns` is a usage error (exit 2, stderr), checked once
    // parsing finishes, order-independent, before any other validation.
    // Portage: actions.py, parse_opts post-pass
    if opts.tree && opts.columns {
        eprintln!("emerge: can't specify both of \"--tree\" and \"--columns\".");
        return Some(ExitCode::from(2));
    }

    // The `--exclude`-style option families validate before anything else
    // action-shaped runs (Portage `_emerge/main.py` option post-pass, exit
    // 2): `--buildpkg-exclude` first with its own message, then the nine
    // strict families in tuple order. See `validate_exclude_options`.
    if let Some(code) = validate_exclude_options(opts) {
        return Some(code);
    }

    let &mut EmergeOpts {
        pretend: ref mut pretend_out,
        ask: ref mut ask_out,
        sync_action,
        search_action,
        info_action,
        list_sets,
        check_news,
        regen_action,
        metadata_action,
        color_opt,
        debug,
        deselect,
        depclean,
        prune,
        unmerge,
        ref atom_args,
        jobs,
        load_average,
        config_action,
        resume,
        clean_action,
        rage_clean,
        ..
    } = opts;
    let mut pretend = *pretend_out;
    let mut ask = *ask_out;

    // `--pretend` silently drops `--ask`; then a remaining `--ask` on a
    // non-terminal stdin exits immediately -- nobody piped in could answer.
    // Portage: actions.py, option parsing post-pass
    if pretend {
        ask = false;
    }
    if ask && !stdin_is_tty() {
        eprintln!("!!! \"--ask\" should only be used in a terminal. Exiting.");
        return Some(ExitCode::from(1));
    }

    // `--sync` is not portuale's job; point at the tool that owns it.
    // Portage: actions.py (`emerge --sync` delegates to `emaint sync`)
    if sync_action {
        eprintln!("Functionality has moved to `emaint sync`.");
        return Some(ExitCode::from(1));
    }

    // Any action that writes into a root-owned filesystem needs privilege;
    // `--pretend` and the read-only query actions are exempt. `--regen`
    // rewrites the repo cache, not the root, so it is not gated here.
    // Portage: _emerge/actions.py, action_build privilege check
    let write_action = !pretend
        && !search_action
        && !info_action
        && !list_sets
        && !check_news
        && !regen_action
        && !metadata_action;
    if write_action && !crate::privileges::is_privileged(&root_from_env()) {
        // With `--ask`, offer to add `--pretend` instead of refusing; without
        // it, refuse on stderr with exit 1 (no hint line).
        // Portage: _emerge/actions.py, privilege check
        if ask {
            println!("This action requires superuser access...");
            match ask_yes_no(
                &Colorizer::new(color::resolve_havecolor(color_opt)),
                "Would you like to add --pretend to options?",
            ) {
                Some(true) => {
                    pretend = true;
                    ask = false;
                    portage_repo::set_resolver_debug(debug && pretend);
                }
                Some(false) => return Some(ExitCode::from(130)),
                None => return Some(ExitCode::from(130)),
            }
        } else {
            eprintln!("emerge: superuser access is required");
            return Some(ExitCode::from(1));
        }
    }

    // `config`, `metadata` and `regen` only ever do real work, so a dry run
    // is meaningless: they reject `--pretend` outright. (`--config` keeps
    // ignoring `--pretend` for compatibility instead.)
    // Portage: _emerge/actions.py, action dispatch
    if pretend {
        for (flag, name) in [(regen_action, "regen"), (metadata_action, "metadata")] {
            if flag {
                eprintln!("emerge: The '{name}' action does not support '--pretend'.");
                return Some(ExitCode::from(1));
            }
        }
    }

    // `--unmerge`/`--depclean`/`--prune`/`--deselect` without `--pretend`
    // really execute after the display; no `requires --pretend` gate left.

    // `--deselect` is a standalone action only when no other action claimed
    // the run; under `--depclean`/`--prune`/`--unmerge` it is just a modifier.
    // Portage: _emerge/main.py, action selection
    if deselect && !depclean && !prune && !unmerge {
        return Some(run_deselect(
            atom_args,
            &root_from_env(),
            pretend,
            ask,
            &Colorizer::new(color::resolve_havecolor(color_opt)),
        ));
    }

    // `--list-sets` needs nothing from the resolved config; dispatch before
    // the atom guard and the repos.conf/profile machinery.
    if list_sets {
        return Some(run_list_sets(&config_root_from_env()));
    }

    // `--regen` regenerates every repo's `metadata/md5-cache` by running each
    // ebuild's `depend` phase; `--jobs`/`--load-average` thread through.
    // Portage: _emerge/actions.py, action_regen
    if regen_action {
        return Some(crate::regen::run(
            &config_root_from_env(),
            &root_from_env(),
            debug,
            jobs,
            load_average,
        ));
    }
    // `--metadata`: portuale reads `metadata/md5-cache` directly and models
    // no `depcachedir`, so there is nothing to transfer -- just the header.
    // Portage: _emerge/actions.py, action_metadata
    if metadata_action {
        println!("\n>>> Updating Portage cache");
        return Some(ExitCode::SUCCESS);
    }

    if atom_args.is_empty()
        && !unmerge
        && !depclean
        && !prune
        && !config_action
        && !resume
        && !search_action
        && !check_news
        && !clean_action
        && !rage_clean
        && !info_action
    {
        eprintln!(
            "emerge: no targets given (an atom, an @set, or an installed file / \
             ebuild / tbz2 / gpkg; `emerge --help` for usage)"
        );
        return Some(ExitCode::from(2));
    }
    *pretend_out = pretend;
    *ask_out = ask;
    None
}
