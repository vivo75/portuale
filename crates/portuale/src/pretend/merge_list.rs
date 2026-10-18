//! The counts and summary lines under the merge list, and the redisplay before a merge.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// The values [`show_merge_list_summary`] reads from its caller.
pub(crate) struct ShowMergeListSummaryArgs<'a> {
    pub(crate) show_merge_list: bool,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) circular_forced_display: bool,
    pub(crate) rendered_rows: &'a [(usize, bool)],
    pub(crate) display_entries: &'a [GraphEntry],
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

/// The values [`render_merge_list`] reads from its caller.
pub(crate) struct RenderMergeListArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) show_merge_list: bool,
    pub(crate) disp_tree: bool,
    pub(crate) circular_forced_display: bool,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) expanded_atoms: &'a [String],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) result: &'a GraphResult,
    pub(crate) rendered_rows: &'a mut Vec<(usize, bool)>,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) disp_verbose: bool,
    pub(crate) disp_quiet: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    pub(crate) columnwidth: i64,
    pub(crate) gated_abort_partial: Option<&'a Vec<GraphEntry>>,
    pub(crate) deferred_abort_unsat: &'a mut Vec<usize>,
}

/// The merge list: one row per entry, plus blocker rows.
pub(crate) fn render_merge_list(args: RenderMergeListArgs<'_>) {
    let RenderMergeListArgs {
        plan,
        opts,
        show_merge_list,
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms,
        config,
        root,
        repos,
        result,
        rendered_rows,
        top_level_pkgs,
        disp_verbose,
        disp_quiet,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        columnwidth,
        gated_abort_partial,
        deferred_abort_unsat,
    } = args;
    let &ResolvePlan {
        dynamic_deps,
        ref root_deps_running_root,
        ..
    } = plan;
    let &EmergeOpts {
        implicit_system_deps,
        onlydeps,
        oneshot,
        unordered_display,
        alphabetical,
        quiet,
        columns,
        pretend,
        verbose,
        ..
    } = opts;
    if show_merge_list {
        render_merge_rows(RenderMergeRowsArgs {
            disp_tree,
            circular_forced_display,
            display_entries,
            expanded_atoms,
            config,
            root,
            implicit_system_deps,
            repos,
            dynamic_deps,
            result,
            rendered_rows,
            top_level_pkgs,
            onlydeps,
            oneshot,
            unordered_display,
            disp_verbose,
            disp_quiet,
            alphabetical,
            root_deps_running_root,
            color,
            system_atoms,
            world_atoms,
            force_reinstall_cps,
            blocker_lines,
            quiet,
            columns,
            columnwidth,
            pretend,
            gated_abort_partial,
            deferred_abort_unsat,
            verbose,
        });
    } else {
        // No list is shown, but an aborted resolve still says why.
        show_abort_disclosures(ShowAbortDisclosuresArgs {
            result,
            root,
            top_level_pkgs,
            onlydeps,
            oneshot,
            alphabetical,
            root_deps_running_root,
            color,
            system_atoms,
            world_atoms,
            force_reinstall_cps,
            quiet,
            columns,
            columnwidth,
            pretend,
            gated_abort_partial,
            deferred_abort_unsat,
            verbose,
        });
    }
}

/// The values [`render_merge_rows`] reads from its caller.
pub(crate) struct RenderMergeRowsArgs<'a> {
    pub(crate) disp_tree: bool,
    pub(crate) circular_forced_display: bool,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) expanded_atoms: &'a [String],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) implicit_system_deps: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) dynamic_deps: bool,
    pub(crate) result: &'a GraphResult,
    pub(crate) rendered_rows: &'a mut Vec<(usize, bool)>,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) unordered_display: bool,
    pub(crate) disp_verbose: bool,
    pub(crate) disp_quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    pub(crate) quiet: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
    pub(crate) pretend: bool,
    pub(crate) gated_abort_partial: Option<&'a Vec<GraphEntry>>,
    pub(crate) deferred_abort_unsat: &'a mut Vec<usize>,
    pub(crate) verbose: bool,
}

