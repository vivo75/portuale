//! The options every merge path shares: status mode, package and merge options, PORTAGE_TMPDIR.

use crate::ebuild_merge::MergeOptions;
use crate::ebuild_package::PackageOptions;
use crate::emerge_build::StatusMode;
use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// What [`merge_setup`] hands back to its caller.
pub(crate) struct MergeSetupOut {
    pub(crate) status_mode: StatusMode,
    pub(crate) package_options: PackageOptions,
    pub(crate) portage_tmpdir: PathBuf,
    pub(crate) merge_options: MergeOptions,
}

/// The options every merge path shares: status mode, package and merge options, PORTAGE_TMPDIR.
/// Also rotates the resume list to its backup, as Portage does before a merge.
pub(crate) fn merge_setup(
    opts: &EmergeOpts<'_>,
    entries: &[GraphEntry],
    config: &Config,
    root: &Path,
) -> MergeSetupOut {
    let &EmergeOpts {
        jobs,
        quiet,
        quiet_build,
        verbose,
        shell,
        debug,
        ..
    } = opts;
    // Backlog #185/#197: real `Scheduler._status_msg`
    // (`Scheduler.py:2387-2397`) precedes every status line -- the
    // first `>>> Emerging ...` among them -- with a blank line,
    // unless the scheduler is in background mode (real
    // `_background_mode`: parallel `--jobs`, `--quiet`, or
    // `--quiet-build=y`, except a one-package mergelist without
    // `--quiet` -- see `emerge_build::scheduler_status_mode`).
    // #185 ported only the first blank with a `jobs`/`quiet`
    // approximation; #197 generalizes the rule to every status
    // line, and the merge functions own every blank through their
    // `StatusDisplay::status()` -- including the first, so no
    // blank prints here (fix round 1 removed the duplicate that
    // printed a second blank before the first `>>>` line). The
    // exact background predicate still computes here.
    let status_mode = crate::emerge_build::scheduler_status_mode(
        jobs,
        quiet,
        quiet_build,
        entries.len(),
        verbose,
    );
    // Real BINPKG_COMPRESS/BINPKG_COMPRESS_FLAGS[_<NAME>]/
    // PORTAGE_BZIP2_COMMAND/PKGDIR/... resolution -- see
    // `package_options_from_env`.
    let package_options = package_options_from_env(
        shell,
        debug,
        Some(&config_features_string(config)),
        Some(config),
    );
    let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(config));
    let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
    // The compiler / make flags real portage puts in every build
    // phase's environment, from the resolved config (make.conf +
    // profile `make.defaults` + the `env` layer -- all folded into
    // `Config::other_vars`). `merge_one_source_entry` appends the
    // per-entry `USE` on top. Only the deterministic build-var set --
    // not every scalar (`PATH`/`HOME`/... are handled by
    // `phase_env_vars` itself), and `FEATURES` stays out (portuale
    // models it via its own `feature_enabled`, forcing `""` here).
    merge_options.build_env = crate::emerge_build::run_wide_phase_env(config);
    merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
    // Real `preinst_mask()`: `INSTALL_MASK` + the `no{man,info,doc}`
    // `FEATURES` fold, from the resolved config (not the process
    // env). Applied to every merged image before `CONTENTS` is
    // recorded -- see `ebuild_merge::apply_install_mask`.
    (
        merge_options.install_mask,
        merge_options.install_mask_prunes_usr_share,
    ) = config_install_mask(config);
    merge_options.set_resolved_features(&config_features_string(config));
    // #333: real `dblink` protects `settings["CONFIG_PROTECT"]`, the
    // resolved fold (calling env included), not the raw process env
    // `from_env` read (`MergeOptions::apply_config_protect`).
    merge_options.apply_config_protect(config);
    // Real `_grab_pkg_env` into `configdict["pkg"]`: a `package.env`
    // entry matching a build-bound package layers its env file's
    // build vars over the run-wide set above.
    // `emerge_build::entry_package_env_vars` does the per-entry atom
    // match.
    merge_options.package_env_vars = config.package_env_vars.clone();
    // Real `actions.py:664-672`: right before a fresh, non-`--resume`
    // merge starts, an existing multi-package `mtimedb["resume"]`
    // (left over from some earlier, abandoned run) is preserved as
    // `resume_backup` rather than just being silently overwritten
    // once this run's own outcome gets recorded -- `emerge --resume`
    // can still recover it later if this run wasn't actually meant
    // to replace it. This whole branch is only ever reached for a
    // non-`--resume` action (`--resume` returns early, well above).
    crate::mtimedb::rotate_resume_to_backup(root);
    MergeSetupOut {
        status_mode,
        package_options,
        portage_tmpdir,
        merge_options,
    }
}

/// The values [`show_nothing_to_merge`] reads from its caller.
pub(crate) struct ShowNothingToMergeArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) show_merge_list: bool,
    pub(crate) mergecount: usize,
    pub(crate) deferred: bool,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) selected_set_args: &'a [String],
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) color: Colorizer,
}

