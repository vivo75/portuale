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
