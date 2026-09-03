//! The circular-dependency block.

use crate::pretend::*;

/// Arguments of [`circular_node_text`].
pub(crate) struct CircularNodeTextArgs<'a> {
    pub(crate) cpv: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) color: &'a Colorizer,
}

/// Portage's `Package.__str__` for a merge-bound package -- the
/// `(cpv:slot/sub_slot::repo, <type> scheduled for merge)` node text.
/// Cuts: the ` to '<ROOT>'` arm (every pinned cycle forms in `/`, so the
/// gate stays shut) and the `installed`/`uninstall` variants (members are
/// merge-bound by construction). A member missing from `entries` falls back
/// to the bare cpv at the call site.
/// Portage: resolver/circular_dependency.py
pub(crate) fn circular_node_text(args: CircularNodeTextArgs<'_>) -> String {
    let CircularNodeTextArgs {
        cpv,
        slot,
        sub_slot,
        repo,
        type_name,
        color,
    } = args;
    let head = format!("{cpv}:{slot}/{sub_slot}::{repo}");
    let head = match type_name {
        "binary" => color.c("PKG_BINARY_MERGE", &head),
        _ => color.c("PKG_MERGE", &head),
    };
    format!("({head}, {type_name} scheduled for merge)")
}

/// Arguments of [`print_circular_block`].
pub(crate) struct PrintCircularBlockArgs<'a> {
    pub(crate) cycle: &'a [String],
    pub(crate) edge_labels: &'a [String],
    pub(crate) color: &'a Colorizer,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) autounmask_use_changes: &'a [portage_repo::AutounmaskChange],
    pub(crate) entries: &'a [portage_repo::GraphEntry],
    pub(crate) large_cycle_count: bool,
}

/// Portage's `_show_circular_deps`: the `* Error: circular dependencies:`
/// block, the `Change USE:` suggestion-or-advisory branch, and the
/// `large_cycle_count` trailer. Prints only -- call sites return the exit.
/// Portage: depgraph.py, _show_circular_deps
pub(crate) fn print_circular_block(args: PrintCircularBlockArgs<'_>) {
    let PrintCircularBlockArgs {
        cycle,
        edge_labels,
        color,
        repos,
        config,
        autounmask_use_changes,
        entries,
        large_cycle_count,
    } = args;
    let prefix = color.c("BAD", " * ");
    eprint!("\n\n\n{prefix}Error: circular dependencies:\n\n");
    // `<node> depends on`, then each subsequent `<node> (<priorities[-1]>)`
    // at a growing indent, closing back on the first package. A member
    // missing from `entries` keeps the bare cpv so the block still prints.
    let mut by_cpv: HashMap<String, (String, String, String, &str)> = HashMap::new();
    for e in entries {
        let version = match &e.outcome {
            portage_repo::PretendOutcome::New { version }
            | portage_repo::PretendOutcome::Reinstall { version, .. } => version.clone(),
            portage_repo::PretendOutcome::Upgrade { to, .. }
            | portage_repo::PretendOutcome::Downgrade { to, .. } => to.clone(),
            _ => continue,
        };
        let (Some(slot), Some(sub_slot), Some(repo)) =
            (e.slot.clone(), e.sub_slot.clone(), e.repo_name.clone())
        else {
            continue;
        };
        let type_name = match e.source {
            portage_repo::CandidateSource::Binary => "binary",
            portage_repo::CandidateSource::Ebuild => "ebuild",
        };
        by_cpv.insert(
            format!("{}/{}-{version}", e.category, e.package),
            (slot, sub_slot, repo, type_name),
        );
    }
    let node = |cpv: &str| -> String {
        match by_cpv.get(cpv) {
            Some((slot, sub_slot, repo, type_name)) => circular_node_text(CircularNodeTextArgs {
                cpv,
                slot,
                sub_slot,
                repo,
                type_name,
                color,
            }),
            None => cpv.to_string(),
        }
    };
    let mut lines: Vec<String> = vec![format!("{} depends on", node(&cycle[0]))];
    // `edge_labels[i]` is the edge into line `i + 1`'s package, and the
    // closing line's label is the wrap-around edge. A short slice falls
    // back to `(buildtime)`.
    for (pos, pkg) in cycle.iter().enumerate().skip(1) {
        let label = edge_labels
            .get(pos - 1)
            .map(String::as_str)
            .unwrap_or("buildtime");
        lines.push(format!("{}{} ({label})", " ".repeat(pos), node(pkg)));
    }
    let closing = edge_labels
        .last()
        .map(String::as_str)
        .unwrap_or("buildtime");
    lines.push(format!(
        "{}{} ({closing})",
        " ".repeat(cycle.len()),
        node(&cycle[0])
    ));
    eprint!("{}", lines.join("\n"));

    // A concrete `Change USE:` fix prints instead of the generic advisory.
    let suggestions =
        portage_repo::circular_dep_solutions(cycle, repos, config, autounmask_use_changes, entries);
    if suggestions.is_empty() {
        eprint!(
            "\n\n{prefix}Note that circular dependencies can often be avoided by temporarily\n\
             {prefix}disabling USE flags that trigger optional dependencies.\n"
        );
    } else {
        eprint!("\n\nIt might be possible to break this cycle\n");
        if suggestions.len() == 1 {
            eprintln!("by applying the following change:");
        } else {
            eprintln!(
                "by applying {} the following changes:",
                color.c("bold", "any of")
            );
        }
        for s in &suggestions {
            let changes: Vec<String> = s
                .changes
                .iter()
                .map(|(f, on)| {
                    if *on {
                        color.c("red", &format!("+{f}"))
                    } else {
                        color.c("blue", &format!("-{f}"))
                    }
                })
                .collect();
            eprintln!("- {} (Change USE: {})", s.parent_cpv, changes.join(" "));
            if s.followup {
                eprint!(" (This change might require USE changes on parent packages.)");
            }
        }
        eprintln!("\nNote that this change can be reverted, once the package has been installed.");
        // Shown only with a concrete suggestion, like Portage (the trailer
        // lives inside its `if suggestions:` branch).
        if large_cycle_count {
            eprint!(
                "\nNote that the dependency graph contains a lot of cycles.\n\
                 Several changes might be required to resolve all cycles.\n\
                 Temporarily changing some use flag for all packages might be the better option.\n"
            );
        }
    }
}
