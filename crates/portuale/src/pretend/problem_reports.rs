//! Missing arguments, and the abort reports for cycles and unsatisfied dependencies.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// Missing arguments, and the abort reports for cycles and unsatisfied dependencies.
pub(crate) fn show_missing_args_and_aborts(
    result: &GraphResult,
    root: &Path,
    color: &Colorizer,
    repos: &[RepoConfig],
    config: &Config,
) {
    // Backlog #286: real `display_problems()`'s world-file tail
    // (`depgraph.py:11150-11190`) for the dropped set members
    // (`GraphResult::missing_args`) -- what real prints right after the
    // skipped-update block above. The world-file notice fires only for a
    // missing member of the world file itself (real's `atom in world_set`
    // gate: a `@system`/`@profile` member riding `@world` prints only
    // the list); the masked-or-don't-exist list names every missing arg
    // in requested order. Unlike the missed-update blocks above these
    // two carry no `--quiet` gate (real prints them outside
    // `_show_missed_update`); `--json` returns above, and `--columns`
    // has no gate (not merge-list rows).
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

    // Backlog #19 Slice 5: with the gate on, an aborted cycle prints its
    // circular block HERE -- before the autounmask section -- matching
    // real `display_problems()` order (`_show_circular_deps` at
    // `depgraph.py:11113`, `_display_autounmask` at `:11140`). Flow then
    // continues into the autounmask rendering below (USE block, notice,
    // exit 1), which is exactly the fourth-shape oracle (spec §4d). A
    // masked/unsat abort prints no circular block at all (real abandons
    // the walk before serialization ever runs); gate off keeps the
    // legacy position below.
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted {
            reason: portage_repo::AbortReason::UnserializableCycle { members: _ },
            ..
        } = &result.outcome
        && let Some(cycle) = result.circular_deps.first()
    {
        print_circular_block(
            cycle,
            &portage_repo::cycle_edge_labels(&result.entries, cycle),
            color,
            repos,
            config,
            &result.autounmask_use_changes,
            &result.entries,
            result.large_cycle_count,
        );
    }

    // Backlog #193: real `select_files`' `_virtual_cycle_error` handler
    // (`lib/_emerge/depgraph.py:5002-5013` in 3rdparty/portage 3.0.82.2)
    // prints `\n\n!!! virtual cycle detected:\n\n`, one
    // `  {cpv}::{repo}` line per probe-stack member (sorted), then `\n`
    // -- to stderr (`writemsg`, noiselevel -1), mid-resolution (between
    // real's `Calculating dependencies ...` notice halves; portuale
    // prints that notice whole in the preamble above, like the
    // masked/unsat arm). No merge list, no counters, no trailing error
    // block: real's run ends after the timing line (g193 probe), and the
    // shared abort gate below exits 1. Uncoloured like real's own
    // `writemsg` (no colour markup on that path).
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
pub(crate) fn show_skipped_missing_deps(quiet: bool, debug: bool, result: &GraphResult) {
    // Backlog #129 (S1): real `_show_missed_update_unsatisfied_dep`'s
    // abbreviated tail (`depgraph.py:1638-1649`) -- the slots whose
    // higher masked version lost to an unsatisfied dependency under
    // backtracking (`GraphResult::skipped_missing_deps`, in first-mask
    // order like the `WARNING` rows above). Real prefixes the section
    // with one blank line (its slot section already ends with one);
    // the `for <root>` suffix is omitted like every other portuale
    // notice row. Same `--quiet`-unless-`--debug` gate as the
    // `WARNING` block above (real drops both types together,
    // `depgraph.py:1576-1581`); likewise no `--columns` gate.
    if !(quiet && !debug) && !result.skipped_missing_deps.is_empty() {
        println!();
        println!("!!! The following update(s) have been skipped due to unsatisfied dependencies");
        println!("!!! triggered by backtracking:");
        println!();
        for s in &result.skipped_missing_deps {
            println!("{}/{}:{}", s.category, s.package, s.slot);
        }
    }

    // Backlog #198: real `_show_missed_update_unsatisfied_dep`'s full
    // per-update form (`depgraph.py:1592-1636`) -- one block per masked
    // higher version whose missing atom bites nothing backtrack-masked
    // (`GraphResult::skipped_missing_dep_full`, in the same first-mask
    // order). The `cat/pkg:slot` header, the `selected:` / `skipped:
    // ... (see unsatisfied dependency below)` rows and the missing
    // atom's own disclosure follow real line for line; the `for <root>`
    // suffix is omitted like every other portuale notice row. Same
    // `--quiet`-unless-`--debug` gate as the two blocks above (real
    // drops all missed-update types together, `depgraph.py:1576-1581`);
    // likewise no `--columns` gate, and `--json` returns above.
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
                        "!!! All ebuilds that could satisfy {:?} have been masked.",
                        report.atom
                    );
                    println!(
                        "!!! One of the following masked packages is required to complete your request:"
                    );
                    for (cpv, reasons) in &report.masked {
                        println!("- {cpv} (masked by: {})", reasons.join(", "));
                    }
                    println!();
                    for (node, ty) in &report.chain {
                        println!("(dependency required by \"{node}\" [{ty}])");
                    }
                    println!("For more information, see the MASKED PACKAGES section in the emerge");
                    println!("man page or refer to the Gentoo Handbook.");
                    println!();
                }
                portage_repo::SkippedDepDisclosure::Plain(report) => {
                    println!("emerge: there are no ebuilds to satisfy {:?}.", report.atom);
                    for (node, ty) in &report.chain {
                        println!("(dependency required by \"{node}\" [{ty}])");
                    }
                    println!();
                }
            }
        }
    }
}

