//! The applied-updates `AUTOCLEAN` force: Portage's `_global_updates`
//! over the target root, then the running root when different, before
//! the resolve. An applied run prints the `Performing Global Updates`
//! block, moves the installed entries, rewrites world, `package.*` and
//! the dep strings, records the updates mtimes and forces `AUTOCLEAN`
//! on the target root only.
//! Portage: lib/_emerge/actions.py, run_action

use crate::global_updates::{RunGlobalUpdatesArgs, run_global_updates};
use crate::pretend::*;

/// What [`global_updates_force`] needs: the CLI options (for the
/// skips), the repos, the target config (reloaded when its updates
/// applied), the config root for that reload, the target root, and the
/// stdout stream for the updates block.
pub(crate) struct GlobalUpdatesArgs<'a> {
    pub opts: &'a EmergeOpts<'a>,
    pub repos: &'a [RepoConfig],
    pub config: &'a mut portage_profile::Config,
    pub config_root: &'a Path,
    pub root: &'a Path,
    pub out: &'a mut dyn std::io::Write,
}

/// The target root's applied-updates force, for the merge gate. Runs for
/// every action except help/info/sync/version and `--package-moves=n`,
/// pre-resolve.
// Portage: lib/_emerge/actions.py, run_action
pub(crate) fn global_updates_force(args: GlobalUpdatesArgs<'_>) -> bool {
    if !args.opts.package_moves
        || args.opts.info_action
        || args.opts.sync_action
        || args.opts.version_action
    {
        return false;
    }
    updates_force_for_roots(UpdatesForRootsArgs {
        repos: args.repos,
        config: args.config,
        config_root: args.config_root,
        target: args.root,
        running: &portage_repo::running_root_from_env(),
        quiet: args.opts.quiet,
        out: args.out,
    })
}

/// What [`updates_force_for_roots`] needs: the repos, the target
/// config (reloaded when the target updates applied), the config root
/// for that reload, both roots, the `--quiet` flag and the stdout
/// stream.
pub(crate) struct UpdatesForRootsArgs<'a> {
    pub repos: &'a [RepoConfig],
    pub config: &'a mut portage_profile::Config,
    pub config_root: &'a Path,
    pub target: &'a Path,
    pub running: &'a Path,
    pub quiet: bool,
    pub out: &'a mut dyn std::io::Write,
}

/// The two-root core: the target root first, then the running root when
/// it differs. Only the target root's force is reported (the post-merge
/// tail reads the target settings), but both roots' records are
/// maintained. A failed pass is reported and reads as no force.
// Portage: lib/_emerge/actions.py, run_action
pub(crate) fn updates_force_for_roots(args: UpdatesForRootsArgs<'_>) -> bool {
    let UpdatesForRootsArgs {
        repos,
        config,
        config_root,
        target,
        running,
        quiet,
        out,
    } = args;
    // `secpass` is global: uid 0, or the caller owning the target root
    // in unprivileged mode, processes both roots.
    // Portage: lib/portage/data.py, _unprivileged_mode
    let privileged = crate::privileges::is_privileged(target);
    let sandboxed = std::env::var_os("SANDBOX_ACTIVE").is_some();
    let target_settings = crate::global_updates::settings_for(config);
    let target_applied = match run_global_updates(RunGlobalUpdatesArgs {
        root: target,
        repos,
        settings: &target_settings,
        privileged,
        sandboxed,
        quiet,
        out,
    }) {
        Ok(applied) => applied,
        Err(e) => {
            eprintln!("emerge: global updates: {e}");
            false
        }
    };
    if target_applied {
        // Portage reloads the whole config from scratch, so the rest of
        // the run sees the moved world, `package.*`, vdb and binpkgs.
        match resolve_config_for_root(config_root, repos, target) {
            Ok(reloaded) => *config = reloaded,
            Err(e) => eprintln!("emerge: global updates: {e}"),
        }
    }
    if running != target {
        let running_settings = resolve_config_for_root(running, repos, running)
            .ok()
            .map(|running_config| crate::global_updates::settings_for(&running_config))
            .unwrap_or_else(|| target_settings.clone());
        if let Err(e) = run_global_updates(RunGlobalUpdatesArgs {
            root: running,
            repos,
            settings: &running_settings,
            privileged,
            sandboxed,
            quiet,
            out,
        }) {
            eprintln!("emerge: global updates: {e}");
        }
    }
    target_applied
}
