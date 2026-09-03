//! Missing arguments, and the abort reports for cycles and unsatisfied dependencies.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// Missing arguments, and the abort reports for cycles and unsatisfied dependencies.
pub(crate) fn show_missing_args_and_aborts(
    result: &GraphResult,
    root: &Path,
    color: &Colorizer,
    repos: &[RepoConfig],
    config: &Config,
) {
    // The world-file tail for dropped set members: the notice fires only for
    // a missing member of the world file itself; the masked-or-don't-exist
    // list names every missing arg in requested order. Neither carries a
    // `--quiet` gate; `--json` returns above, and `--columns` has no gate.
    // Portage: depgraph.py, display_problems
    if !result.missing_args.is_empty() {
        let world_atoms: HashSet<String> = read_world_atoms(root)
            .unwrap_or_default()
            .into_iter()
            .map(|a| portage_repo::apply_updates_to_atom(&a))
            .collect();
        if result.missing_args.iter().any(|a| world_atoms.contains(a)) {
            println!();
            println!("!!! Problems have been detected with your world file");
            println!("!!! Please run emaint --check world");
            println!();
        }
        println!();
        println!("!!! Ebuilds for the following packages are either all");
        println!("!!! masked or don't exist:");
        println!("{}", result.missing_args.join(" "));
    }

    // With the abort gate on, an aborted cycle prints its circular block here,
    // before the autounmask section, matching Portage's `display_problems`
    // order. A masked/unsat abort prints no circular block at all.
    // Portage: depgraph.py, display_problems
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted {
            reason: portage_repo::AbortReason::UnserializableCycle { members: _ },
            ..
        } = &result.outcome
        && let Some(cycle) = result.circular_deps.first()
    {
        print_circular_block(PrintCircularBlockArgs {
            cycle,
            edge_labels: &portage_repo::cycle_edge_labels(&result.entries, cycle),
            color,
            repos,
            config,
            autounmask_use_changes: &result.autounmask_use_changes,
            entries: &result.entries,
            large_cycle_count: result.large_cycle_count,
        });
    }

    // A virtual cycle prints `\n\n!!! virtual cycle detected:\n\n`, one
    // `  {cpv}::{repo}` line per probe-stack member (sorted), then `\n`,
    // to stderr, mid-resolution. No merge list, no counters, no trailing
    // error block; the shared abort gate below exits 1. Uncoloured.
    // Portage: lib/_emerge/depgraph.py, select_files
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted {
            reason: portage_repo::AbortReason::VirtualCycle { members },
            ..
        } = &result.outcome
    {
        eprint!("\n\n!!! virtual cycle detected:\n\n");
        for member in members {
            eprintln!("  {member}");
        }
        eprintln!();
    }
}

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

/// The values [`show_slot_conflicts`] reads from its caller.
pub(crate) struct ShowSlotConflictsArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) useoldpkg_atoms_render: &'a [String],
    pub(crate) color: &'a Colorizer,
}