/// The values [`show_skipped_updates`] reads from its caller.
pub(crate) struct ShowSkippedUpdatesArgs<'a> {
    pub(crate) quiet: bool,
    pub(crate) debug: bool,
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) running_config: &'a Option<Config>,
    pub(crate) repos: &'a [RepoConfig],
}

/// Updates skipped by slot conflicts or masks.
pub(crate) fn show_skipped_updates(args: ShowSkippedUpdatesArgs<'_>) {
    let ShowSkippedUpdatesArgs {
        quiet,
        debug,
        result,
        root,
        root_deps_running_root,
        running_config,
        repos,
    } = args;
    // Backlog #90 (S2) + #92, #129 review round 2: real
    // `_conflict_missed_update["slot conflict"]`
    // (`depgraph.py:2063-2106`) -- `WARNING: One or more
    // updates/rebuilds have been skipped due to a dependency
    // conflict:` plus one `conflicts with` block per missed upgrade,
    // printed after the merge list, rc 0. Real keeps one entry per
    // `(root, slot_atom)` with the whole `(parent, atom)` set inside
    // (`_get_missed_updates`, `:1553-1562`); portuale's rows are one
    // per rejecting parent, so rows naming the same missed pkg group
    // into one block (`portage_repo::group_skipped_updates`). Two
    // producers, one shape: the direct solve's removed instances and
    // the reverse-pin withholds (`GraphResult::skipped_updates`; the
    // renderer cannot tell them apart and real does not distinguish
    // them either). Backlog #230 renders each package with real's full
    // `pkg_use_display` (`skipped_update_use_display_for` /
    // `skipped_update_installed_use_display_for`, reusing the
    // slot-collision notice's `render_pkg_use_display` string form):
    // the whole effective USE masked to the valid-IUSE domain, one
    // `VAR="…"` group per non-hidden `USE_EXPAND` var, `( )`-wrapped
    // force/mask flags. Backlog #242 Slice D: destinations follow real's
    // per-node rule (`skipped_node_suffix` — quoted `to '<root>'` iff
    // the node's OWN root is not `/`), and each side paints from its own
    // root's profile (the running `Config` when the row rides the
    // running root, recomputed here from the shared repos; single-root
    // runs keep the resolve-time target paint). An installed consumer
    // renders `(cpv, installed in '<root>')` with its own root, like the
    // notice's installed instances. Real's own rendering pairs a bare
    // missed line with `to`-suffixed parents exactly when the missed
    // package sits in the running-root (`/`) tree while the parents sit
    // in the target (the bed blk0 shape with the running pin dropped);
    // under `fixture_env`'s single root every node is target-rooted, so
    // the missed line and the parents all carry the fixture `to` suffix
    // with fixture-profile paint — deterministic, and the contract pin
    // records it as such. The `^` marker line
    // mirrors real `format_unmatched_atom`'s operator + version spans
    // plus a mismatched `:slot[/sub-slot]` span (USE-token spans stay a
    // documented cut -- no grounded case carries USE-deps here). A
    // top-level `(Argument)` parent (empty `consumer_cpv`: a CLI atom
    // that accepts only the surviving instance) renders as real's bare
    // indented arg line with no atom and no marker
    // (`depgraph.py:1681-1686`). Suppressed under `--quiet` unless
    // `--debug` -- real `_show_missed_update` drops both notice types
    // then (`depgraph.py:1576-1581`). `--json` never reaches this block
    // (it returns above); `--columns` has no gate (real shows the
    // notices regardless of columns: they are not merge-list rows).
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
            // Backlog #242 Slice D: per-node destination (quoted, iff the
            // missed node's own root is not `/`) and own-root USE paint
            // (recomputed from the shared repos under the running profile
            // when the miss rides the running root).
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
                    // Real's `PackageArg`/`AtomArg` arm
                    // (`depgraph.py:1681-1686`): the bare command-line
                    // argument, no atom and no `^` marker. `s.atom`
                    // already IS that argument here: the only producer
                    // that emits empty-`consumer_cpv` rows is the
                    // direct-solve path, and its empty-cpv parents come
                    // solely from top-level argument pullers, whose
                    // recorded atom is the argument text as typed
                    // (unit-pinned by portage-repo's
                    // `direct_solve_reports_an_argument_parent_with_the_cli_text`;
                    // the reverse-pin producer always builds a
                    // non-empty consumer, the backtrack-mask rows skip
                    // empty pullers). No contract or bed cell grounds
                    // this arm end to end (blk0 parents are all
                    // Packages), so beyond that producer pin it stays
                    // unpinned.
                    println!("    {}", s.atom);
                    continue;
                }
                if s.consumer_installed {
                    // Backlog #242 Slice D: the installed root is the
                    // consumer's own (`in '<root>'` with the running path
                    // when the consumer rides it — always the target walk
                    // from every producer today, so the flag is false and
                    // this matches the old rendering exactly).
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
                    // Backlog #242 Slice D: per-node destination + own-root
                    // USE paint, like the missed line above. The consumer
                    // cpv re-derives `(cat, pkg, ver)` for the recompute;
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
                // #270: real marks the operator + version only when the
                // missed package does not satisfy the version part
                // (`highlight_version`, `output.py:925-929`: the atom
                // without repo, slot and USE against the package).
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
    pub(crate) deferred_abort_unsat: &'a [usize],
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
}