/// An empty plan: Portage's "Nothing to merge" notices and exit.
pub(crate) fn show_nothing_to_merge(args: ShowNothingToMergeArgs<'_>) -> Option<ExitCode> {
    let ShowNothingToMergeArgs {
        opts,
        show_merge_list,
        mergecount,
        deferred,
        root,
        config_root,
        selected_set_args,
        entries,
        system_atoms,
        repos,
        color,
    } = args;
    let &EmergeOpts {
        pretend,
        buildpkgonly,
        ref atom_args,
        ask,
        oneshot,
        onlydeps,
        ..
    } = opts;
    if nothing_to_merge(pretend, show_merge_list, mergecount) {
        if deferred {
            // Real `actions.py:526`: the blank line prints on the
            // prompt shape too (`prompt` stays `None` on this arm,
            // so `UserQuery` is skipped), then the run falls through
            // to the `resume_backup` rotation and
            // `saveNomergeFavorites` -- only `Scheduler` is skipped
            // (`:664-675`).
            println!();
            crate::mtimedb::rotate_resume_to_backup(root);
            // Backlog #232, second half: real
            // `depgraph.saveNomergeFavorites`
            // (`depgraph.py:11305-11402`) records the world-candidate
            // favorites that stayed unmerged. Its own suppression set
            // (`:11308-11317`) is `--buildpkgonly` / `--fetchonly` /
            // `--fetch-all-uri` / `--oneshot` / `--onlydeps` /
            // `--pretend`: `--pretend` never reaches this branch,
            // `--oneshot` / `--onlydeps` already keep the deferral
            // gate above at bay (real's folded variable,
            // `actions.py:257`), and `--fetchonly` / `--fetch-all-uri`
            // are portuale-unimplemented options (a usage error long
            // before this point), so only `--buildpkgonly` still
            // suppresses here. Under `--ask` the
            // prompt moves here (`:11376-11386`): the additions list
            // prints first, then `Would you like to add these
            // packages to your world favorites?`, and a "No" (or EOF,
            // which `ask_yes_no` folds into the same decline) records
            // nothing -- the run still succeeds, the rotation above
            // already happened.
            if !buildpkgonly {
                // Backlog #232 review (Important-2): real's record
                // skips non-candidate sets (`depgraph.py:11366`) --
                // the same `usersets_world_candidate` filter as the
                // gate above, so only the gate-passing sets record.
                let record_sets: &[String] = if usersets_world_candidate(config_root) {
                    selected_set_args
                } else {
                    &[]
                };
                let fav_refs: Vec<&str> = atom_args.to_vec();
                let mut additions = match read_world_atoms(root) {
                    Ok(current) => {
                        world_file_additions(&current, &fav_refs, entries, system_atoms, repos)
                    }
                    Err(e) => {
                        eprintln!("emerge: {e}");
                        return Some(ExitCode::from(1));
                    }
                };
                match read_world_sets(root) {
                    Ok(names) => {
                        let current: Vec<String> =
                            names.into_iter().map(|n| format!("@{n}")).collect();
                        additions.extend(world_sets_additions(&current, record_sets));
                    }
                    Err(e) => {
                        eprintln!("emerge: {e}");
                        return Some(ExitCode::from(1));
                    }
                }
                if ask && !additions.is_empty() {
                    println!();
                    let mut shown = additions.clone();
                    shown.sort();
                    shown.dedup();
                    for a in &shown {
                        println!(" {} {}", color.c("GOOD", "*"), a);
                    }
                    println!();
                    if ask_yes_no(
                        &color,
                        "Would you like to add these packages to your world favorites?",
                    ) != Some(true)
                    {
                        return Some(ExitCode::SUCCESS);
                    }
                }
                // The writers below re-read the world files and
                // recompute the same additions instead of reusing
                // `additions`: deliberate -- single-threaded, no
                // intervening write, so the recompute is identical,
                // and it keeps one writer entry point for both this
                // arm and the success path.
                if let Err(e) = update_world_file(UpdateWorldFileArgs {
                    root,
                    target_atoms: &fav_refs,
                    entries,
                    system_atoms,
                    repos,
                    oneshot,
                    onlydeps,
                }) {
                    eprintln!("emerge: {e}");
                    return Some(ExitCode::from(1));
                }
                if let Err(e) = update_world_sets_file(root, record_sets, oneshot, onlydeps) {
                    eprintln!("emerge: {e}");
                    return Some(ExitCode::from(1));
                }
            }
            return Some(ExitCode::SUCCESS);
        }
        // Real's own `print()` before the message
        // (`actions.py:518`): `--ask` reaches here with no blank
        // line printed yet (the #185 `actions.py:526` blank only
        // covers the no-prompt shape, and `ask_confirm`'s own
        // opener never runs).
        if ask {
            println!();
        }
        println!("Nothing to merge; quitting.");
        println!();
        return Some(ExitCode::SUCCESS);
    }
    None
}
