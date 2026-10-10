//! The last gates before execution: ABI rebuilds, changed-deps report, buildpkgonly deps, circular dependencies.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`final_gates`] reads from its caller.
pub(crate) struct FinalGatesArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) result: &'a GraphResult,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) color: &'a Colorizer,
}

/// The last gates before execution: ABI rebuilds, changed-deps report, buildpkgonly deps, circular dependencies.
pub(crate) fn final_gates(args: FinalGatesArgs<'_>) -> Option<ExitCode> {
    let FinalGatesArgs {
        opts,
        result,
        entries,
        root,
        repos,
        config,
        color,
    } = args;
    let &EmergeOpts {
        verbose_slot_rebuilds,
        pretend,
        ..
    } = opts;
    // Real `_show_abi_rebuild_info` (`depgraph.py:1210`), gated on
    // `--verbose-slot-rebuilds != "n"` (default on, NOT `--verbose`) and
    // rendered right after the merge list / autounmask blocks, before the
    // changed-deps report -- `_forced_rebuilds[root][child] = {parents}`,
    // grouped by the provider whose new sub-slot broke each consumer's
    // built `:=` link. `writemsg_stdout`, so stdout. Real prints each
    // package as `str(Package)` (`Package.__str__`):
    // `(cpv:slot/sub_slot::repo, ebuild scheduled for merge[ to '<root>'])`.
    if verbose_slot_rebuilds && !result.abi_rebuilds.is_empty() {
        // cpv -> (slot, sub_slot, repo, type_name) for every merge-bound
        // entry, so both sides of each edge render at their *resolved*
        // slot (the rebuild's new sub-slot, not the vdb's).
        let mut by_cpv: HashMap<String, (String, String, String, &str)> = HashMap::new();
        for e in entries {
            let version = match &e.outcome {
                portage_repo::PretendOutcome::New { version }
                | portage_repo::PretendOutcome::Reinstall { version, .. } => version.clone(),
                portage_repo::PretendOutcome::Upgrade { to, .. }
                | portage_repo::PretendOutcome::Downgrade { to, .. } => to.clone(),
                _ => continue,
            };
            let type_name = match e.source {
                portage_repo::CandidateSource::Binary => "binary",
                portage_repo::CandidateSource::Ebuild => "ebuild",
            };
            by_cpv.insert(
                format!("{}/{}-{version}", e.category, e.package),
                (
                    e.slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.repo_name.clone().unwrap_or_default(),
                    type_name,
                ),
            );
        }
        let root_suffix = if root == Path::new("/") {
            String::new()
        } else {
            format!(" to '{}'", root.display())
        };
        let render = |cpv: &str| -> String {
            match by_cpv.get(cpv) {
                Some((slot, sub_slot, repo, type_name)) => format!(
                    "({cpv}:{slot}/{sub_slot}::{repo}, {type_name} scheduled for merge{root_suffix})"
                ),
                None => cpv.to_string(),
            }
        };
        println!();
        println!("The following packages are causing rebuilds:");
        println!();
        let mut provider: Option<&str> = None;
        for (child, parent) in &result.abi_rebuilds {
            if provider != Some(child.as_str()) {
                println!("  {} causes rebuilds for:", render(child));
                provider = Some(child.as_str());
            }
            println!("    {}", render(parent));
        }
    }

    // `--changed-deps-report`: real `_changed_deps_report`'s own WARN
    // block, ported verbatim (real portage colorizes it when the
    // terminal supports it; portuale, like every other message it
    // prints, stays plain text). Already empty unless `changed_deps_report`
    // was given AND `changed_deps` was NOT (see `resolve_pretend_graph`'s
    // own doc comment for that gating), so no extra condition needed
    // here beyond "is there anything to report at all".
    if !result.changed_deps_report.is_empty() {
        eprintln!();
        eprintln!("!!! Detected ebuild dependency change(s) without revision bump:");
        eprintln!();
        for c in &result.changed_deps_report {
            if root == Path::new("/") {
                eprintln!(
                    "    {}/{}-{}::{}",
                    c.category, c.package, c.version, c.repo_name
                );
            } else {
                eprintln!(
                    "    {}/{}-{}::{} for {}",
                    c.category,
                    c.package,
                    c.version,
                    c.repo_name,
                    root.display()
                );
            }
        }
        eprintln!();
        eprintln!("NOTE: Refer to the following page for more information about dependency");
        eprintln!("      change(s) without revision bump:");
        eprintln!();
        eprintln!("          https://wiki.gentoo.org/wiki/Project:Portage/Changed_dependencies");
        eprintln!();
        eprintln!("      In order to suppress reports about dependency changes, add");
        eprintln!("      --changed-deps-report=n to the EMERGE_DEFAULT_OPTS variable in");
        eprintln!("      '/etc/portage/make.conf'.");
        eprintln!();
        eprintln!("HINT: In order to avoid problems involving changed dependencies, use the");
        eprintln!("      --changed-deps option to automatically trigger rebuilds when changed");
        eprintln!("      dependencies are detected. Refer to the emerge man page for more");
        eprintln!("      information about this option.");
    }

    // Real depgraph.py's own display_problems(): shown *after* the merge
    // list above (real `_show_merge_list()` runs first), then the whole
    // action fails -- see GraphResult::buildpkgonly_deps_unsatisfied's
    // own doc comment for the exact real check this mirrors.
    if result.buildpkgonly_deps_unsatisfied {
        eprintln!("\n!!! --buildpkgonly requires all dependencies to be merged.");
        eprintln!("!!! Cannot merge requested packages. Merge deps and try again.\n");
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }

    // Real `_serialize_tasks` -> `_show_circular_deps` (`depgraph.py:
    // 10425`): an unbreakable dependency cycle among the merge-bound
    // packages -- every edge in it one real portage's `_ignore_runtime`
    // scan can't linearize (an unsatisfied `DEPEND`/`BDEPEND` with no
    // run-time alternative, or since backlog #228 an unsatisfied
    // slot-operator edge). Printed to stderr after the merge list, then
    // the whole action fails (exit 1), pretend or not -- real portage
    // never proceeds to build a cycle.
    //
    // Faithful transcription of `_show_circular_deps`'s `writemsg`
    // sequence: the reduced cycle-only re-display
    // (`self.display(handler.merge_list)`, printed above between the
    // merge list and this block -- flat portuale lines, since portuale's
    // tree model dedups shared nodes and never abandons the list) and
    // the `large_cycle_count` "a lot of cycles" trailer below (shown
    // only with a concrete suggestion, like real). The `Change USE:`
    // suggestion branch itself is shipped
    // (`portage_repo::circular_dep_solutions`, real
    // `circular_dependency_handler._find_suggestions`): fixtures
    // `usecyclea` (bare suggestion), `gpcyclec` (hard grandparent clash
    // disqualifies it), and `fucyclec` (conditional grandparent clash
    // keeps it with `followup_change`) pin all three outcomes.
    // The shortest cycle drives `_prepare_circular_dep_message`; every
    // line after `depends on` carries its own edge's `priorities[-1]`
    // label (backlog #228 -- `(buildtime)` on every line for the
    // all-buildtime cycles, `slopcyca` pins the mixed shape).
    // Slice 5 routing for the legacy site: gate-on aborts never reach
    // here with work left (a cycle abort printed above, before the
    // autounmask section; a masked/unsat abort prints no circular block
    // at all), so this serves gate-off and Complete resolves only — the
    // pre-Slice-5 print-and-exit-1 shape, byte-identical.
    let gated_abort = portage_repo::abort_path_enabled()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted { .. }
        );
    if let Some(cycle) = result.circular_deps.first() {
        if !gated_abort {
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
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }

    // Backlog #19 abort-path outcome mapping (real `_emerge/actions.py:
    // 460-462`: `not success` -> `display_problems()`, `return 1` — the
    // merge list `display()` is never reached, so an aborted resolve
    // shows at most the partial list and always exits 1). Gated on
    // `portage_repo::abort_path_enabled` (`PORTUALE_ABORT_PATH=0` keeps
    // the legacy "report, don't enforce" list + exit 0 — Gate G0.4).
    // Since Slice 3 the resolver produces `Aborted` (see
    // `portage_repo::abort_outcome`), so this fires for a masked-only or
    // otherwise unsatisfiable dependency of a merge-bound package and for
    // an unserializable cycle; it sits *after* the circular block above
    // because real's `display_problems()` prints `_show_circular_deps`
    // first (`depgraph.py:11113`) and only then returns 1. Slice 4 moves
    // the partial-list rendering here. `mrg` needs no separate wiring:
    // `to_emerge_argv` hands its argv to this same `pretend::run`
    // (`mrg.rs`), so resolution output and exit codes are literally
    // `emerge`'s by construction.
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted { .. } = &result.outcome
    {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    None
}

/// The `--debug` resolver trace stages printed after the merge list.
pub(crate) fn print_resolver_debug(
    root: &Path,
    entries: &Vec<GraphEntry>,
    reinstall_atoms: &[String],
    result: &GraphResult,
) {
    // `emerge --pretend --debug` Stage 5: real
    // `_compute_abi_rebuild_info`'s own DEBUG-level dump (real
    // `depgraph.py:1011-1206`), on stdout, right before the
    // `_show_abi_rebuild_info` "causing rebuilds" block. Real's
    // `_slot_operator_deps` / `_forced_rebuilds` are richer dicts than
    // portuale's flat `abi_rebuilds` `(provider-cpv, consumer-cpv)`
    // list -- portuale emits the same shapes from what it has (a
    // documented fidelity gap: real's `slot operator dependencies:`
    // lists *every* installed `:=` edge, portuale only the ones that
    // actually forced a rebuild this run).
    if portage_repo::resolver_debug() {
        let root_str = root.display().to_string();
        // cpv -> (cp, slot, sub_slot, repo, type_name) for every
        // merge-bound entry (both ends of each abi-rebuild edge are
        // Reinstall/Upgrade). Same shape the "causing rebuilds" block
        // below builds.
        let mut by_cpv: HashMap<String, (String, String, String, String, &str)> = HashMap::new();
        for e in entries {
            let version = match &e.outcome {
                portage_repo::PretendOutcome::New { version }
                | portage_repo::PretendOutcome::Reinstall { version, .. } => version.clone(),
                portage_repo::PretendOutcome::Upgrade { to, .. }
                | portage_repo::PretendOutcome::Downgrade { to, .. } => to.clone(),
                _ => continue,
            };
            let ty = match e.source {
                portage_repo::CandidateSource::Binary => "binary",
                portage_repo::CandidateSource::Ebuild => "ebuild",
            };
            by_cpv.insert(
                format!("{}/{}-{version}", e.category, e.package),
                (
                    format!("{}/{}", e.category, e.package),
                    e.slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.repo_name
                        .clone()
                        .unwrap_or_else(|| "__unknown__".to_string()),
                    ty,
                ),
            );
        }
        let label = |cpv: &str| -> String {
            match by_cpv.get(cpv) {
                Some((_, slot, sub_slot, repo, ty)) => {
                    format!("({cpv}:{slot}/{sub_slot}::{repo}, {ty} scheduled for merge)")
                }
                None => format!("({cpv}, installed)"),
            }
        };

        println!("forced reinstall atoms:");
        if !reinstall_atoms.is_empty() {
            println!("   root: {root_str}");
            for atom in reinstall_atoms {
                println!("      atom: {atom}");
            }
        }
        print!("\n\n");

        println!("slot operator dependencies:");
        // Group abi-rebuild pairs by the provider's `cp:slot` slot-atom.
        let mut by_slot_atom: std::collections::BTreeMap<String, Vec<&(String, String)>> =
            std::collections::BTreeMap::new();
        for pair in &result.abi_rebuilds {
            let slot_atom = match by_cpv.get(&pair.0) {
                Some((cp, slot, ..)) => format!("{cp}:{slot}"),
                None => pair.0.clone(),
            };
            by_slot_atom.entry(slot_atom).or_default().push(pair);
        }
        for (slot_atom, pairs) in &by_slot_atom {
            println!("   ({root_str}, {slot_atom})");
            for (provider, consumer) in pairs {
                println!("      parent: {}", label(consumer));
                println!("        child: {} (runtime_slot_op)", label(provider));
            }
        }
        print!("\n\n");

        println!("forced rebuilds:");
        let mut by_child: std::collections::BTreeMap<&str, Vec<&str>> =
            std::collections::BTreeMap::new();
        for (provider, consumer) in &result.abi_rebuilds {
            by_child.entry(provider).or_default().push(consumer);
        }
        if !by_child.is_empty() {
            println!("   root: {root_str}");
            for (child, parents) in &by_child {
                println!("      child: {}", label(child));
                for parent in parents {
                    println!("         parent: {}", label(parent));
                }
            }
        }
        print!("\n\n");
    }
}

/// The values [`failure_gates`] reads from its caller.
pub(crate) struct FailureGatesArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) result: &'a GraphResult,
    pub(crate) show_merge_list: bool,
    pub(crate) color: &'a Colorizer,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
}

