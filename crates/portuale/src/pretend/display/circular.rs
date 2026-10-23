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
    // match-str-ok: display header, not a match string
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
    /// The target root, for the installed check of masked alternatives.
    pub(crate) root: &'a Path,
    /// `--circular-deps-report`.
    pub(crate) format: CircularDepsReport,
    /// Every cycle of the graph (`GraphResult::report_cycles`), for `json`.
    pub(crate) all_cycles: &'a [Vec<String>],
}

/// The `, USE=<flags>` tail of an edge label: enabled flags red, disabled
/// ones blue with a `-`. Empty when no flag is blamed.
// Portage: resolver/circular_dependency.py, _edge_description
pub(crate) fn edge_use_tail(flags: &[(String, bool)], color: &Colorizer) -> String {
    if flags.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = flags
        .iter()
        .map(|(flag, on)| {
            if *on {
                color.c("red", flag)
            } else {
                color.c("blue", &format!("-{flag}"))
            }
        })
        .collect();
    format!(", USE={}", rendered.join(" "))
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
        root,
        format,
        all_cycles,
    } = args;
    let (max_affecting_use, invalid_max) = portage_repo::circular_max_use_flags(config);
    if let Some(value) = invalid_max {
        eprint!(
            "!!! Invalid PORTAGE_CIRCULAR_MAX_USE_FLAGS: {value}\n\
             !!! Expected an integer greater than zero, using {} instead.\n",
            portage_repo::CIRCULAR_MAX_USE_FLAGS_DEFAULT
        );
    }
    if format == CircularDepsReport::Json {
        let report = portage_repo::build_cycle_report(portage_repo::CycleReportArgs {
            cycles: all_cycles,
            fallback: cycle,
            repos,
            config,
            autounmask_use_changes,
            entries,
            root,
            max_affecting_use,
        });
        print!("{}", render_circular_json(&report));
        return;
    }
    let facts = portage_repo::cycle_report_facts(portage_repo::CycleReportFactsArgs {
        cycle,
        edge_labels,
        repos,
        config,
        autounmask_use_changes,
        root,
    });
    // `labels[i]` is the edge `cycle[i] -> cycle[i + 1]`.
    let labels: Vec<String> = edge_labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let tail = facts
                .edge_use
                .get(i)
                .map(|flags| edge_use_tail(flags, color))
                .unwrap_or_default();
            format!("{label}{tail}")
        })
        .collect();
    let edge_labels = labels.as_slice();
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

    let search = portage_repo::circular_dep_search(portage_repo::CircularDepSearchArgs {
        cycle,
        repos,
        config,
        autounmask_use_changes,
        entries,
        max_affecting_use,
    });
    if !search.search_truncated.is_empty() {
        eprint!(
            "\n\n{prefix}The search for USE changes was given up on for the following\n\
             {prefix}packages, because they have too many USE flags that affect the\n\
             {prefix}dependency. Raise PORTAGE_CIRCULAR_MAX_USE_FLAGS (currently {max_affecting_use})\n\
             {prefix}to search harder:\n"
        );
        for cpv in &search.search_truncated {
            eprintln!("- {cpv}");
        }
    }
    if !facts.masked_alternatives.is_empty() {
        eprint!(
            "\n\n{prefix}The following masked packages could be used instead of a\n\
             {prefix}package in the cycle. Unmasking them, for example via\n\
             {prefix}/etc/portage/package.accept_keywords, might solve it:\n"
        );
        for (alternative, member) in &facts.masked_alternatives {
            eprintln!("- {alternative} (instead of {member})");
        }
    }
    if !facts.test_dep_parents.is_empty() {
        eprint!(
            "\n\n{prefix}This cycle involves test dependencies. If you do not\n\
             {prefix}need to run the test suites of the packages listed below,\n\
             {prefix}disable FEATURES=test for them. Put FEATURES=\"-test\" in\n\
             {prefix}/etc/portage/env/no-test.conf, then add the following to\n\
             {prefix}/etc/portage/package.env (see \"package.env\" in the\n\
             {prefix}portage(5) man page for more details):\n\n"
        );
        for cpv in &facts.test_dep_parents {
            eprintln!("={cpv} no-test.conf");
        }
    }

    // A concrete `Change USE:` fix prints instead of the generic advisory.
    let suggestions = search.suggestions;
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
