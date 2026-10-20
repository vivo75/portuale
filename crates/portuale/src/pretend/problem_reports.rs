//! Missing arguments, and the abort reports for cycles and unsatisfied dependencies.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;
use portage_repo::SlotConflictInstance;

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

/// Updates skipped for missing dependencies.
pub(crate) fn show_skipped_missing_deps(
    quiet: bool,
    debug: bool,
    result: &GraphResult,
    color: &Colorizer,
) {
    // The abbreviated tail: slots whose higher masked version lost to an
    // unsatisfied dependency under backtracking. Same `--quiet`-unless-`--debug`
    // gate as the `WARNING` block; no `--columns` gate.
    // Portage: depgraph.py, _show_missed_update_unsatisfied_dep
    if !(quiet && !debug) && !result.skipped_missing_deps.is_empty() {
        println!();
        println!("!!! The following update(s) have been skipped due to unsatisfied dependencies");
        println!("!!! triggered by backtracking:");
        println!();
        for s in &result.skipped_missing_deps {
            println!("{}/{}:{}", s.category, s.package, s.slot);
        }
    }

    // The full per-update form: one block per masked higher version whose
    // missing atom bites nothing backtrack-masked. Same gate as above;
    // `--json` returns above, no `--columns` gate.
    // Portage: depgraph.py, _show_missed_update_unsatisfied_dep
    if !(quiet && !debug) && !result.skipped_missing_dep_full.is_empty() {
        for f in &result.skipped_missing_dep_full {
            println!();
            println!("!!! The following update has been skipped due to unsatisfied dependencies:");
            println!();
            println!("{}/{}:{}", f.category, f.package, f.slot);
            println!();
            println!("  selected: {}", f.selected_display);
            println!(
                "  skipped: {} (see unsatisfied dependency below)",
                f.skipped_display
            );
            println!();
            match &f.dep {
                portage_repo::SkippedDepDisclosure::Masked(report) => {
                    println!(
                        "!!! {}{}{}",
                        color.c("BAD", "All ebuilds that could satisfy "),
                        color.c(
                            "INFORM",
                            &format!("{:?}{}", report.atom, portage_repo::display_root_suffix())
                        ),
                        color.c("BAD", " have been masked.")
                    );
                    println!(
                        "!!! One of the following masked packages is required to complete your request:"
                    );
                    for (cpv, reasons) in &report.masked {
                        println!("- {cpv} (masked by: {})", reasons.join(", "));
                    }
                    if report
                        .masked
                        .iter()
                        .any(|(_, reasons)| reasons.iter().any(|r| r.starts_with("EAPI ")))
                    {
                        print!("{}", portage_repo::eapi_mask_upgrade_notice());
                    }
                    println!();
                    for (node, ty) in &report.chain {
                        println!(
                            "(dependency required by \"{}\" [{ty}])",
                            color.c("INFORM", node)
                        );
                    }
                    println!("For more information, see the MASKED PACKAGES section in the emerge");
                    println!("man page or refer to the Gentoo Handbook.");
                    println!();
                }
                portage_repo::SkippedDepDisclosure::Plain(report) => {
                    println!(
                        "emerge: there are no {} to satisfy {}.",
                        if report.binary_only {
                            "binary packages"
                        } else {
                            "ebuilds"
                        },
                        color.c(
                            "green",
                            &format!("{:?}{}", report.atom, portage_repo::display_root_suffix())
                        )
                    );
                    for (node, ty) in &report.chain {
                        println!(
                            "(dependency required by \"{}\" [{ty}])",
                            color.c("INFORM", node)
                        );
                    }
                    println!();
                }
            }
        }
    }
}