/// Unsolvable blockers, unresolved slot conflicts with backtracking off, and an unresolved --prune.
pub(crate) fn failure_gates(args: FailureGatesArgs<'_>) -> Option<ExitCode> {
    let FailureGatesArgs {
        opts,
        result,
        show_merge_list,
        color,
        repos,
        root,
        config,
    } = args;
    let &EmergeOpts {
        pretend,
        backtrack_max,
        buildpkgonly,
        nodeps,
        ..
    } = opts;
    // Real `_serialize_tasks` -> `_show_unsatisfied_blockers` +
    // `show_blocker_docs_link` (`depgraph.py:10496`): a blocked installed
    // package the resolver could neither replace nor unmerge is an
    // unresolved conflict -- real prints the `* Error: The above package
    // list …` block and `actions.py` returns 1. Portuale's per-parent
    // `pulled in by` detail is a documented cut (it needs real's
    // `_parent_atoms` / complete graph); the sentence, the docs link and
    // the exit code are what a `$?` check and the `Conflict: N blocks
    // (M unsatisfied)` line depend on.
    let unsolvable_blockers = result
        .entries
        .iter()
        .any(|e| e.blockers.iter().any(|b| b.unsolvable))
        // Backlog #80: an orphan row is unsolvable by construction
        // (`file_blocker_conflicts` only orphans `unsolvable` rows), so
        // it fails the run exactly like an entry-owned one -- real
        // prints the same `* Error` block for the n1b shape.
        || !result.orphan_blockers.is_empty();
    if unsolvable_blockers && show_merge_list {
        eprintln!();
        for line in [
            "Error: The above package list contains packages which cannot be",
            "installed at the same time on the same system.",
        ] {
            eprintln!(" {} {line}", color.c("BAD", "*"));
        }
        eprintln!(
            "\nFor more information about {}, please refer to the following",
            color.c("BAD", "Blocked Packages")
        );
        eprintln!("section of the Gentoo Linux x86 Handbook (architecture is irrelevant):\n");
        eprintln!("https://wiki.gentoo.org/wiki/Handbook:X86/Working/Portage#Blocked_packages\n");
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }

    // Backlog #62: a recorded slot conflict fails the whole action with
    // rc 1, matching real Portage 3.0.82.2. Real's rule, verified against
    // the source and host-side oracles before coding (the obvious "notice
    // printed => rc 1" formulation is wrong in the escape cases, and the
    // first draft of the plan had it wrong too):
    //
    // - The notice itself is unconditional on the conflict existing
    //   (`display_problems` prints it whenever
    //   `any(_package_tracker.slot_conflicts())`, `depgraph.py:
    //   11117-11120`); it says nothing about the exit code.
    // - The exit code comes from `action_build`'s `if not success:
    //   display_problems(); return 1` (`actions.py:460-462`), and
    //   `success` is False exactly when a conflict is recorded and not
    //   tolerated (`_resolve` tail, `depgraph.py:5685-5691`).
    // - Escape 1 -- `--autounmask-only` returns 0 first (`actions.py:
    //   456-458`), notice printed and all. Preserved for free: this gate
    //   sits after that early return above. Host oracle: the
    //   needer/othermod triangle with `--autounmask-only` prints the
    //   notice and exits 0.
    // - Escape 2 -- `_accept_blocker_conflicts()` (`depgraph.py:
    //   9259-9271`): backtracking disabled plus one of `--buildpkgonly`,
    //   `--fetchonly`, `--fetch-all-uri`, `--nodeps` tolerates the
    //   conflict. Host oracles: `--backtrack=0 --nodeps` and
    //   `--backtrack=0 --buildpkgonly` on the triangle exit 0 (in fact
    //   real records no conflict at all there -- `--nodeps` never walks
    //   the deps that would pull it in, `--buildpkgonly` resolves the
    //   shape without the upgrade -- so no "notice with rc 0" shape was
    //   reproducible; the gate still matters because portuale's resolver
    //   *does* record the conflict there). `--fetchonly`/
    //   `--fetch-all-uri` are still portuale-unimplemented options (rc 2
    //   before reaching here), so they need no parsed flag yet -- extend
    //   this condition when that surface lands. `--fetchonly` shapes
    //   could not be oracled hermetically anyway: real attempts the fetch
    //   phase past resolution and fails on the fixture's unexpanded
    //   `file://${PORTAGE_CONFIGROOT}` binhost URL (rc 1 even with no
    //   conflict anywhere).
    //
    // Gated on the data (`!result.slot_conflicts.is_empty()`), never on
    // "the notice was printed", so `-q`/`--json`/`--columns` cannot change
    // the rc -- `--json` has no real counterpart (real rejects it, rc 2)
    // and follows the same rule for consistency. The blocker path is
    // untouched: its `all satisfied` -> rc 0 / `unsolvable` -> rc 1 split
    // already matches real.
    let backtracking_disabled = backtrack_max == 0;
    let slot_conflict_tolerated = backtracking_disabled && (buildpkgonly || nodeps);
    if !result.slot_conflicts.is_empty() && !slot_conflict_tolerated {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    // Backlog #295: the best-run re-pass could not take the `prune_rebuilds`
    // restart real wants (`depgraph.py:5763-5775`) -- display, then fail.
    if result.prune_unresolved {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    None
}