/// The merge-list rows and the blocker lines.
pub(crate) fn render_merge_rows(args: RenderMergeRowsArgs<'_>) {
    let RenderMergeRowsArgs {
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        result,
        rendered_rows,
        top_level_pkgs,
        onlydeps,
        oneshot,
        unordered_display,
        disp_verbose,
        disp_quiet,
        alphabetical,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        quiet,
        columns,
        columnwidth,
        pretend,
        gated_abort_partial,
        deferred_abort_unsat,
        verbose,
    } = args;
    render_tree_rows(RenderTreeRowsArgs {
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        result,
        rendered_rows,
        top_level_pkgs,
        onlydeps,
        oneshot,
        unordered_display,
        disp_verbose,
        disp_quiet,
        alphabetical,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        quiet,
        columns,
        columnwidth,
    });

    show_abort_disclosures(ShowAbortDisclosuresArgs {
        result,
        root,
        top_level_pkgs,
        onlydeps,
        oneshot,
        alphabetical,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        quiet,
        columns,
        columnwidth,
        pretend,
        gated_abort_partial,
        deferred_abort_unsat,
        verbose,
    });

    // The collected `[blocks B ...]` lines print as one group after every
    // package line and before the counters; owner-less rows join the group.
    // Portage: output.py, Display.print_blockers
    for orphan in &result.orphan_blockers {
        blocker_lines.push(format_blocker_row(
            &format!(
                "{}/{}-{}",
                orphan.owner_category, orphan.owner_package, orphan.owner_version
            ),
            !quiet,
            "",
            color,
            &orphan.conflict,
        ));
    }
    for line in blocker_lines {
        println!("{line}");
    }
}

/// The values [`show_abort_disclosures`] reads from its caller.
pub(crate) struct ShowAbortDisclosuresArgs<'a> {
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) alphabetical: bool,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) quiet: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
    pub(crate) pretend: bool,
    pub(crate) gated_abort_partial: Option<&'a Vec<GraphEntry>>,
    pub(crate) deferred_abort_unsat: &'a mut Vec<usize>,
    pub(crate) verbose: bool,
}

/// The unsatisfied-dependency disclosures of an aborted resolve.
pub(crate) fn show_abort_disclosures(args: ShowAbortDisclosuresArgs<'_>) {
    let ShowAbortDisclosuresArgs {
        result,
        root,
        top_level_pkgs,
        onlydeps,
        oneshot,
        alphabetical,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        quiet,
        columns,
        columnwidth,
        pretend,
        gated_abort_partial,
        deferred_abort_unsat,
        verbose,
    } = args;
    // The aborted partial list carries no `NoVisibleCandidate` entries, so
    // re-emit their disclosure lines here in full-entries order (stderr,
    // like the loop above). A non-`--pretend` abort prints the disclosures but
    // never the preamble or the partial list.
    // Portage: _emerge/depgraph.py, display_problems
    if gated_abort_partial.is_some()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted {
                reason: portage_repo::AbortReason::MaskedDep { .. }
                    | portage_repo::AbortReason::UnsatisfiedAtom { .. }
                    | portage_repo::AbortReason::VirtualCycle { .. },
                ..
            }
        )
    {
        if pretend {
            // The merge-list preamble and timing line print before the abort
            // block on stderr; the partial list itself is empty for these arms.
            // The wall-clock seconds are cut (determinism); only `backtrack:
            // N/M` is kept. Portage: _emerge/depgraph.py, _show_merge_list
            println!(
                "These are the packages that would be merged, in order:
"
            );
            println!("Calculating dependencies ... done!");
            // The timing line is suppressed under `--quiet`.
            if !quiet {
                println!(
                    "Dependency resolution took (backtrack: {}/{}).
",
                    result.backtrack_restarts, result.backtrack_max,
                );
            }
        }
        let mut thrown_away: Vec<String> = Vec::new();
        for (i, entry) in result.entries.iter().enumerate() {
            // A dependency collected into a parent USE flip renders no row
            // (the USE-changes block below is its whole output).
            if matches!(
                entry.outcome,
                portage_repo::PretendOutcome::NoVisibleCandidate
            ) && entry.parent_use_suggestion.is_none()
            {
                // With a slot conflict recorded, this line waits until after
                // the slot-conflict block (Portage's `display_problems` order).
                if !result.slot_conflicts.is_empty() {
                    deferred_abort_unsat.push(i);
                    continue;
                }
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
}

/// The values [`render_tree_rows`] reads from its caller.
pub(crate) struct RenderTreeRowsArgs<'a> {
    pub(crate) disp_tree: bool,
    pub(crate) circular_forced_display: bool,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) expanded_atoms: &'a [String],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) implicit_system_deps: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) dynamic_deps: bool,
    pub(crate) result: &'a GraphResult,
    pub(crate) rendered_rows: &'a mut Vec<(usize, bool)>,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) unordered_display: bool,
    pub(crate) disp_verbose: bool,
    pub(crate) disp_quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    pub(crate) quiet: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
}

