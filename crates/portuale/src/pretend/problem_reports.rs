//! Missing arguments, and the abort reports for cycles and unsatisfied dependencies.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`show_deferred_unsat`] reads from its caller.
pub(crate) struct ShowDeferredUnsatArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) deferred_abort_unsat: &'a [usize],
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) columnwidth: i64,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
}

/// Unsatisfied dependencies whose report waited for the merge list.
pub(crate) fn show_deferred_unsat(args: ShowDeferredUnsatArgs<'_>) {
    let ShowDeferredUnsatArgs {
        plan,
        opts,
        deferred_abort_unsat,
        result,
        root,
        top_level_pkgs,
        columnwidth,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
    } = args;
    let ResolvePlan {
        root_deps_running_root,
        ..
    } = plan;
    let &EmergeOpts {
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        ..
    } = opts;
    if !deferred_abort_unsat.is_empty() {
        let mut thrown_away: Vec<String> = Vec::new();
        for &i in deferred_abort_unsat {
            print_entry_line(PrintEntryLineArgs {
                entries: &result.entries,
                root,
                index: i,
                indent: "",
                ordered: true,
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                running_root: root_deps_running_root.as_deref(),
                color,
                system_atoms,
                world_atoms,
                force_reinstall_cps,
                blocker_lines: &mut thrown_away,
                masked_deps: &result.masked_deps,
                use_unsat_deps: &result.use_unsat_deps,
                plain_miss_deps: &result.plain_miss_deps,
                force_sizes: false,
                circular: &result.circular_dependency,
            });
        }
    }
}

/// The values [`show_cycle_redisplay`] reads from its caller.
pub(crate) struct ShowCycleRedisplayArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) show_merge_list: bool,
    pub(crate) result: &'a GraphResult,
    pub(crate) skip_redisplay: bool,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) columnwidth: i64,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
}

/// The circular-dependency block shown again after the merge list.
pub(crate) fn show_cycle_redisplay(args: ShowCycleRedisplayArgs<'_>) {
    let ShowCycleRedisplayArgs {
        plan,
        opts,
        show_merge_list,
        result,
        skip_redisplay,
        display_entries,
        root,
        top_level_pkgs,
        columnwidth,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
    } = args;
    let ResolvePlan {
        root_deps_running_root,
        ..
    } = plan;
    let &EmergeOpts {
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        ..
    } = opts;
    if show_merge_list && !result.cycle_display.is_empty() && !skip_redisplay {
        println!();
        let mut thrown_away: Vec<String> = Vec::new();
        for cpv in &result.cycle_display {
            let Some((idx, _entry)) = display_entries.iter().enumerate().find(|(_, e)| {
                let ver = match &e.outcome {
                    portage_repo::PretendOutcome::New { version }
                    | portage_repo::PretendOutcome::Reinstall { version, .. } => version,
                    portage_repo::PretendOutcome::Upgrade { to, .. }
                    | portage_repo::PretendOutcome::Downgrade { to, .. } => to,
                    _ => return false,
                };
                format!("{}/{}-{ver}", e.category, e.package) == *cpv
            }) else {
                continue;
            };
            print_entry_line(PrintEntryLineArgs {
                entries: display_entries,
                root,
                index: idx,
                indent: "",
                ordered: true,
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                running_root: root_deps_running_root.as_deref(),
                color,
                system_atoms,
                world_atoms,
                force_reinstall_cps,
                blocker_lines: &mut thrown_away,
                masked_deps: &result.masked_deps,
                use_unsat_deps: &result.use_unsat_deps,
                plain_miss_deps: &result.plain_miss_deps,
                force_sizes: false,
                circular: &result.circular_dependency,
            });
        }
    }
}
