//! Option checks that end the run early, and the actions that need no resolve.

use crate::pretend::*;

/// The values [`validate_and_early_actions`] reads from its caller.
pub(crate) struct ValidateAndEarlyActionsArgs<'a> {
    pub(crate) tree: bool,
    pub(crate) columns: bool,
    pub(crate) pretend: &'a mut bool,
    pub(crate) ask: &'a mut bool,
    pub(crate) sync_action: bool,
    pub(crate) search_action: bool,
    pub(crate) info_action: bool,
    pub(crate) list_sets: bool,
    pub(crate) check_news: bool,
    pub(crate) regen_action: bool,
    pub(crate) metadata_action: bool,
    pub(crate) color_opt: Option<bool>,
    pub(crate) debug: bool,
    pub(crate) deselect: bool,
    pub(crate) depclean: bool,
    pub(crate) prune: bool,
    pub(crate) unmerge: bool,
    pub(crate) atom_args: &'a [&'a str],
    pub(crate) jobs: usize,
    pub(crate) load_average: Option<f64>,
    pub(crate) config_action: bool,
    pub(crate) resume: bool,
    pub(crate) clean_action: bool,
    pub(crate) rage_clean: bool,
}

/// Option checks that end the run early, and the actions that need no resolve.
pub(crate) fn validate_and_early_actions(
    args: ValidateAndEarlyActionsArgs<'_>,
) -> Option<ExitCode> {
    let ValidateAndEarlyActionsArgs {
        tree,
        columns,
        pretend: pretend_out,
        ask: ask_out,
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
        atom_args,
        jobs,
        load_average,
        config_action,
        resume,
        clean_action,
        rage_clean,
    } = args;
    let mut pretend = *pretend_out;
    let mut ask = *ask_out;
    // Real actions.py: "if '--tree' in emerge_config.opts and '--columns'
    // in emerge_config.opts: print(...); return 1" -- checked once
    // parsing finishes (order-independent: works whichever flag came
    // first in argv), right after option parsing and before any other
    // validation, matching real portage's own placement. Portuale's
    // own CLI-usage-error convention (see the contract suite's own doc
    // comment: exit 2, stderr) differs deliberately from real portage's
    // literal `return 1`/stdout here, matching every other CLI-usage
    // error portuale already reports (`--exclude` requires an
    // argument, an invalid `--deep` value, etc.) rather than real
    // portage's own inconsistent mix of exit codes for different
    // usage errors.
    if tree && columns {
        eprintln!("emerge: can't specify both of \"--tree\" and \"--columns\".");
        return Some(ExitCode::from(2));
    }

    // Real `actions.py:3917-3926`, checked in the same order right after
    // option parsing finishes: `-p` first silently removes `--ask` (a
    // `--pretend` run never does anything an actual confirmation would
    // gate), and only once that's settled is `--ask` checked against
    // `stdin.isatty()` -- so `--ask --pretend` on a non-terminal stdin
    // is fine (ask is already `false` by the time the isatty check
    // runs), but a bare `--ask` piped from a non-terminal is a hard,
    // immediate exit rather than a prompt nobody typing into a pipe
    // could ever answer.
    if pretend {
        ask = false;
    }
    if ask && !stdin_is_tty() {
        eprintln!("!!! \"--ask\" should only be used in a terminal. Exiting.");
        return Some(ExitCode::from(1));
    }

    // `--sync`: repo syncing will never be part of portuale -- point the
    // user at the tool that owns it (matching real portage's own
    // long-standing split: `emerge --sync` delegates to `emaint sync`).
    if sync_action {
        eprintln!("Functionality has moved to `emaint sync`.");
        return Some(ExitCode::from(1));
    }

    // Real `actions.py:3964-4012`: an action that writes into a
    // root-owned filesystem -- any real merge, plus `--unmerge`/-C,
    // `--depclean`/-c, `--prune`/-P, `--deselect`/-W (world-file
    // rewrite), `--config`, `--clean` -- requires `secpass == 2`
    // (`uid == 0`, or non-root ownership of the target root; see
    // `privileges::is_privileged`). `--pretend` and the read-only query
    // actions are exempt. `--regen` rewrites the repo cache, not the
    // root, so it is not gated here.
    let write_action = !pretend
        && !search_action
        && !info_action
        && !list_sets
        && !check_news
        && !regen_action
        && !metadata_action;
    if write_action && !crate::privileges::is_privileged(&root_from_env()) {
        // Real `_emerge/actions.py:3983-4006` (backlog #246): with
        // `--ask`, a non-privileged caller is offered `Would you like
        // to add --pretend to options?` instead of being refused. Real
        // prints `This action requires {access_desc} access...` to
        // stdout first; portuale has a single privilege tier (no
        // portage-group tier -- see `privileges.rs`), so `access_desc`
        // is always `superuser` here. The prompt itself goes through
        // the shared `UserQuery.query` port (`ask_yes_no`, backlog
        // #240), so "No" exits `128 + SIGINT` with nothing else
        // printed, and EOF/SIGINT prints `Interrupted.` and exits the
        // same way; "Yes" sets `--pretend` and drops `--ask`, and the
        // run continues as a pretend run. (Host probe 2026-09-29,
        // `/usr/sbin/emerge` 3.0.82.2 as uid 1000, secpass 0: exact
        // bytes in the pin docstrings below.)
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
            // Real `actions.py:4007-4012` without `--ask`: `emerge:
            // superuser access is required` on stderr, exit 1 -- and no
            // hint line (host probe 2026-09-29), so this no longer goes
            // through `privileges::deny_superuser` (kept for `ebuild`,
            // whose refusal is unprobed).
            eprintln!("emerge: superuser access is required");
            return Some(ExitCode::from(1));
        }
    }

    // Real `actions.py:4106-4111`: the `config`, `metadata` and `regen`
    // actions reject `--pretend` outright (they only ever do real work,
    // so a dry run is meaningless). `--config` already runs ignoring
    // `--pretend` in portuale -- kept as-is for compatibility; the two
    // others are gated here.
    if pretend {
        for (flag, name) in [(regen_action, "regen"), (metadata_action, "metadata")] {
            if flag {
                eprintln!("emerge: The '{name}' action does not support '--pretend'.");
                return Some(ExitCode::from(1));
            }
        }
    }

    // `--unmerge`/`-C`, `--depclean`/`-c`, `--prune`/`-P` and now
    // `--deselect`/`-W` WITHOUT `--pretend` are all real writes:
    // `run_unmerge_pretend` / `run_depclean_pretend` / `run_prune_pretend`
    // / the `--nodeps` variant each run `execute_unmerge` after the
    // display when `pretend` is `false`, and `run_deselect` rewrites the
    // `world` / `world_sets` files (real `action_deselect`'s own
    // `world_set.replace(remaining)`). No `requires --pretend` gate left.

    // Real non-dry-run execution paths portuale implements for `emerge`
    // itself: `--buildpkgonly` (builds a binary package, never merges --
    // `emerge_build.rs`), `--getbinpkgonly` (downloads remote binpkgs and
    // merges them -- `emerge_getbinpkg.rs`), and a plain `emerge <atom>`
    // (real source build + merge -- `emerge_build::run_source_merge`).
    // Dispatched from the `!pretend` block far below, after resolution.

    // Real `main.py`: `--deselect` becomes a standalone action only when
    // `myaction is None` -- `--depclean`/`--prune`/`--unmerge` each set
    // their own action first, and then `--deselect=y|n` is just a
    // modifier on it (real `action_depclean`'s `deselect` -- see
    // `run_depclean_pretend`).
    if deselect && !depclean && !prune && !unmerge {
        return Some(run_deselect(
            atom_args,
            &root_from_env(),
            pretend,
            ask,
            &Colorizer::new(color::resolve_havecolor(color_opt)),
        ));
    }

    // `--list-sets`: a standalone action needing nothing from the
    // resolved config -- dispatch before the "expected a package atom"
    // guard and all the repos.conf/profile machinery.
    if list_sets {
        return Some(run_list_sets(&config_root_from_env()));
    }

    // `--regen` (real `action_regen`): a real write action -- regenerate
    // every repo's `metadata/md5-cache` by running each ebuild's `depend`
    // phase. `--pretend` was already rejected above. `--jobs` /
    // `--load-average` thread straight through (real
    // `action_regen(settings, portdb, max_jobs, max_load)`).
    if regen_action {
        return Some(crate::regen::run(
            &config_root_from_env(),
            &root_from_env(),
            debug,
            jobs,
            load_average,
        ));
    }
    // `--metadata` (real `action_metadata`): transfers a repo's
    // pre-generated cache into portage's own `depcachedir`. Portuale
    // reads `metadata/md5-cache` directly and models no `depcachedir`, so
    // there is nothing to transfer -- real portage's own
    // `>>> Updating Portage cache` header, then done.
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
