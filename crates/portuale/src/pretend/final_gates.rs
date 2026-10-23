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

    // The ABI-rebuild report: grouped by the provider whose new sub-slot broke
    // each consumer's built `:=` link; each package renders as
    // `(cpv:slot/sub_slot::repo, type scheduled for merge[ to '<root>'])`.
    // Gated on `--verbose-slot-rebuilds != "n"` (default on, not `--verbose`).
    // Portage: depgraph.py, _show_abi_rebuild_info
    if verbose_slot_rebuilds && !result.abi_rebuilds.is_empty() {
        // cpv -> resolved (slot, sub_slot, repo, type) for every merge-bound
        // entry, so both ends render at their resolved slot.
        let mut by_cpv: HashMap<String, (String, String, String, &str, String)> = HashMap::new();
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
                    // A binary's build id follows its version.
                    // Portage: Package.py, __str__
                    e.build_id
                        .as_deref()
                        .map(|id| format!("-{id}"))
                        .unwrap_or_default(),
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
                Some((slot, sub_slot, repo, type_name, build)) => format!(
                    // match-str-ok: display line, not a match string
                    "({cpv}{build}:{slot}/{sub_slot}::{repo}, {type_name} scheduled for merge{root_suffix})"
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

    // The ignored-user-patches block (backlog #509): binaries the
    // `--binpkg-respect-user-patches` filter dropped for cps that merge.
    // Portage's `display()` prints it after the ABI rebuilds and before
    // the changed-deps report; suppressed under `--quiet` and under an
    // explicit `--binpkg-respect-user-patches=y|n`.
    // Portage: depgraph.py, _show_ignored_binaries_user_patches
    if !opts.quiet
        && opts.binpkg_respect_user_patches.is_none()
        && let Some(block) = portage_repo::render_ignored_user_patch_binaries(
            &result.ignored_user_patch_binaries,
            root,
            opts.verbose,
        )
    {
        eprint!("{block}");
    }

    // The `--changed-deps-report` WARN block, ported verbatim (plain text like
    // every other portuale message). Non-empty only when the flag was given
    // without `--changed-deps`, so no extra condition is needed here.
    // Portage: depgraph.py, _changed_deps_report
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

    // `--buildpkgonly` with unmerged dependencies fails after the merge list.
    // See `GraphResult::buildpkgonly_deps_unsatisfied`.
    // Portage: depgraph.py, display_problems
    if result.buildpkgonly_deps_unsatisfied {
        eprintln!("\n!!! --buildpkgonly requires all dependencies to be merged.");
        eprintln!("!!! Cannot merge requested packages. Merge deps and try again.\n");
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }

    // An unbreakable dependency cycle among merge-bound packages: printed to
    // stderr after the merge list, then the whole action fails (exit 1),
    // pretend or not -- Portage never proceeds to build a cycle. Aborts under
    // the abort-path gate never reach here with work left, so this serves
    // gate-off and Complete resolves only.
    // Portage: _emerge/depgraph.py, _show_circular_deps
    //
    // The `Change USE:` suggestion branch is shipped; fixtures `usecyclea`,
    // `gpcyclec` and `fucyclec` pin its outcomes.
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
                root,
                format: opts
                    .circular_deps_report
                    .unwrap_or(CircularDepsReport::Text),
                all_cycles: &result.report_cycles,
            });
        }
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }

    // An aborted resolve exits 1 without ever reaching the merge list. This
    // sits after the circular block because Portage's `display_problems()`
    // prints circular deps first. `mrg` needs no separate wiring: it hands its
    // argv to this same `pretend::run`, so output and exit codes are `emerge`'s.
    // Portage: _emerge/actions.py, action_build
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
    // Stage 5: the slot-operator/`forced rebuilds` DEBUG dump on stdout, right
    // before the "causing rebuilds" block. Only the edges that actually forced
    // a rebuild this run are listed (a documented fidelity gap: Portage lists
    // every installed `:=` edge).
    // Portage: depgraph.py, _compute_abi_rebuild_info
    if portage_repo::resolver_debug() {
        let root_str = root.display().to_string();
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
                    // match-str-ok: display line, not a match string
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
    // A blocked installed package the resolver could neither replace nor
    // unmerge fails the run (exit 1). The per-parent detail is a documented
    // cut; the sentence, the docs link and the exit code are what matters.
    // Portage: _emerge/depgraph.py, _show_unsatisfied_blockers
    let unsolvable_blockers = result
        .entries
        .iter()
        .any(|e| e.blockers.iter().any(|b| b.unsolvable))
        // An orphan row is unsolvable by construction, so it fails the run
        // exactly like an entry-owned one.
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

    // A recorded slot conflict fails the whole action (rc 1), gated on the data
    // -- never on "the notice was printed", so `-q`/`--json`/`--columns` cannot
    // change the rc. Escapes: `--autounmask-only` (returns 0 earlier), and
    // backtracking disabled plus `--buildpkgonly`/`--nodeps`.
    // Portage: _emerge/actions.py, action_build; depgraph.py, _accept_blocker_conflicts
    let backtracking_disabled = backtrack_max == 0;
    let slot_conflict_tolerated = backtracking_disabled && (buildpkgonly || nodeps);
    if !result.slot_conflicts.is_empty() && !slot_conflict_tolerated {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    // An unresolved `--prune`: display, then fail (no `prune_rebuilds` restart).
    // Portage: _emerge/depgraph.py, _resolve
    if result.prune_unresolved {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    None
}