/// The tree-mode rows of the merge list.
pub(crate) fn render_tree_rows(args: RenderTreeRowsArgs<'_>) {
    let RenderTreeRowsArgs {
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        result,
        rendered_rows,
        top_level_pkgs,
        onlydeps,
        oneshot,
        unordered_display,
        disp_verbose,
        disp_quiet,
        alphabetical,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        quiet,
        columns,
        columnwidth,
    } = args;
    if disp_tree {
        // The tree-mode serialization order, computed on the displayed
        // slice so abort-partial lists stay self-consistent.
        let tree_order =
            if circular_forced_display && display_entries.iter().any(|e| e.targets_running_root) {
                // A cross-root cycle partial goes in reversed, like Portage's
                // stuck remainder (never re-serialized).
                (0..display_entries.len()).rev().collect()
            } else {
                portage_repo::tree_display_order(portage_repo::TreeDisplayOrderArgs {
                    entries: display_entries,
                    top_level_atoms: expanded_atoms,
                    config,
                    root,
                    implicit_system_deps,
                    repos,
                    dynamic_deps,
                    circular: &result.circular_dependency,
                })
            };
        *rendered_rows = print_tree(PrintTreeArgs {
            entries: display_entries,
            display_order: &tree_order,
            root,
            top_level_pkgs,
            onlydeps,
            oneshot,
            unordered_display,
            verbose: disp_verbose,
            quiet: disp_quiet,
            alphabetical,
            running_root: root_deps_running_root.as_deref(),
            color,
            system_atoms,
            world_atoms,
            force_reinstall_cps,
            blocker_lines: &mut *blocker_lines,
            masked_deps: &result.masked_deps,
            use_unsat_deps: &result.use_unsat_deps,
            plain_miss_deps: &result.plain_miss_deps,
            force_sizes: circular_forced_display,
            circular: &result.circular_dependency,
        });
    } else {
        // Blocker rows print right after their replacement's package line.
        // Portage: output.py
        let inline_blockers = collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
            entries: display_entries,
            root,
            quiet,
            columns,
            color,
            circular: &result.circular_dependency,
        });
        for i in 0..display_entries.len() {
            print_entry_line(PrintEntryLineArgs {
                entries: display_entries,
                root,
                index: i,
                indent: "",
                ordered: true,
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose: disp_verbose,
                quiet: disp_quiet,
                alphabetical,
                columns,
                columnwidth,
                running_root: root_deps_running_root.as_deref(),
                color,
                system_atoms,
                world_atoms,
                force_reinstall_cps,
                blocker_lines: &mut *blocker_lines,
                masked_deps: &result.masked_deps,
                use_unsat_deps: &result.use_unsat_deps,
                plain_miss_deps: &result.plain_miss_deps,
                force_sizes: circular_forced_display,
                circular: &result.circular_dependency,
            });
            for (_, line) in inline_blockers.iter().filter(|(after, _)| *after == i) {
                println!("{line}");
            }
        }
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

/// The header printed before the merge list of a non-pretend run.
pub(crate) fn show_resolution_header(opts: &EmergeOpts<'_>, result: &GraphResult) {
    show_resolution_header_with_counts(opts, result.backtrack_restarts, result.backtrack_max);
}

/// The same notice with explicit backtrack counts: a non-`--pretend` run
/// that aborts before any `GraphResult` exists (the top-level all-masked
/// arm) prints it with zero restarts -- the argument dies on first
/// dequeue, before any backtrack -- over the run's backtrack max.
pub(crate) fn show_resolution_header_with_counts(
    opts: &EmergeOpts<'_>,
    backtrack_restarts: u64,
    backtrack_max: u32,
) {
    let &EmergeOpts {
        pretend,
        quiet,
        ask,
        tree,
        verbose,
        buildpkgonly,
        unordered_display,
        nodeps,
        ..
    } = opts;
    // The resolution-phase display for a non-`--pretend` run, printed whether
    // or not the list itself is shown and whether or not resolution succeeds.
    // See `resolution_header_shown`/`resolution_notice_shown` for the gates.
    // Portage: _emerge/depgraph.py, _start_resolution_display
    if !pretend && !quiet {
        if resolution_header_shown(ResolutionHeaderShownArgs {
            ask,
            tree,
            verbose,
            quiet,
        }) {
            // `These are the packages that would be ...`: `in reverse order` for
            // `--tree` (a plain `:` under `--unordered-display`), `built` under
            // `--buildpkgonly`, else `merged, in order:`.
            println!();
            let action = if buildpkgonly { "built" } else { "merged" };
            if tree {
                if unordered_display {
                    println!("These are the packages that would be {action}:");
                } else {
                    println!("These are the packages that would be {action}, in reverse order:");
                }
            } else {
                println!("These are the packages that would be {action}, in order:");
            }
            println!();
        }
        if resolution_notice_shown(quiet, nodeps) {
            // Ported from the pinned source (one space before `...`).
            println!("Calculating dependencies ... done!");
            // The wall-clock seconds are cut (determinism); only the
            // `backtrack: B/M` signal is kept, plus a trailing blank line.
            println!(
                "Dependency resolution took (backtrack: {backtrack_restarts}/{backtrack_max}).\n",
            );
        }
    }
}
