//! The counts and summary lines under the merge list, and the redisplay before a merge.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`show_merge_list_summary`] reads from its caller.
pub(crate) struct ShowMergeListSummaryArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) circular_forced_display: bool,
    pub(crate) rendered_rows: &'a [(usize, bool)],
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) show_merge_list: bool,
    pub(crate) disp_verbose: bool,
    pub(crate) disp_quiet: bool,
    pub(crate) display_list_suppressed: bool,
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) color: &'a Colorizer,
    pub(crate) result: &'a GraphResult,
    pub(crate) disp_tree: bool,
}

/// The counts and summary lines under the merge list, and the redisplay before a merge.
pub(crate) fn show_merge_list_summary(args: ShowMergeListSummaryArgs<'_>) {
    let ShowMergeListSummaryArgs {
        opts,
        circular_forced_display,
        rendered_rows,
        display_entries,
        show_merge_list,
        disp_verbose,
        disp_quiet,
        display_list_suppressed,
        root,
        top_level_pkgs,
        color,
        result,
        disp_tree,
    } = args;
    let &EmergeOpts {
        onlydeps,
        pretend,
        ask,
        ..
    } = opts;
    // The `Total: …` counters line: only under `-v` (never with `-q`), after
    // every entry line. The circular display runs forced-verbose, so the gate
    // reads the effective flags and counts rendered rows, not entries: repeats
    // count again, `[nomerge]` rows never count.
    // Portage: output.py, display
    let disp_count_entries: Option<Vec<GraphEntry>> = if circular_forced_display {
        let mut counted: Vec<GraphEntry> = Vec::new();
        for (i, ordered) in rendered_rows {
            if !ordered {
                continue;
            }
            let e = &display_entries[*i];
            match &e.outcome {
                portage_repo::PretendOutcome::New { .. }
                | portage_repo::PretendOutcome::Upgrade { .. }
                | portage_repo::PretendOutcome::Downgrade { .. }
                | portage_repo::PretendOutcome::Reinstall { .. }
                | portage_repo::PretendOutcome::Uninstall { .. } => {
                    counted.push(e.clone());
                }
                portage_repo::PretendOutcome::AlreadyInstalled { .. }
                | portage_repo::PretendOutcome::NoVisibleCandidate => {}
            }
        }
        Some(counted)
    } else {
        None
    };
    if show_merge_list && disp_verbose && !disp_quiet && !display_list_suppressed {
        println!();
        println!(
            "{}",
            package_counters_summary(PackageCountersSummaryArgs {
                entries: disp_count_entries.as_deref().unwrap_or(display_entries),
                root,
                top_level_pkgs,
                onlydeps,
                color,
                orphans: &result.orphan_blockers,
                tree: disp_tree,
                circular: &result.circular_dependency
            })
        );
    }

    // One blank line after the merge list when it printed on a non-`--pretend`
    // run without `--ask` (`ask_confirm` already opens with that blank line).
    // Portage: _emerge/actions.py, action_build
    if !pretend && show_merge_list && !ask {
        println!();
    }
}

/// The values [`print_json_result`] reads from its caller.
pub(crate) struct PrintJsonResultArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) result: &'a GraphResult,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) has_autounmask_changes: bool,
    pub(crate) gated_abort_partial: Option<&'a Vec<GraphEntry>>,
}

/// The `--json` output, which replaces the merge list.
pub(crate) fn print_json_result(args: PrintJsonResultArgs<'_>) -> Option<ExitCode> {
    let PrintJsonResultArgs {
        plan,
        opts,
        display_entries,
        result,
        top_level_pkgs,
        has_autounmask_changes,
        gated_abort_partial,
    } = args;
    let ResolvePlan {
        root_deps_running_root,
        ..
    } = plan;
    let &EmergeOpts {
        json,
        verbose,
        autounmask_only,
        pretend,
        autounmask_continue,
        backtrack_max,
        buildpkgonly,
        nodeps,
        ..
    } = opts;
    if json {
        print_json(PrintJsonArgs {
            entries: display_entries,
            backtrack_restarts: result.backtrack_restarts,
            backtrack_max: result.backtrack_max,
            slot_conflicts: &result.slot_conflicts,
            changed_deps_report: &result.changed_deps_report,
            autounmask_keyword_changes: &result.autounmask_keyword_changes,
            autounmask_use_changes: &result.autounmask_use_changes,
            autounmask_license_changes: &result.autounmask_license_changes,
            autounmask_mask_changes: &result.autounmask_mask_changes,
            abi_rebuilds: &result.abi_rebuilds,
            outcome: &result.outcome,
            top_level_pkgs,
            verbose,
            running_root: root_deps_running_root.as_deref(),
        });
        // Same non-zero-on-autounmask-changes contract as the text path: a
        // `--json` consumer's exit-code check must see the "config still needs
        // editing" signal too. `--autounmask-only` stays exit 0.
        if has_autounmask_changes
            && !autounmask_only
            && !(!pretend && autounmask_continue == Some(true))
        {
            return Some(ExitCode::from(1));
        }
        // An unsolvable blocker fails the run for a `--json` consumer's `$?`
        // check too -- entry-owned rows and orphans alike.
        // Portage: _emerge/actions.py, action_build
        if result
            .entries
            .iter()
            .any(|e| e.blockers.iter().any(|b| b.unsolvable))
            || !result.orphan_blockers.is_empty()
        {
            return Some(ExitCode::from(1));
        }
        // Same slot-conflict rule as the text path (see `failure_gates` for the
        // full grounding): a recorded conflict fails the run unless an escape
        // holds (`--autounmask-only`, or backtracking disabled plus
        // `--buildpkgonly`/`--nodeps`).
        if !autounmask_only
            && !result.slot_conflicts.is_empty()
            && !(backtrack_max == 0 && (buildpkgonly || nodeps))
        {
            return Some(ExitCode::from(1));
        }
        // An aborted resolve exits 1 in every output format.
        if gated_abort_partial.is_some() {
            return Some(ExitCode::from(1));
        }
        return Some(ExitCode::SUCCESS);
    }
    None
}

/// The notice for targets satisfied by package.provided.
pub(crate) fn show_pprovided_notice(result: &GraphResult, color: &Colorizer) {
    // Printed to stderr, before the merge list. The "pulled in by" ref is
    // always `'args'` (no `SetArg` tracking); the `@world`/`@selected`
    // solution text is never reached (see `GraphEntry::pprovided_atoms`).
    // Portage: _emerge/depgraph.py, display_problems
    if !result.pprovided_atoms.is_empty() {
        eprint!("{}", color.c("BAD", "\nWARNING: "));
        if result.pprovided_atoms.len() > 1 {
            eprintln!("Requested packages will not be merged because they are listed in");
        } else {
            eprintln!("A requested package will not be merged because it is listed in");
        }
        eprintln!("package.provided:\n");
        for atom in &result.pprovided_atoms {
            eprintln!("  {} pulled in by 'args'", color.c("INFORM", atom));
        }
        eprintln!();
    }
}
