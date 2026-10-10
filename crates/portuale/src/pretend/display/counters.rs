//! The `Total:` summary line.

use crate::pretend::*;

/// Real `_PackageCounters.__str__` (`output_helpers.py`), the trailing
/// `Total: …` summary line real `output.py::print_verbose` emits via
/// `writemsg_stdout(f"\n{self.counters}\n")` -- gated, in real portage
/// too, on `verbosity == 3` (i.e. `-v`), never plain `-p`. Now includes
/// `, Size of downloads: …` (real `_calc_size`/`counters.totalsize`, via
/// `GraphEntry::download_files`, deduped by filename across the graph
/// like real `myfetchlist`) and the `\nFetch Restriction: N package[s][
/// (M unsatisfied)]` line (from `GraphEntry::fetch_restrict` /
/// `fetch_restrict_satisfied`). The `Conflict:` line's own `(N
/// unsatisfied)`/`(all satisfied)` suffix is classified per
/// `BlockerConflict::unsolvable` (#68 S2/S3); the resolution is still
/// report-only, never an enforcement, as `resolve_pretend_graph`'s doc
/// comment (portage-repo) explains. A top-level package suppressed by
/// `--onlydeps` isn't in real's merge list at all (`pkg_info.ordered`),
/// so it isn't counted here either.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn package_counters_summary(
    entries: &[GraphEntry],
    root: &Path,
    top_level_pkgs: &HashSet<(String, String)>,
    onlydeps: bool,
    color: &Colorizer,
    // Backlog #80: unresolved rows with no owner entry count in
    // `Conflict:` exactly like entry-owned rows (real counts every
    // `Blocker` node in the merge list).
    orphans: &[portage_repo::OrphanBlocker],
    // Backlog #81: in tree mode the solved-stuck rows count too (C0 p2b:
    // `Conflict: 1 block (all satisfied)` under `--tree -v`).
    tree: bool,
    // Backlog #221 (I5): the settling pass's recorded cycle edges,
    // threaded into the blocker-row counters below.
    circular: &HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
) -> String {
    let plural = |n: u64| if n > 1 { "s" } else { "" };
    let (mut upgrades, mut downgrades, mut new, mut newslot, mut reinst) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut binary, mut interactive, mut uninst) = (0u64, 0u64, 0u64);
    // #72 B1/B2: exactly the rows the merge list carries
    // (`count_blocker_rows`), the same disposition split the line printer
    // uses -- plus the #80 orphans, which print in the same trailing
    // group (unsolvable ones are never suppressed anywhere).
    let (blocks, blocks_unsolvable) = count_blocker_rows(entries, root, tree, circular);
    let orphan_unsolvable = orphans.iter().filter(|o| o.conflict.unsolvable).count() as u64;
    let blocks = blocks + orphans.len() as u64;
    let blocks_unsolvable = blocks_unsolvable + orphan_unsolvable;
    let (mut restrict_fetch, mut restrict_fetch_satisfied) = (0u64, 0u64);
    let mut totalsize: u64 = 0;
    let mut fetched: HashSet<&str> = HashSet::new();
    for entry in entries {
        let suppressed =
            onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
        if suppressed {
            continue;
        }
        let merge_bound = match &entry.outcome {
            PretendOutcome::New { .. } => {
                if entry.new_slot {
                    newslot += 1;
                } else {
                    new += 1;
                }
                true
            }
            PretendOutcome::Upgrade { .. } => {
                upgrades += 1;
                true
            }
            PretendOutcome::Downgrade { .. } => {
                downgrades += 1;
                true
            }
            PretendOutcome::Reinstall { .. } => {
                reinst += 1;
                true
            }
            // #72 B4: real `set_pkg_info` counts an ordered uninstall
            // node (`output.py:619-621`); it is rendered in the `Total:`
            // details after `binary`, before `interactive`, and is not
            // part of `total_installs`.
            PretendOutcome::Uninstall { .. } => {
                uninst += 1;
                false
            }
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => false,
        };
        if merge_bound {
            if entry.source == portage_repo::CandidateSource::Binary {
                binary += 1;
            }
            if entry.interactive {
                interactive += 1;
            }
            if entry.fetch_restrict {
                restrict_fetch += 1;
            }
            if entry.fetch_restrict_satisfied {
                restrict_fetch_satisfied += 1;
            }
            // Real `_calc_size`: sum the bytes still to fetch, counting
            // a shared distfile once (real `myfetchlist`).
            for (name, size) in &entry.download_files {
                if fetched.insert(name.as_str()) {
                    totalsize += size;
                }
            }
        }
    }

    // Real `total_installs = upgrades + downgrades + newslot + new + reinst`.
    let total = upgrades + downgrades + newslot + new + reinst;
    let mut out = format!(
        "Total: {total} package{}",
        if total != 1 { "s" } else { "" }
    );
    let mut details: Vec<String> = Vec::new();
    if upgrades > 0 {
        details.push(format!("{upgrades} upgrade{}", plural(upgrades)));
    }
    if downgrades > 0 {
        details.push(format!("{downgrades} downgrade{}", plural(downgrades)));
    }
    if new > 0 {
        details.push(format!("{new} new"));
    }
    if newslot > 0 {
        details.push(format!("{newslot} in new slot{}", plural(newslot)));
    }
    if reinst > 0 {
        details.push(format!("{reinst} reinstall{}", plural(reinst)));
    }
    if binary > 0 {
        details.push(format!(
            "{binary} {}",
            if binary > 1 { "binaries" } else { "binary" }
        ));
    }
    if uninst > 0 {
        details.push(format!("{uninst} uninstall{}", plural(uninst)));
    }
    if interactive > 0 {
        // Real `_PackageCounters.__str__`: `colorize("WARN", "interactive")`
        // -- only the word, not the count.
        details.push(format!("{interactive} {}", color.c("WARN", "interactive")));
    }
    if total != 0 {
        out.push_str(&format!(" ({})", details.join(", ")));
    }
    // Real `__str__`: `f", Size of downloads: {localized_size(self.totalsize)}"`
    // -- appended to the `Total:` line unconditionally.
    out.push_str(&format!(
        ", Size of downloads: {}",
        localized_size(totalsize)
    ));
    if restrict_fetch > 0 {
        out.push_str(&format!(
            "\nFetch Restriction: {restrict_fetch} package{}",
            plural(restrict_fetch)
        ));
        if restrict_fetch_satisfied < restrict_fetch {
            // Real `_PackageCounters.__str__`: `bad(f" (N unsatisfied)")`
            // -- the whole parenthetical is red (`bad` = `BAD` = red).
            out.push_str(&color.c(
                "BAD",
                &format!(
                    " ({} unsatisfied)",
                    restrict_fetch - restrict_fetch_satisfied
                ),
            ));
        }
    }
    if blocks > 0 {
        out.push_str(&format!("\nConflict: {blocks} block{}", plural(blocks)));
        // Real `_PackageCounters.__str__`: `bad(f" ({N} unsatisfied)")`
        // when the resolver left any `_unsolvable_blockers` -- a blocked
        // installed package it could neither replace nor unmerge -- and
        // `good(" (all satisfied)")` when every block resolved since
        // #68 S2/S3 gave the rows a satisfied state.
        if blocks_unsolvable > 0 {
            out.push_str(&color.c("BAD", &format!(" ({blocks_unsolvable} unsatisfied)")));
        } else {
            out.push_str(&color.c("GOOD", " (all satisfied)"));
        }
    }
    out
}