/// Unsatisfied dependencies whose report waited for the merge list.
pub(crate) fn show_deferred_unsat(args: ShowDeferredUnsatArgs<'_>) {
    let ShowDeferredUnsatArgs {
        deferred_abort_unsat,
        result,
        root,
        top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        columnwidth,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
    } = args;
    if !deferred_abort_unsat.is_empty() {
        let mut thrown_away: Vec<String> = Vec::new();
        for &i in deferred_abort_unsat {
            print_entry_line(
                &result.entries,
                root,
                i,
                "",
                true,
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                root_deps_running_root.as_deref(),
                color,
                system_atoms,
                world_atoms,
                force_reinstall_cps,
                &mut thrown_away,
                &result.masked_deps,
                &result.use_unsat_deps,
                &result.plain_miss_deps,
                false,
                &result.circular_dependency,
            );
        }
    }
}

/// The values [`show_slot_conflicts`] reads from its caller.
pub(crate) struct ShowSlotConflictsArgs<'a> {
    pub(crate) result: &'a GraphResult,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) verbose_conflicts: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) useoldpkg_atoms_render: &'a [String],
    pub(crate) usepkgonly: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) backtrack_max: u32,
}

/// The slot-conflict notice.
pub(crate) fn show_slot_conflicts(args: ShowSlotConflictsArgs<'_>) {
    let ShowSlotConflictsArgs {
        result,
        root,
        repos,
        config,
        verbose_conflicts,
        excluded,
        useoldpkg_atoms_render,
        usepkgonly,
        color,
        backtrack_max,
    } = args;
    // Real `depgraph._show_slot_collision_notice` -> `slot_conflict_handler.
    // get_conflict()` (`lib/_emerge/resolver/slot_collision.py`): the
    // `!!! Multiple package instances within a single package slot ...`
    // block, then the advisory paragraph. Ported: the preamble, the
    // per-instance `(<cpv>, ebuild scheduled for merge) USE="" pulled in
    // by` line, real `_prepare_conflict_msg_and_check_for_specificity`'s
    // `collision_reasons` grouping + one-representative-per-reason
    // selection (every parent under `--verbose-conflicts`), the
    // `highlight_violations` `^` marker line (operator, version, slot,
    // and USE-token spans, wrapped in `BAD` red under `--color y` with
    // markers kept aligned on the displayed string -- real marks
    // pre-color indices and drifts, a genuine upstream bug portuale
    // deliberately does not reproduce), the `(and N more with the
    // same problem[s])` tail, the `NOTE: Use the '--verbose-conflicts'
    // option ...` footer, the `need_rebuild` "cannot be rebuilt" trailer,
    // and the advisory (with the `--backtrack=30` hint gated the real
    // way: shown unless `--backtrack` is >=30 or 0). Documented cuts
    // (fixtures don't exercise them): the `soname` reason key (no soname
    // atom can reach this renderer) and an `=*` operator's `("version",
    // None)` key. The one deliberate divergence in this block: real
    // drifts its `^` markers once ANSI codes lengthen the line under
    // `--color y` (genuine upstream bug); portuale wraps the same spans
    // but keeps the markers aligned. The block is display-only: the
    // exit-code consequence lives below in the "display, then fail the
    // action" band next to `unsolvable_blockers` (real `actions.py:
    // 460-462` -- a slot conflict fails the run with rc 1, `--pretend`
    // included; see that gate's own comment for the two escapes).
    if !result.slot_conflicts.is_empty() {
        let mut any_omitted = false;
        // Real `need_rebuild`: installed parents with built slot-operator
        // atoms that cannot be rebuilt, in first-seen order, printed once
        // after all conflicts (real keeps a dict across the whole
        // handler).
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
                    // Real's installed nomerge node: `(cpv, installed in
                    // '<root>')` (see `build_residual_slot_conflicts`).
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
                // Whether a bare `(Argument)` parent shows: real files one
                // under a reason only when the *other* package is already
                // installed -- always the case for a residual record's
                // installed side, never for merge-vs-merge records (whose
                // instances are all scheduled for merge, so such a parent
                // contributes no specific reason and is not shown).
                let show_argument_parents = c
                    .instances
                    .iter()
                    .any(|o| !std::ptr::eq(o, inst) && o.installed);
                // Real compares against every *other* instance in the
                // slot (`slot_collision.py`: `if other_pkg == pkg:
                // continue`) -- identity, not version: a same-version
                // merge/installed twin (a USE-only break, backlog #222)
                // still sees its twin, so the USE parent classifies.
                let others: Vec<ConflictOther> = c
                    .instances
                    .iter()
                    .filter(|o| !std::ptr::eq(*o, inst))
                    .map(|o| {
                        // Flag sets for the `("use", flag)` branch: an
                        // installed other's own vdb-recorded sets, like
                        // the `use_display` on its header line.
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
                // (parent_cpv, atom, display, installed) for the selected
                // line rendering below -- an installed parent renders
                // `required by (<cpv>, installed in '<root>')`.
                let mut classified_installed: Vec<bool> = Vec::new();
                for p in &inst.parents {
                    if p.parent_cpv.is_empty() && !show_argument_parents {
                        continue;
                    }
                    let Some(atom) = parse_atom(&p.atom) else {
                        continue;
                    };
                    let (mut reasons, use_unconditional) = slot_conflict_reasons(&atom, &others);
                    // Real `slot_collision.py`'s `("AtomArg", None)` key
                    // (`elif isinstance(ppkg, AtomArg) and
                    // other_pkg.installed`): a bare command-line parent
                    // satisfied by every other instance is still shown
                    // once some other instance is installed. Like real's
                    // `elif`, it only fires when no version/slot/USE
                    // reason did (so an argument parent that already
                    // classifies, like pg1's `>=cgp1x-2`, renders exactly
                    // once, as today).
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
                // Representative selection: farthest version per `cp` for
                // a `version` group, the first member for a `slot` group,
                // every member of a `use` group (real shows all broken
                // reverse dependencies -- "people can simply use a pager"),
                // and every member under `--verbose-conflicts`.
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
                // Real's `need_rebuild` scan runs over every `slot`-group
                // parent (verbose or not): an unrebuildable parent is
                // always shown, and recorded for the once-per-notice
                // trailer below.
                for (reason, members) in &groups {
                    if !matches!(reason, SlotConflictReason::Slot(_)) {
                        continue;
                    }
                    for &m in members {
                        let p = &classified[m];
                        if let Some(why) = slot_conflict_need_rebuild(
                            p.parent_cpv,
                            &p.atom,
                            excluded,
                            useoldpkg_atoms_render,
                            usepkgonly,
                            repos,
                            config,
                            &installed_pkgs,
                        ) {
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
                // Real shows `unconditional_use_deps` first (its own set
                // order is nondeterministic; portuale keeps the stable
                // classified order within each half).
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
                    // Real `highlight_violations` wraps the violated
                    // spans in `BAD` red here (`--color y` only; plain
                    // otherwise) -- and then marks the PRE-color
                    // indices, so its `^` line drifts once ANSI codes
                    // lengthen the line (genuine upstream bug: the
                    // markers land left of their visible targets, and
                    // real's own red varies by terminal colormap, so
                    // byte-parity with the drift is unachievable
                    // anyway). Portuale wraps the same spans but marks
                    // the DISPLAYED string, so color and carets agree.
                    let (atom_display, shifted) = colorize_marked_spans(p.atom_str, &idx, color);
                    // A bare command-line parent renders `<atom>
                    // (Argument)` with no marker line (real shows no `^`
                    // under one either); an installed parent names its
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
        // Real `slot_collision.py`'s `need_rebuild` trailer: installed
        // parents with built slot-operator atoms that cannot be rebuilt.
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
        // Real gates the hint on the reported depgraph's own
        // `not _allow_backtracking` too (`depgraph.py:1750-1759`): a loop
        // ended early by `need_config_change` reports a pass that still
        // allowed backtracking (backlog #244).
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
    pub(crate) show_merge_list: bool,
    pub(crate) result: &'a GraphResult,
    pub(crate) skip_redisplay: bool,
    pub(crate) display_entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
    pub(crate) root_deps_running_root: &'a Option<PathBuf>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
}

/// The circular-dependency block shown again after the merge list.
pub(crate) fn show_cycle_redisplay(args: ShowCycleRedisplayArgs<'_>) {
    let ShowCycleRedisplayArgs {
        show_merge_list,
        result,
        skip_redisplay,
        display_entries,
        root,
        top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        columnwidth,
        root_deps_running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
    } = args;
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
            print_entry_line(
                display_entries,
                root,
                idx,
                "",
                true,
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                root_deps_running_root.as_deref(),
                color,
                system_atoms,
                world_atoms,
                force_reinstall_cps,
                &mut thrown_away,
                &result.masked_deps,
                &result.use_unsat_deps,
                &result.plain_miss_deps,
                // The legacy (gate-off) re-display keeps the user-flagged
                // rendering it always had; the forced-verbose display is
                // the gated path's main list above (whose re-display is
                // skipped as duplicative).
                false,
                &result.circular_dependency,
            );
        }
    }
}
