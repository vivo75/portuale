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