/// Updates skipped by slot conflicts or masks.
pub(crate) fn show_skipped_updates(
    plan: &ResolvePlan,
    opts: &EmergeOpts<'_>,
    result: &GraphResult,
    root: &Path,
    repos: &[RepoConfig],
) {
    let ResolvePlan {
        root_deps_running_root,
        running_config,
        ..
    } = plan;
    let &EmergeOpts { quiet, debug, .. } = opts;
    // The slot-conflict missed-update block, printed after the merge list
    // with rc 0. Rows naming the same missed package group into one block.
    // Each package renders with its effective USE display; destinations
    // follow the per-node rule (quoted `to '<root>'` iff the node's own
    // root is not `/`). A top-level `(Argument)` parent renders as the bare
    // indented arg line with no atom and no marker. Suppressed under
    // `--quiet` unless `--debug`; `--json` and `--columns` never reach here.
    // Portage: depgraph.py, _conflict_missed_update
    if !(quiet && !debug) && !result.skipped_updates.is_empty() {
        println!(
            "WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:"
        );
        for group in portage_repo::group_skipped_updates(&result.skipped_updates) {
            let Some(header) = group.first() else {
                continue;
            };
            println!();
            println!("{}/{}:{}", header.category, header.package, header.slot);
            println!();
            // Per-node destination and own-root USE paint, like the missed
            // line above.
            let missed_suffix = skipped_node_suffix(
                header.missed_targets_running_root,
                root,
                root_deps_running_root.as_deref(),
            );
            let missed_use = if header.missed_targets_running_root
                && let Some(cfg) = running_config.as_ref()
            {
                portage_repo::skipped_update_use_display_for(
                    repos,
                    cfg,
                    &header.category,
                    &header.package,
                    &header.skipped_version,
                )
            } else {
                header.skipped_use.clone()
            };
            println!(
                "  ({}/{}-{}:{}/{}::{}, ebuild scheduled for merge{}) {} conflicts with",
                header.category,
                header.package,
                header.skipped_version,
                header.slot,
                header.skipped_sub_slot,
                header.skipped_repo,
                missed_suffix,
                render_pkg_use_display(&missed_use),
            );
            for s in group.iter() {
                if s.consumer_cpv.is_empty() {
                    // The bare command-line argument: no atom and no `^` marker.
                    // The recorded atom already is that argument text.
                    // Portage: depgraph.py
                    println!("    {}", s.atom);
                    continue;
                }
                if s.consumer_installed {
                    // The installed root is the consumer's own.
                    // Portage: depgraph.py
                    let installed_root: &Path = if s.consumer_targets_running_root {
                        root_deps_running_root.as_deref().unwrap_or(root)
                    } else {
                        root
                    };
                    println!(
                        "    {} required by ({}, installed in '{}') {}",
                        s.atom,
                        s.consumer_cpv,
                        installed_root.display(),
                        render_pkg_use_display(&s.consumer_use),
                    );
                } else {
                    // Per-node destination + own-root USE paint, like above;
                    // an unparsable cpv keeps the resolve-time paint.
                    let consumer_suffix = skipped_node_suffix(
                        s.consumer_targets_running_root,
                        root,
                        root_deps_running_root.as_deref(),
                    );
                    let consumer_use = if s.consumer_targets_running_root
                        && let Some(cfg) = running_config.as_ref()
                        && let Some((cc, cp2, cv)) = s
                            .consumer_cpv
                            .split(':')
                            .next()
                            .and_then(portage_repo::split_cpv)
                    {
                        portage_repo::skipped_update_use_display_for(repos, cfg, &cc, &cp2, &cv)
                    } else {
                        s.consumer_use.clone()
                    };
                    println!(
                        "    {} required by ({}, ebuild scheduled for merge{}) {}",
                        s.atom,
                        s.consumer_cpv,
                        consumer_suffix,
                        render_pkg_use_display(&consumer_use),
                    );
                }
                // The `^` marker spans the operator + version only when the
                // missed package does not satisfy the version part.
                // Portage: output.py, highlight_version
                let version_ok = {
                    let cpv = format!(
                        "{}/{}-{}",
                        header.category, header.package, header.skipped_version
                    );
                    let base = portage_dep::without_use(&s.atom);
                    let base = base.split("::").next().unwrap_or(base);
                    let base = base.split(':').next().unwrap_or(base);
                    portage_dep::match_from_list(base, &[cpv.as_str()])
                        .is_some_and(|m| !m.is_empty())
                };
                println!(
                    "    {}",
                    skip_conflict_caret_line(
                        &s.atom,
                        &header.slot,
                        &header.skipped_sub_slot,
                        !version_ok
                    )
                );
            }
        }
        println!();
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
                show_conflict_instance(ShowConflictInstanceArgs {
                    inst,
                    c,
                    root,
                    repos,
                    config,
                    verbose_conflicts,
                    excluded,
                    useoldpkg_atoms_render,
                    usepkgonly,
                    installed_pkgs: &installed_pkgs,
                    need_rebuild: &mut need_rebuild,
                    color,
                    any_omitted: &mut any_omitted,
                });
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

/// The values [`show_conflict_instance`] reads from its caller.
pub(crate) struct ShowConflictInstanceArgs<'a> {
    pub(crate) inst: &'a SlotConflictInstance,
    pub(crate) c: &'a SlotConflict,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) verbose_conflicts: bool,
    pub(crate) excluded: &'a Vec<String>,
    pub(crate) useoldpkg_atoms_render: &'a [String],
    pub(crate) usepkgonly: bool,
    pub(crate) installed_pkgs: &'a HashSet<(String, String, String)>,
    pub(crate) need_rebuild: &'a mut Vec<(String, String)>,
    pub(crate) color: &'a Colorizer,
    pub(crate) any_omitted: &'a mut bool,
}

/// Print one instance of a slot conflict with the parents that pull it in.
pub(crate) fn show_conflict_instance(args: ShowConflictInstanceArgs<'_>) {
    let ShowConflictInstanceArgs {
        inst,
        c,
        root,
        repos,
        config,
        verbose_conflicts,
        excluded,
        useoldpkg_atoms_render,
        usepkgonly,
        installed_pkgs,
        need_rebuild,
        color,
        any_omitted: any_omitted_out,
    } = args;
    let mut any_omitted = *any_omitted_out;
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
                portage_repo::installed_pkg_iuse_and_use(root, &c.category, &c.package, &o.version)
            } else {
                slot_conflict_flag_sets(repos, config, &c.category, &c.package, &o.version)
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
        if p.parent_cpv.is_empty() && reasons.is_empty() && others.iter().any(|o| o.installed) {
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
    let mut selected = select_conflict_parents(SelectConflictParentsArgs {
        groups: &groups,
        verbose_conflicts,
        classified: &classified,
        excluded,
        useoldpkg_atoms_render,
        usepkgonly,
        repos,
        config,
        installed_pkgs,
        need_rebuild,
    });
    show_selected_parents(
        &mut selected,
        &classified,
        color,
        &classified_installed,
        root,
    );
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
    *any_omitted_out = any_omitted;
}

/// The values [`select_conflict_parents`] reads from its caller.
pub(crate) struct SelectConflictParentsArgs<'a> {
    pub(crate) groups: &'a [(SlotConflictReason, Vec<usize>)],
    pub(crate) verbose_conflicts: bool,
    pub(crate) classified: &'a [ClassifiedParent<'a>],
    pub(crate) excluded: &'a Vec<String>,
    pub(crate) useoldpkg_atoms_render: &'a [String],
    pub(crate) usepkgonly: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) installed_pkgs: &'a HashSet<(String, String, String)>,
    pub(crate) need_rebuild: &'a mut Vec<(String, String)>,
}

