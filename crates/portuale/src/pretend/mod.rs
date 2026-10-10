//! `emerge --pretend`: resolve against every repos.conf repo and display, never
//! build. USE/ACCEPT_KEYWORDS/mask data come from the profile chain + make.conf
//! (see portage-profile), not a stand-in. DEPEND+RDEPEND recurse (diamond-safe;
//! multiple slots coexist); blockers and slot conflicts are reported, not enforced.
//! Output is a simplified subset of Portage's `--pretend` output, not byte-identical.
//! Every Portage emerge option/action is recognized by name; unimplemented ones fail
//! with an accurate message, unknown ones with "unrecognized option".
//!
//! Flag notes below; full Portage groundings live in the code-notes file.
//
//! `--update` keeps an installed version that still satisfies the atom;
//! `--deep` (`-D`, optional value) cuts recursion into installed packages'
//! dependencies; `--exclude` (`-X`, repeatable, space-separated lists) pins
//! matching packages as-is. `--deselect` (`-W`, optional value) is its own
//! standalone action, previewed under `--pretend`.
//! `--json` is portuale-only (Portage has no such mode): the whole graph as one
//! JSON line. `--verbose` (`-v`, optional value) and short-flag bundling (`-pv`)
//! follow Portage's value-taking rules; `--help` wins wherever it appears.

use crate::color::{self, Colorizer};

mod args;
mod atoms;
mod autounmask_report;
mod config_action;
mod conflicts;
mod deselect;
mod display;
mod early_actions;
mod env_config;
mod expand_targets;
mod final_gates;
mod gates;
mod help;
mod info;
mod json;
mod merge_list;
mod merge_run;
mod merge_setup;
mod news;
mod post_merge;
mod problem_reports;
mod process_globals;
mod prompt;
mod removal;
mod resolve_phases;
mod resolve_plan;
mod resume;
mod search;
mod standalone_actions;
mod world;

#[cfg(test)]
mod tests;

pub(crate) use args::*;
pub(crate) use atoms::*;
pub(crate) use autounmask_report::*;
pub(crate) use config_action::*;
pub(crate) use conflicts::*;
pub(crate) use deselect::*;
pub(crate) use display::*;
pub(crate) use early_actions::*;
pub(crate) use env_config::*;
pub(crate) use expand_targets::*;
pub(crate) use final_gates::*;
pub(crate) use gates::*;
pub(crate) use help::*;
pub(crate) use info::*;
pub(crate) use json::*;
pub(crate) use merge_list::*;
pub(crate) use merge_run::*;
pub(crate) use merge_setup::*;
pub use news::*;
pub(crate) use post_merge::*;
pub(crate) use problem_reports::*;
pub(crate) use process_globals::*;
pub(crate) use prompt::*;
pub(crate) use removal::*;
pub(crate) use resolve_phases::*;
pub(crate) use resolve_plan::*;
pub(crate) use resume::*;
pub(crate) use search::*;
pub(crate) use standalone_actions::*;
pub(crate) use world::*;

use crate::difflib;

use crate::ebuild_merge;

use crate::ebuild_package;

use crate::ebuild_phases;

use crate::emerge_build;

use crate::emerge_getbinpkg;

use crate::emerge_options;

use crate::needed_elf;

use portage_dep::{
    Atom, Blocker, Operator, SlotOperator, match_from_list, parse_atom, use_mismatch_flags,
};

use portage_repo::{
    BlockerConflict, BlockerSatisfiedBy, ChangedDepsReportEntry, GraphEntry, PretendOutcome,
    ResolveRequest, SlotConflict, active_resolver_for, all_installed_packages,
    config_root_from_env, ebuild_visible_at, root_from_env, slot_conflict_flag_sets, split_pf,
};

use std::collections::{BTreeMap, HashMap, HashSet};

use std::path::Path;

use std::process::ExitCode;