/// The slot-conflict notice.
pub(crate) fn show_slot_conflicts(args: ShowSlotConflictsArgs<'_>) {
    let ShowSlotConflictsArgs {
        plan,
        opts,
        result,
        root,
        repos,
        config,
        useoldpkg_atoms_render,
        color,
    } = args;
    let &ResolvePlan { usepkgonly, .. } = plan;
    let &EmergeOpts {
        verbose_conflicts,
        ref excluded,
        backtrack_max,
        ..
    } = opts;
    // The slot-conflict notice: the preamble, one block per conflicting
    // slot, the per-instance `pulled in by` lines with the `^` marker, the
    // `(and N more ...)` tail, the `--verbose-conflicts` footer, the
    // `need_rebuild` trailer, and the advisory. Display-only: the
    // exit-code consequence lives at the "display, then fail" gate.
    // Portage: _emerge/depgraph.py, _show_slot_collision_notice;
    // resolver/slot_collision.py
    if !result.slot_conflicts.is_empty() {
        let mut any_omitted = false;
        // Installed parents with built slot-operator atoms that cannot be
        // rebuilt, in first-seen order, printed once after all conflicts.
        let mut need_rebuild: Vec<(String, String)> = Vec::new();
        let installed_pkgs: std::collections::HashSet<(String, String, String)> =
            all_installed_packages(root)
                .into_iter()
                .map(|p| (p.category, p.package, p.version))
                .collect();
        println!();
        println!("!!! Multiple package instances within a single package slot have been pulled");
        println!("!!! into the dependency graph, resulting in a slot conflict:");
        for c in &result.slot_conflicts {
            println!();
            println!("{}/{}:{}", c.category, c.package, c.slot);
            for inst in &c.instances {
                println!();
                if inst.installed {
                    // An installed instance names its position.
                    println!(
                        "  ({}/{}-{}:{}/{}::{}, installed in '{}') {} pulled in by",
                        c.category,
                        c.package,
                        inst.version,
                        c.slot,
                        inst.sub_slot,
                        inst.repo_name,
                        root.display(),
                        render_pkg_use_display(&inst.use_display)
                    );
                } else {
                    println!(
                        "  ({}/{}-{}:{}/{}::{}, ebuild scheduled for merge) {} pulled in by",
                        c.category,
                        c.package,
                        inst.version,
                        c.slot,
                        inst.sub_slot,
                        inst.repo_name,
                        render_pkg_use_display(&inst.use_display)
                    );
                }
                // A bare `(Argument)` parent shows only when some other
                // instance is already installed.
                let show_argument_parents = c
                    .instances
                    .iter()
                    .any(|o| !std::ptr::eq(o, inst) && o.installed);
                // Compare against every other instance in the slot by identity,
                // not version: a same-version merge/installed twin still sees
                // its twin, so the USE parent classifies.
                // Portage: resolver/slot_collision.py
                let others: Vec<ConflictOther> = c
                    .instances
                    .iter()
                    .filter(|o| !std::ptr::eq(*o, inst))
                    .map(|o| {
                        // An installed other's own vdb-recorded flag sets,
                        // like the `use_display` on its header line.
                        let (iuse, use_flags) = if o.installed {
                            portage_repo::installed_pkg_iuse_and_use(
                                root,
                                &c.category,
                                &c.package,
                                &o.version,
                            )
                        } else {
                            slot_conflict_flag_sets(
                                repos,
                                config,
                                &c.category,
                                &c.package,
                                &o.version,
                            )
                        };
                        ConflictOther {
                            cpv: format!(
                                "{}/{}-{}:{}/{}::{}",
                                c.category, c.package, o.version, c.slot, o.sub_slot, o.repo_name
                            ),
                            iuse,
                            use_flags,
                            installed: o.installed,
                        }
                    })
                    .collect();
                let mut classified: Vec<ClassifiedParent> = Vec::new();
                // An installed parent renders `required by (<cpv>, installed
                // in '<root>')`.
                let mut classified_installed: Vec<bool> = Vec::new();
                for p in &inst.parents {
                    if p.parent_cpv.is_empty() && !show_argument_parents {
                        continue;
                    }
                    let Some(atom) = parse_atom(&p.atom) else {
                        continue;
                    };
                    let (mut reasons, use_unconditional) = slot_conflict_reasons(&atom, &others);
                    // A bare command-line parent satisfied by every other
                    // instance is still shown once some other instance is
                    // installed; only fires when no version/slot/USE reason
                    // did.
                    // Portage: resolver/slot_collision.py
                    if p.parent_cpv.is_empty()
                        && reasons.is_empty()
                        && others.iter().any(|o| o.installed)
                    {
                        reasons.push(SlotConflictReason::Argument);
                    }
                    if !reasons.is_empty() {
                        classified.push(ClassifiedParent {
                            parent_cpv: &p.parent_cpv,
                            atom_str: &p.atom,
                            atom,
                            reasons,
                            use_unconditional,
                            use_display: &p.use_display,
                        });
                        classified_installed.push(p.installed);
                    }
                }
                let num_all_specific: usize = classified.iter().map(|p| p.reasons.len()).sum();
                // Group parents by reason, first-seen order preserved.
                let mut groups: Vec<(SlotConflictReason, Vec<usize>)> = Vec::new();
                for (i, p) in classified.iter().enumerate() {
                    for reason in &p.reasons {
                        match groups.iter_mut().find(|(r, _)| r == reason) {
                            Some((_, members)) => members.push(i),
                            None => groups.push((reason.clone(), vec![i])),
                        }
                    }
                }
                // Representative selection: farthest version per `cp` for a
                // `version` group, the first member for a `slot` group, every
                // member of a `use` group, and every member under
                // `--verbose-conflicts`.
                // Portage: resolver/slot_collision.py
                let mut selected: Vec<usize> = Vec::new();
                for (reason, members) in &groups {
                    if verbose_conflicts {
                        selected.extend(members.iter().copied());
                        continue;
                    }
                    match reason {
                        SlotConflictReason::Version(sub) => {
                            let mut best: Vec<(String, usize)> = Vec::new();
                            for &m in members {
                                let atom = &classified[m].atom;
                                let cp = format!("{}/{}", atom.category, atom.package);
                                match best.iter_mut().find(|(c, _)| *c == cp) {
                                    None => best.push((cp, m)),
                                    Some((_, cur)) => {
                                        let cur_ver =
                                            classified[*cur].atom.version.as_deref().unwrap_or("");
                                        let new_ver = atom.version.as_deref().unwrap_or("");
                                        let cmp = portage_versions::vercmp(new_ver, cur_ver);
                                        let farther = match *sub {
                                            "ge" | "eq" => cmp == Some(1),
                                            "le" => cmp == Some(-1),
                                            _ => false,
                                        };
                                        if farther {
                                            *cur = m;
                                        }
                                    }
                                }
                            }
                            selected.extend(best.into_iter().map(|(_, m)| m));
                        }
                        SlotConflictReason::Slot(_) => {
                            if let Some(&first) = members.first() {
                                selected.push(first);
                            }
                        }
                        SlotConflictReason::Use(_) => {
                            selected.extend(members.iter().copied());
                        }
                        SlotConflictReason::Argument => {
                            selected.extend(members.iter().copied());
                        }
                    }
                }
                // The `need_rebuild` scan runs over every `slot`-group parent
                // (verbose or not): an unrebuildable parent is always shown,
                // and recorded for the once-per-notice trailer below.
                for (reason, members) in &groups {
                    if !matches!(reason, SlotConflictReason::Slot(_)) {
                        continue;
                    }
                    for &m in members {
                        let p = &classified[m];
                        if let Some(why) = slot_conflict_need_rebuild(SlotConflictNeedRebuildArgs {
                            parent_cpv: p.parent_cpv,
                            atom: &p.atom,
                            excluded,
                            useoldpkg_atoms: useoldpkg_atoms_render,
                            usepkgonly,
                            repos,
                            config,
                            installed: &installed_pkgs,
                        }) {
                            if !selected.contains(&m) {
                                selected.push(m);
                            }
                            let entry = (format!("({}, installed)", p.parent_cpv), why);
                            if !need_rebuild.contains(&entry) {
                                need_rebuild.push(entry);
                            }
                        }
                    }
                }
                selected.sort_unstable();
                selected.dedup();
                // `unconditional_use_deps` first (Portage's own set order is
                // nondeterministic; portuale keeps the stable classified
                // order within each half).
                selected.sort_by_key(|m| !classified[*m].use_unconditional);
                for &m in &selected {
                    let p = &classified[m];
                    let version_violated = p
                        .reasons
                        .iter()
                        .any(|r| matches!(r, SlotConflictReason::Version(_)));
                    let slot_violated = p
                        .reasons
                        .iter()
                        .any(|r| matches!(r, SlotConflictReason::Slot(_)));
                    let use_flags: Vec<String> = p
                        .reasons
                        .iter()
                        .filter_map(|r| match r {
                            SlotConflictReason::Use(f) => Some(f.clone()),
                            _ => None,
                        })
                        .collect();
                    let idx = slot_conflict_caret_idx(
                        p.atom_str,
                        &p.atom,
                        version_violated,
                        slot_violated,
                        &use_flags,
                    );
                    // The violated spans render red under `--color y`; the `^`
                    // markers are placed on the displayed string, so color
                    // and carets agree (Portage marks pre-color indices and
                    // drifts: a genuine upstream bug not reproduced here).
                    let (atom_display, shifted) = colorize_marked_spans(p.atom_str, &idx, color);
                    // A bare command-line parent renders `<atom> (Argument)`
                    // with no marker line; an installed parent names its
                    // position like an installed instance does.
                    if p.parent_cpv.is_empty() {
                        println!("    {atom_display} (Argument)");
                        continue;
                    }
                    let parent_pos = if classified_installed[m] {
                        format!("{}, installed in '{}'", p.parent_cpv, root.display())
                    } else {
                        format!("{}, ebuild scheduled for merge", p.parent_cpv)
                    };
                    let cur_line = format!(
                        "{} required by ({}) {}\n",
                        atom_display,
                        parent_pos,
                        render_pkg_use_display(p.use_display)
                    );
                    let marker: String = (0..cur_line.chars().count())
                        .map(|k| if shifted.contains(&k) { '^' } else { ' ' })
                        .collect();
                    print!("    {cur_line}");
                    println!("    {marker}");
                }
                if selected.is_empty() && !classified.is_empty() {
                    println!(
                        "    (no parents that aren't satisfied by other packages in this slot)"
                    );
                }
                let omitted = num_all_specific - selected.len();
                if omitted > 0 {
                    any_omitted = true;
                    let noun = if selected.len() > 1 {
                        "problems"
                    } else {
                        "problem"
                    };
                    println!("    (and {omitted} more with the same {noun})");
                }
            }
        }
        if any_omitted {
            println!();
            println!(
                "{}",
                color.c(
                    "INFORM",
                    "NOTE: Use the '--verbose-conflicts' option to display parents omitted above"
                )
            );
            println!();
        }
        // Installed parents with built slot-operator atoms that cannot
        // be rebuilt.
        // Portage: resolver/slot_collision.py
        if !need_rebuild.is_empty() {
            println!();
            println!("!!! The slot conflict(s) shown above involve package(s) which may need to");
            println!("!!! be rebuilt in order to solve the conflict(s). However, the following");
            println!("!!! package(s) cannot be rebuilt for the reason(s) shown:");
            println!();
            for (ppkg, reason) in &need_rebuild {
                println!("  {ppkg}: {reason}");
            }
        }
        println!();
        for line in [
            "It may be possible to solve this problem by using package.mask to",
            "prevent one of those packages from being selected. However, it is also",
            "possible that conflicting dependencies exist such that they are",
            "impossible to satisfy simultaneously.  If such a conflict exists in",
            "the dependencies of two different packages, then those packages can",
        ] {
            println!("{line}");
        }
        // The `--backtrack` hint shows unless backtracking is off or already
        // at >= 30; a loop ended early by `need_config_change` still counts
        // as allowing backtracking.
        // Portage: _emerge/depgraph.py
        if backtrack_max > 0 && backtrack_max < 30 && !result.settled_with_backtracking {
            println!("not be installed simultaneously. You may want to try a larger value of");
            println!("the --backtrack option, such as --backtrack=30, in order to see if");
            println!("that will solve this conflict automatically.");
        } else {
            println!("not be installed simultaneously.");
        }
        println!();
        println!("For more information, see MASKED PACKAGES section in the emerge man");
        println!("page or refer to the Gentoo Handbook.");
        println!();
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
