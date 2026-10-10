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
    // Real `output.py::display`: `if self.conf.verbosity == 3:
    // self.print_verbose(...)` -- the `Total: …` counters line, printed
    // after every entry (and blocker) line, only under `-v`, for the
    // tree/columns/flat layouts alike. Real emits `f"\n{self.counters}\n"`
    // (a leading blank line). `--quiet` forces verbosity to 1, so `-pvq`
    // suppresses the line that `-pv` would show. Suppressed together with
    // an empty aborted list (real never calls `display()` there, so no
    // counters exist); the cycle partial counts over its own rows only.
    //
    // Backlog #206 S2: the circular display runs forced-verbose (see
    // `circular_forced_display` above), so the gate reads the effective
    // flags and the counters count the rendered rows, not the entries:
    // real's `_PackageCounters` accumulate per displayed merge row, so a
    // repeat-merge row (the cycle closure) counts again while a
    // `[nomerge]` row never counts. The recount input is exactly the
    // rendered merge/uninstall rows (`rendered_rows` from the tree walk
    // above); `nomerge`, already-installed and disclosure rows contribute
    // nothing, matching real's `set_pkg_info` gating.
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

    // Backlog #185: real `_emerge/actions.py:526` prints one blank line
    // after the merge list, before the `--ask` prompt (or the scheduler,
    // when nothing prompts). `show_merge_list` (not the bare gate) is
    // the condition: real's `print()` only runs when the list branch ran
    // (an aborted resolve returns before it). `--ask` needs none here:
    // `ask_confirm` already opens with that same single blank line
    // (real's `print()` before `UserQuery.query`), so printing another
    // would double it.
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
        if disp_tree {
            // #131 S1: the tree-mode serialization order for the display
            // walk (real's reversed tree-mode retlist). Computed on the
            // displayed slice, so abort-partial lists stay
            // self-consistent. One extra schedule+select pass per `--tree`
            // run; the flat list (and everything else) never sees it.
            let tree_order = if circular_forced_display
                && display_entries.iter().any(|e| e.targets_running_root)
            {
                // Backlog #245 (cross-root partial only; the single-root
                // cycle keeps the re-serialization below): real hands `handler.merge_list` (the stuck remainder
                // drained in digraph insertion order) straight to `display`; it is
                // not re-serialized, and `print_tree` walks the reverse of its
                // retlist, so the partial goes in reversed.
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
            // #68/#72 B2: rows whose replacement waits on its owner print
            // right after that replacement's package line (real appends
            // them to `print_msg` at the solved blocker's merge-list
            // position, `output.py:119-121`).
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

        // Backlog #19 Slice 4: the aborted partial list carries no
        // `NoVisibleCandidate` entries (masked/unsat partials are empty;
        // the cycle remainder is merge-bound), so the disclosure / bare-
        // unsat lines `print_entry_line` emits for them would go silent
        // with the list switch. Re-emit them here, in full-entries order
        // — the same stderr sequence the full-list walk produced, since
        // these lines already go to stderr while the list goes to stdout.
        // Slice 5 reshapes this into the `AbortReason` error block that
        // follows the partial list; until then the content is unchanged.
        // Non-abort resolves (including `Complete` graphs that still
        // carry NVC entries, e.g. build-root-only misses) render those
        // lines in the loop above exactly as before.
        // Backlog #185: pretend-only since the unconditional
        // resolution-phase block above took over the non-`--pretend`
        // preamble. Real's resolution-phase prints (header, notice,
        // timing) run before success/failure is known, so a failed
        // non-`--pretend` resolve already printed them above; re-printing
        // here would double them, and real never shows the partial list
        // itself on a non-`--pretend` abort (`actions.py:460-462`).
        if pretend
            && gated_abort_partial.is_some()
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
            // #135 (c) (O7: un-suppress): real prints the merge-list
            // preamble and the timing line and *then* aborts
            // (`These are the packages ...` + `Calculating dependencies
            // ... done!` + `Dependency resolution took N s
            // (backtrack: N/M).` + blank, all on stdout, before the
            // abort block on stderr). The partial list itself is empty
            // for these arms, so this is the preamble only.
            // Determinism cut (lessons-of-backlog-ops: nondeterministic
            // values ride `--json` only, never stdout; the
            // byte-identical repeated-run invariant is a jointly-owned
            // gate): the wall-clock seconds real prints are omitted --
            // the line carries real's `backtrack: N/M` signal only.
            // O7's "timing line" is therefore the signal half, not the
            // seconds; re-open with the owner to print real seconds.
            println!(
                "These are the packages that would be merged, in order:
"
            );
            println!("Calculating dependencies ... done!");
            // Real prints the timing line on every non-quiet run
            // (backlog #59(b)); under `--quiet` it is suppressed.
            if !quiet {
                println!(
                    "Dependency resolution took (backtrack: {}/{}).
",
                    result.backtrack_restarts, result.backtrack_max,
                );
            }
            let mut thrown_away: Vec<String> = Vec::new();
            for (i, entry) in result.entries.iter().enumerate() {
                // Backlog #195 (round 2): a dependency the resolver
                // collected into a parent USE flip (record-and-fail, real
                // `_apply_parent_use_changes` dropping the item from
                // `_unsatisfied_deps_for_display`) renders no row at all
                // -- the USE-changes block below is its whole output, and
                // the `!!! note:` fallback in `print_entry_line` would
                // duplicate it.
                if matches!(
                    entry.outcome,
                    portage_repo::PretendOutcome::NoVisibleCandidate
                ) && entry.parent_use_suggestion.is_none()
                {
                    // Backlog #244: real's `display_problems` prints the
                    // slot-conflict block before the leftover
                    // `_unsatisfied_deps_for_display` item
                    // (`depgraph.py:11113-11280`); with a conflict
                    // recorded, this line waits until after that block.
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

        // Real `Display.print_blockers()`: the collected `[blocks B ...]`
        // lines, printed as one group after every package line and before
        // the counters. Backlog #80: unresolved rows with no owner entry
        // join the same group (flat and tree alike -- real's
        // `Display.blockers` is mode-independent). Unresolved rows are
        // never column-suppressed, so no `columns` gate here.
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
        // Same non-zero-on-autounmask-changes contract as the text path
        // below (a `--json` consumer's exit-code check must see the
        // "config still needs editing" signal too). `--autounmask-only`
        // stays exit 0 (its whole point is "just show the changes").
        // (`has_autounmask_changes` is bound once, above, next to the
        // `--autounmask-only` re-show gate -- backlog #271.)
        if has_autounmask_changes
            && !autounmask_only
            && !(!pretend && autounmask_continue == Some(true))
        {
            return Some(ExitCode::from(1));
        }
        // An unsolvable blocker fails the run for a `--json` consumer's
        // `$?` check too (real `actions.py` returns 1 regardless of
        // output format) -- entry-owned rows and #80 orphans alike.
        if result
            .entries
            .iter()
            .any(|e| e.blockers.iter().any(|b| b.unsolvable))
            || !result.orphan_blockers.is_empty()
        {
            return Some(ExitCode::from(1));
        }
        // Backlog #62: same slot-conflict rule as the text path below
        // (real `actions.py:460-462` runs before any format split, and
        // `--json` has no real counterpart -- it follows the same rule
        // for consistency): a recorded conflict fails the run unless an
        // escape holds. `--autounmask-only` is escape 1 (its whole point
        // is "just show the changes", exit 0); backtracking disabled
        // plus `--buildpkgonly`/`--nodeps` is escape 2
        // (`_accept_blocker_conflicts`). See the text-path gate for the
        // full grounding and the `--fetchonly`/`--fetch-all-uri`
        // carve-out.
        if !autounmask_only
            && !result.slot_conflicts.is_empty()
            && !(backtrack_max == 0 && (buildpkgonly || nodeps))
        {
            return Some(ExitCode::from(1));
        }
        // Backlog #19 Slice 4: an aborted resolve exits 1 in every output
        // format (real `actions.py:460-462` runs before any format
        // split). Gated like the text path below; the `Aborted` arm there
        // fires first, so this only matters for `--json`.
        if gated_abort_partial.is_some() {
            return Some(ExitCode::from(1));
        }
        return Some(ExitCode::SUCCESS);
    }
    None
}

/// The notice for targets satisfied by package.provided.
pub(crate) fn show_pprovided_notice(result: &GraphResult, color: &Colorizer) {
    // Real `depgraph.py:11192-11235`'s `display_problems()` block for a
    // directly-requested atom that matched `package.provided` -- printed
    // to stderr, before the merge list (matching real portage's own
    // `display_problems()` -> `display()` order). Portuale tracks no
    // `SetArg`, so the "pulled in by" ref is always `'args'` and the real
    // `@world`/`@selected` "A) B) C)" solution text is never reached (a
    // documented divergence -- see `GraphResult::pprovided_atoms`).
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

/// The header printed before the merge list of a real (non-pretend) run.
pub(crate) fn show_resolution_header(opts: &EmergeOpts<'_>, result: &GraphResult) {
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
    // Backlog #185 (fix round 1): real's resolution-phase display for
    // a non-`--pretend` run, printed whether or not the list itself is
    // shown -- and whether or not resolution succeeds. Real `depgraph.py::
    // _start_resolution_display` prints the header (when `--ask`,
    // `--tree` or `--verbose` is given and `--quiet` is absent --
    // `--nodeps` is NOT consulted, the `spinner.mode = QUIET`
    // assignment at `depgraph.py:12129-12130` comes after the header
    // block) and starts the spinner notice before resolving, and
    // `backtrack_depgraph`'s `finally:` completes the notice plus the
    // timing line after (success or failure alike). `--quiet` suppresses
    // everything; `--nodeps` suppresses only the notice and the timing
    // line (QUIET mode: `end_notice()` reports nothing to complete).
    // Provenance: container probe m185 (real 3.0.81.3,
    // `localhost/test-portuale:latest`, `emerge --oneshot --usepkgonly
    // probe/probe-a` in six shapes; the gating is identical in 3.0.82.2
    // by source read) plus the 3.0.82.2 source for exact text.
    if !pretend && !quiet {
        if resolution_header_shown(ResolutionHeaderShownArgs {
            ask,
            tree,
            verbose,
            quiet,
        }) {
            // Real `_start_resolution_display`
            // (`depgraph.py:12087-12127`): a leading blank line, then
            // `These are the packages that would be ...` -- `in reverse
            // order` for `--tree` (a plain `:` under
            // `--unordered-display`), `fetched` under `--fetchonly` and
            // `built` under `--buildpkgonly` (`--fetchonly` is
            // unimplemented here, rc 2, so only the `built` arm below is
            // reachable), else `merged, in order:` -- then a blank line.
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
            // Real `stdout_spinner` STATIC mode (piped stdout, real
            // `stdout_spinner.py:105-145`): `begin_notice("Calculating
            // dependencies")` writes the notice plus ` ...`, and
            // `end_notice()` completes it with ` done!`. NOTE: the m185
            // probe (3.0.81.3) shows two spaces before `...`; the 3.0.82.2
            // source (`f"{notice} ..."` + `" done!\n"`) shows one -- ported
            // from the pinned source, pending the coordinator's 3.0.82.2
            // bed confirmation (see the m185 report).
            println!("Calculating dependencies ... done!");
            // Real `_show_resolution_report` (`depgraph.py:12134-12146`):
            // `Dependency resolution took N.NN s (backtrack: B/M).` plus a
            // trailing blank line. The wall-clock seconds are cut exactly
            // like the #135 abort-path timing line just below (repeated-run
            // determinism is a jointly-owned gate; nondeterministic values
            // ride `--json` only) -- the `backtrack: B/M` signal is kept.
            println!(
                "Dependency resolution took (backtrack: {}/{}).\n",
                result.backtrack_restarts, result.backtrack_max,
            );
        }
    }
}