/// Reports and returns the exit code for one option/action token that is not
/// otherwise handled -- shared by standalone tokens and short-flag bundle
/// characters, so both produce identical messages.
fn report_option(token: &str) -> ExitCode {
    if let Some(found) = emerge_options::lookup(token) {
        // Reports and exits immediately, so this option's own value token needs
        // no skipping: nothing after this point is ever looked at.
        let kind = if found.category == emerge_options::Category::Action {
            "action"
        } else {
            "option"
        };
        eprintln!(
            "emerge: {kind} {:?} is a Portage emerge {kind}, but is not yet \
             implemented in portuale -- run \"emerge --help\" for the options \
             and actions that are.",
            found.canonical
        );
    } else {
        eprintln!("emerge: unrecognized option {token:?}");
    }
    ExitCode::from(2)
}

/// The `--backtrack` budget when the flag is absent: Portage's own default
/// (`0` only under `--nodeps`). Single source for the initializer and `--help`.
// Portage: _emerge/depgraph.py, _backtrack_depgraph
const DEFAULT_BACKTRACK_MAX: u32 = 20;

pub fn run(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    // Per-invocation process-wide state resets, so an in-process caller cannot
    // leak them across runs.
    ASK_ENTER_INVALID.store(false, std::sync::atomic::Ordering::Relaxed);
    portage_repo::clear_remote_binary_index_overrides();

    // Config resolution comes before argv parsing, matching Portage's own order
    // (find `--config-root`, load the config, re-parse with `EMERGE_DEFAULT_OPTS`
    // prepended). Portuale has no `--config-root` CLI, so the passes collapse
    // into one. `--help` still short-circuits before any config load.
    // Portage: _emerge/main.py
    let root = root_from_env();
    let config_root = config_root_from_env();

    // The repos + resolved config for the resolve below (and `@system` expansion).
    let (repos, mut config) = match load_repos_and_config(&config_root, &root) {
        Ok(loaded) => loaded,
        Err(message) => {
            eprintln!("emerge: {message}");
            return ExitCode::from(1);
        }
    };

    // `EMERGE_DEFAULT_OPTS` prepends to argv so argv wins for last-wins options.
    // Portage: _emerge/main.py
    let effective_args = args_with_emerge_defaults(&config, args);
    let args: &[String] = &effective_args;

    let mut opts = match parse_emerge_args(args) {
        Ok(opts) => opts,
        Err(code) => return code,
    };

    apply_process_globals(&opts);

    if let Some(code) = validate_and_early_actions(&mut opts) {
        return code;
    }

    // Colour gate, resolved once so every action path shares one `Colorizer`.
    // Portage: _emerge/actions.py, adjust_configs
    let color = Colorizer::new(color::resolve_havecolor(opts.color_opt));

    // Renice / ionice this process so every build subprocess inherits the policy.
    // Portage: _emerge/actions.py, apply_priorities
    apply_portage_scheduling_policy();

    if let Some(code) = dispatch_standalone_actions(DispatchStandaloneActionsArgs {
        opts: &opts,
        config: &mut config,
        root: &root,
        config_root: &config_root,
        color: &color,
        repos: &repos,
    }) {
        return code;
    }
    let ExpandTargetsOut {
        expanded_atoms,
        selected_set_args,
        set_args,
    } = match expand_targets(ExpandTargetsArgs {
        opts: &opts,
        color: &color,
        config: &config,
        root: &root,
        config_root: &config_root,
        repos: &repos,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };

    let plan = match plan_resolve(PlanResolveArgs {
        opts: &mut opts,
        config: &mut config,
        root: &root,
        repos: &repos,
        config_root: &config_root,
        color: &color,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let result = match run_resolve_phases(RunResolvePhasesArgs {
        plan: &plan,
        opts: &opts,
        config: &config,
        config_root: &config_root,
        root: &root,
        expanded_atoms: &expanded_atoms,
        set_args: &set_args,
        repos: &repos,
        color: &color,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let entries = &result.entries;
    // An aborted resolve renders the outcome's partial list instead of the full
    // entries (masked/unsat shapes show no list; the cycle shape shows the stuck
    // remainder). Everything else keeps the full graph; with the gate off the
    // legacy full list renders.
    // Portage: _emerge/actions.py, action_build
    let gated_abort_partial: Option<&Vec<portage_repo::GraphEntry>> =
        if portage_repo::abort_path_enabled() {
            match &result.outcome {
                portage_repo::ResolveOutcome::Aborted { partial, .. } => Some(partial),
                portage_repo::ResolveOutcome::Complete => None,
            }
        } else {
            None
        };
    let display_entries: &[portage_repo::GraphEntry] =
        gated_abort_partial.map_or(entries.as_slice(), Vec::as_slice);
    // `Total:` prints only when the merge list is displayed at all.
    let display_list_suppressed = gated_abort_partial.is_some() && display_entries.is_empty();

    // `--autounmask-only` skips the normal merge list and exits 0 after the
    // problem blocks -- but re-shows the list when it has changes to report
    // (see `autounmask_only_reshows_merge_list`). Also forces the dry-run path.
    // A non-`--pretend` merge shows the list only with `--ask`/`--tree`/`--verbose`
    // (see `merge_list_shown`); an aborted non-`--pretend` resolve shows no list.
    // Portage: _emerge/actions.py, action_build; depgraph.py, _display_autounmask
    let has_autounmask_changes = !result.autounmask_keyword_changes.is_empty()
        || !result.autounmask_mask_changes.is_empty()
        || !result.autounmask_use_changes.is_empty()
        || !result.autounmask_license_changes.is_empty();
    let show_merge_list = (!opts.autounmask_only
        && merge_list_shown(MergeListShownArgs {
            pretend: opts.pretend,
            ask: opts.ask,
            tree: opts.tree,
            verbose: opts.verbose,
            quiet: opts.quiet,
        })
        && (opts.pretend || gated_abort_partial.is_none()))
        || autounmask_only_reshows_merge_list(AutounmaskOnlyReshowsMergeListArgs {
            pretend: opts.pretend,
            autounmask_only: opts.autounmask_only,
            has_changes: has_autounmask_changes,
        });
    show_resolution_header(&opts, &result);

    show_pprovided_notice(&result, &color);

    // Directly-requested (category, package) pairs: a top-level AlreadyInstalled
    // gets its own "nothing to do" line, unlike a dependency-level one.
    let top_level_pkgs: HashSet<(String, String)> = expanded_atoms
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    if let Some(code) = print_json_result(PrintJsonResultArgs {
        plan: &plan,
        opts: &opts,
        display_entries,
        result: &result,
        top_level_pkgs: &top_level_pkgs,
        has_autounmask_changes,
        gated_abort_partial,
    }) {
        return code;
    }

    // COLUMNWIDTH resolves unconditionally, even without `--columns`.
    let columnwidth = columnwidth_from_env();
    // `Display.pkgprint`'s `@system`/world inputs: `@system` is the profile set;
    // world is `var/lib/portage/world` (missing file is a valid empty world).
    let world_atoms: Vec<String> = read_world_atoms(&root)
        .unwrap_or_default()
        .iter()
        .map(|a| portage_repo::apply_updates_to_atom(a))
        .collect();
    let system_atoms = &config.system_packages;
    // The red `r` bracket column: every `(cat, pkg)` on either side of an
    // ABI-rebuild edge. A provider with nothing installed in its own slot stays
    // `N` (a pure install that still caused a rebuild); other-slot installs do
    // not count (`NS`, no `r`).
    // Portage: resolver/output.py, PkgAttrDisplay
    let force_reinstall_cps: HashSet<(String, String)> = result
        .abi_rebuilds
        .iter()
        .flat_map(|(provider, consumer)| [(provider, true), (consumer, false)])
        .filter_map(|(cpv, is_provider)| {
            let (c, p, v) = portage_repo::split_cpv(cpv)?;
            if is_provider {
                let slot = result
                    .entries
                    .iter()
                    .find(|e| {
                        e.category == c
                            && e.package == p
                            && match &e.outcome {
                                portage_repo::PretendOutcome::New { version }
                                | portage_repo::PretendOutcome::Reinstall { version, .. } => {
                                    *version == v
                                }
                                portage_repo::PretendOutcome::Upgrade { to, .. }
                                | portage_repo::PretendOutcome::Downgrade { to, .. } => *to == v,
                                _ => false,
                            }
                    })
                    .and_then(|e| e.slot.clone());
                let in_slot = portage_repo::installed_candidates(&root, &c, &p)
                    .iter()
                    .any(|(_, s, _)| slot.as_ref().is_none_or(|want| s == want));
                if !in_slot {
                    return None;
                }
            }
            Some((c, p))
        })
        .collect();
    // Blocker lines collect while walking and print as one group afterwards.
    let mut blocker_lines: Vec<String> = Vec::new();
    // Abort-path unsatisfied lines held back until after the slot-conflict block.
    // Portage: _emerge/depgraph.py, display_problems
    let mut deferred_abort_unsat: Vec<usize> = Vec::new();
    // A circular abort renders the merge list with forced `--verbose --tree`
    // (quiet popped); the legacy path keeps the user-flagged list plus its own
    // re-display below.
    // Portage: _emerge/depgraph.py, _show_circular_deps
    let circular_forced_display = gated_abort_partial.is_some()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted {
                reason: portage_repo::AbortReason::UnserializableCycle { .. },
                ..
            }
        );
    let (disp_tree, disp_verbose, disp_quiet) = if circular_forced_display {
        (true, true, false)
    } else {
        (opts.tree, opts.verbose, opts.quiet)
    };
    // The rendered tree rows, for the circular `Total:` recount only.
    let mut rendered_rows: Vec<(usize, bool)> = Vec::new();
    render_merge_list(RenderMergeListArgs {
        plan: &plan,
        opts: &opts,
        show_merge_list,
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms: &expanded_atoms,
        config: &config,
        root: &root,
        repos: &repos,
        result: &result,
        rendered_rows: &mut rendered_rows,
        top_level_pkgs: &top_level_pkgs,
        disp_verbose,
        disp_quiet,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
        blocker_lines: &mut blocker_lines,
        columnwidth,
        gated_abort_partial,
        deferred_abort_unsat: &mut deferred_abort_unsat,
    });

    show_merge_list_summary(ShowMergeListSummaryArgs {
        opts: &opts,
        circular_forced_display,
        rendered_rows: &rendered_rows,
        display_entries,
        show_merge_list,
        disp_verbose,
        disp_quiet,
        display_list_suppressed,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        color: &color,
        result: &result,
        disp_tree,
    });

    // The cycle-only re-display between the merge list and the error block:
    // `result.cycle_display` as flat merge lines (no tree nesting or `[nomerge]`
    // marking -- portuale's tree model never abandons the list). Skipped when the
    // abort path already rendered the remainder as the merge list itself.
    // Portage: _emerge/depgraph.py, _show_circular_deps
    let skip_redisplay = gated_abort_partial.is_some()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted {
                reason: portage_repo::AbortReason::UnserializableCycle { .. },
                ..
            }
        );
    show_cycle_redisplay(ShowCycleRedisplayArgs {
        plan: &plan,
        opts: &opts,
        show_merge_list,
        result: &result,
        skip_redisplay,
        display_entries,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        columnwidth,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
    });

    show_slot_conflicts(ShowSlotConflictsArgs {
        plan: &plan,
        opts: &opts,
        result: &result,
        root: &root,
        repos: &repos,
        config: &config,
        useoldpkg_atoms_render: &opts.useoldpkg_atoms,
        color: &color,
    });
    show_deferred_unsat(ShowDeferredUnsatArgs {
        plan: &plan,
        opts: &opts,
        deferred_abort_unsat: &deferred_abort_unsat,
        result: &result,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        columnwidth,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
    });

    show_skipped_updates(&plan, &opts, &result, &root, &repos);

    show_skipped_missing_deps(opts.quiet, opts.debug, &result);

    show_missing_args_and_aborts(&result, &root, &color, &repos, &config);

    if let Some(code) = show_autounmask(ShowAutounmaskArgs {
        opts: &opts,
        color: &color,
        result: &result,
        repos: &repos,
        root: &root,
        config: &config,
        has_autounmask_changes,
    }) {
        return code;
    }

    if let Some(code) = failure_gates(FailureGatesArgs {
        opts: &opts,
        result: &result,
        show_merge_list,
        color: &color,
        repos: &repos,
        root: &root,
        config: &config,
    }) {
        return code;
    }

    print_resolver_debug(&root, entries, &opts.reinstall_atoms, &result);

    if let Some(code) = final_gates(FinalGatesArgs {
        opts: &opts,
        result: &result,
        entries,
        root: &root,
        repos: &repos,
        config: &config,
        color: &color,
    }) {
        return code;
    }

    // Execution below is non-`--pretend` only; see `emerge_build` for what builds.
    if !opts.pretend {
        // An empty displayed plan quits here (before the rotation and the
        // scheduler), after every failure gate above. `--resume` never reaches
        // here. The selective deferral still rotates and records world favorites.
        // Portage: _emerge/actions.py, action_build
        let mergecount = display_entries
            .iter()
            .filter_map(emerge_build::resume_cpv)
            .count();
        // Selective-without-`--oneshot` world candidates: plain favorites and
        // user `@set`s (built-ins never count). `--onlydeps` counts like
        // `--oneshot` here. See `selective_noop_deferred`.
        let deferred = selective_noop_deferred(SelectiveNoopDeferredArgs {
            selective: plan.selective,
            oneshot_or_onlydeps: opts.oneshot || opts.onlydeps,
            has_world_candidates: opts.atom_args.iter().any(|a| !a.starts_with('@'))
                || (usersets_world_candidate(&config_root) && !selected_set_args.is_empty()),
        });
        if let Some(code) = show_nothing_to_merge(ShowNothingToMergeArgs {
            opts: &opts,
            show_merge_list,
            mergecount,
            deferred,
            root: &root,
            config_root: &config_root,
            selected_set_args: &selected_set_args,
            entries,
            system_atoms,
            repos: &repos,
            color,
        }) {
            return code;
        }
        // `--ask` prompts once, after the whole merge list, before anything builds.
        // Portage: _emerge/actions.py, action_build
        if opts.ask && !ask_confirm(&color, "Would you like to merge these packages?") {
            return ExitCode::from(130);
        }
        let MergeSetupOut {
            status_mode,
            package_options,
            portage_tmpdir,
            merge_options,
        } = merge_setup(&opts, entries, &config, &root);

        // With `FEATURES=buildpkg` / `--buildpkg`, a binpkg of each source entry
        // is written into `$PKGDIR` as a side effect of the merge.
        let buildpkg_on = buildpkg_from_config(opts.buildpkg_opt, &config);
        let buildpkg = buildpkg_on.then_some(&package_options);
        // `emerge -k`: the resolver may have put a local-`$PKGDIR` binary on the
        // plan, so the per-entry binary-vs-source executor runs here too, not just
        // under `getbinpkg`. A pure local merge stays offline (binhost touched
        // only when the file is missing).
        let has_binary_entry = entries
            .iter()
            .any(|e| e.source == portage_repo::CandidateSource::Binary);
        if opts.buildpkgonly {
            if let Err(e) = emerge_build::run_buildpkgonly(emerge_build::RunBuildpkgonlyArgs {
                entries,
                config: &config,
                repos: &repos,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                options: &package_options,
                keep_going: opts.keep_going,
                mode: status_mode,
            }) {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        } else if plan.getbinpkg || has_binary_entry {
            if let Some(code) = run_binary_merge_plan(RunBinaryMergePlanArgs {
                opts: &opts,
                entries,
                config: &config,
                repos: &repos,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                package_options: &package_options,
                merge_options: &merge_options,
                buildpkg,
                status_mode,
                color,
            }) {
                return code;
            }
        } else if let Some(code) = run_source_merge_plan(RunSourceMergePlanArgs {
            opts: &opts,
            entries,
            root: &root,
            repos: &repos,
            portage_tmpdir: &portage_tmpdir,
            merge_options: &merge_options,
            buildpkg,
            status_mode,
            config: &config,
            color,
        }) {
            return code;
        }

        if let Some(code) = record_world_after_merge(RecordWorldAfterMergeArgs {
            opts: &opts,
            root: &root,
            entries,
            system_atoms,
            repos: &repos,
            config_root: &config_root,
            selected_set_args: &selected_set_args,
        }) {
            return code;
        }

        if let Some(code) = show_post_merge_notices(ShowPostMergeNoticesArgs {
            opts: &opts,
            entries,
            config: &config,
            root: &root,
            color,
            repos: &repos,
        }) {
            return code;
        }
    }

    // A `--pretend` run prints the GLEP 42 notice at the end (the vdb never
    // changes under `--pretend`). Portage: post_emerge.py
    pretend_end_news_notice(opts.pretend, &repos, &root, &config, &color);

    ExitCode::SUCCESS
}