/// Choose the parents of a conflict instance to show, per reason group, and note the unrebuildable ones.
pub(crate) fn select_conflict_parents(args: SelectConflictParentsArgs<'_>) -> Vec<usize> {
    let SelectConflictParentsArgs {
        groups,
        verbose_conflicts,
        classified,
        excluded,
        useoldpkg_atoms_render,
        usepkgonly,
        repos,
        config,
        installed_pkgs,
        need_rebuild,
    } = args;
    // Representative selection: farthest version per `cp` for a
    // `version` group, the first member for a `slot` group, every
    // member of a `use` group, and every member under
    // `--verbose-conflicts`.
    // Portage: resolver/slot_collision.py
    let mut selected: Vec<usize> = Vec::new();
    for (reason, members) in groups {
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
                            let cur_ver = classified[*cur].atom.version.as_deref().unwrap_or("");
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
    for (reason, members) in groups {
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
                installed: installed_pkgs,
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
    selected
}

/// Print the selected parents of a conflict instance, unconditional USE dependencies first.
pub(crate) fn show_selected_parents(
    selected: &mut Vec<usize>,
    classified: &[ClassifiedParent<'_>],
    color: &Colorizer,
    classified_installed: &[bool],
    root: &Path,
) {
    selected.sort_unstable();
    selected.dedup();
    // `unconditional_use_deps` first (Portage's own set order is
    // nondeterministic; portuale keeps the stable classified
    // order within each half).
    selected.sort_by_key(|m| !classified[*m].use_unconditional);
    for &m in &*selected {
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
        println!("    (no parents that aren't satisfied by other packages in this slot)");
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
