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

/// Backlog #206 S1: real `_emerge/Package.py::Package.__str__` for a
/// merge-bound package — the `(cpv:slot/sub_slot::repo, <type> scheduled
/// for merge)` node text real
/// `circular_dependency_handler._prepare_circular_dep_message`
/// (`resolver/circular_dependency.py:76`) prints for each cycle member
/// (the digraph f-string renders `str(Package)`). The `cpv:slot/sub::repo`
/// head is wrapped in real's per-operation colour (`PKG_MERGE` for an
/// ebuild merge, `PKG_BINARY_MERGE` for a binary one — a no-op unless the
/// caller's `Colorizer` is enabled). Deliberate cuts, both grounded on
/// live real: the ` to '<ROOT>'` arm never fires (eight unanimous
/// `LLM/evidence/2026-09-27-181-circular-text/real/` captures plus the
/// six-case n206 container probe, all `ROOT=$FX`, show no suffix — and
/// backlog #242 Slice D's S0
/// (`LLM/evidence/2026-10-02-242-display-sites.md` §1d) settles why:
/// every pinned cycle forms in the running root `/`, so real's own
/// per-node gate stays shut on every pinned shape; portuale's root is
/// non-`/` in every test, so the literal gate would inject tmp paths;
/// same rationale as `root_suffix`'s pre-D form), and the
/// `installed` / `uninstall` variants are unreachable here (cycle members
/// are merge-bound by construction — `find_hard_cycles` only considers
/// merge-bound entries; a member missing from `entries` falls back to
/// the bare cpv at the call site). The slot-conflict renderers below
/// keep their own local closures (their unknown-member fallbacks differ
/// from each other and carry no colour, so sharing this helper would move
/// their pins).
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

/// Backlog #19 Slice 5: real `_show_circular_deps` (`depgraph.py:10425`)
/// as a callable unit — the `* Error: circular dependencies:` block, the
/// `Change USE:` suggestion-or-advisory branch, and the
/// `large_cycle_count` trailer. Faithful transcription of the `writemsg`
/// sequence (see the call sites for the re-display note). Prints only —
/// every call site returns `ExitCode::from(1)` itself, except the gated
/// cycle-abort path, which prints this *before* the autounmask section
/// (real `display_problems` order, `:11113` before `:11140`) and then
/// continues into it.
///
/// Backlog #206: the block opens with real's own three newlines (real
/// `writemsg("\n\n")` before `display(handler.merge_list)` plus
/// `writemsg("\n")` after it, all stderr), and each cycle member renders
/// through `circular_node_text` (real `str(Package)`).
///
/// Backlog #228: every cycle line after `depends on` carries its own
/// edge's `priorities[-1]` label (real
/// `_prepare_circular_dep_message`, `resolver/circular_dependency.py`;
/// `DepPriority.__str__`, `_emerge/DepPriority.py`), threaded in as
/// `edge_labels` (`portage_repo::cycle_edge_labels` over the same
/// `entries` the message renders -- `labels[i]` is the edge
/// `cycle[i] -> cycle[(i + 1) % len]`). All-buildtime cycles (every
/// pin before #228) still render `(buildtime)` on every line.
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
    // `_prepare_circular_dep_message`: `<node> depends on`, then each
    // subsequent `<node> (<priorities[-1]>)` at a growing one-space
    // indent, closing back on the first package. Each node is real
    // `str(Package)` (`circular_node_text`); each label is real
    // `DepPriority.__str__` of the edge into that line's package
    // (`edge_labels`, backlog #228 -- `(buildtime)` on every line for
    // the all-buildtime cycles that were the only reachable shape
    // before it). A member missing from `entries` (unreachable for
    // resolver-built cycles) keeps the bare cpv so the block still
    // prints.
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
    // Backlog #228: `edge_labels[i]` is the edge into line `i + 1`'s
    // package (`cycle[i] -> cycle[i + 1]`), and the closing line's label
    // is the wrap-around edge (`cycle[last] -> cycle[0]`). A short
    // slice (unreachable -- `cycle_edge_labels` emits one label per
    // member) falls back to `(buildtime)`, the pre-#228 rendering.
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

    // Real `_show_circular_deps` (`depgraph.py:10448`): when
    // `circular_dependency_handler._find_suggestions` turns up a concrete
    // `Change USE:` fix, print it instead of the generic advisory.
    // `+flag` red / `-flag` blue / `any of` bold, exactly as real
    // `colorize`s them. The `large_cycle_count` trailer below fires on
    // the same branch real gates it on (suggestions shown). Fixtures
    // `usecyclea` (bare suggestion), `gpcyclec` (hard grandparent clash
    // disqualifies it), and `fucyclec` (conditional grandparent clash
    // keeps it with `followup_change`) pin all three outcomes.
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
        // Real `circular_dependency_handler.large_cycle_count`
        // (`depgraph.py:10458`): shown only with a concrete
        // suggestion, like real (the trailer lives inside its
        // `if suggestions:` branch).
        if large_cycle_count {
            eprint!(
                "\nNote that the dependency graph contains a lot of cycles.\n\
                 Several changes might be required to resolve all cycles.\n\
                 Temporarily changing some use flag for all packages might be the better option.\n"
            );
        }
    }
}
