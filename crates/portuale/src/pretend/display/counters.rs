//! The `Total:` summary line.

use crate::pretend::*;

/// Arguments of [`package_counters_summary`].
pub(crate) struct PackageCountersSummaryArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) color: &'a Colorizer,
    /// Unresolved rows with no owner entry; they count like entry-owned rows.
    pub(crate) orphans: &'a [portage_repo::OrphanBlocker],
    /// In tree mode the solved-stuck rows count too.
    pub(crate) tree: bool,
    /// The settling pass's recorded cycle edges, for the blocker counters.
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// The trailing `Total: …` summary line, shown at `-v` only.
/// Includes `Size of downloads: …` (deduped by filename) and the
/// `Fetch Restriction:` / `Conflict:` lines. A `--onlydeps`-suppressed
/// top-level package is not counted. Blocker resolution stays report-only.
/// Portage: output_helpers.py, _PackageCounters.__str__
pub(crate) fn package_counters_summary(args: PackageCountersSummaryArgs<'_>) -> String {
    let PackageCountersSummaryArgs {
        entries,
        root,
        top_level_pkgs,
        onlydeps,
        color,
        orphans,
        tree,
        circular,
    } = args;
    let plural = |n: u64| if n > 1 { "s" } else { "" };
    let (mut upgrades, mut downgrades, mut new, mut newslot, mut reinst) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut binary, mut interactive, mut uninst) = (0u64, 0u64, 0u64);
    // The rows the merge list carries, plus the orphans in the same group.
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
            // An ordered uninstall node counts too, rendered after `binary`.
            // Portage: output.py, set_pkg_info
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
            // Sum the bytes still to fetch, counting a shared distfile once.
            for (name, size) in &entry.download_files {
                if fetched.insert(name.as_str()) {
                    totalsize += size;
                }
            }
        }
    }

    // Portage's `total_installs`.
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
        details.push(format!("{interactive} {}", color.c("WARN", "interactive")));
    }
    if total != 0 {
        out.push_str(&format!(" ({})", details.join(", ")));
    }
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
            // The whole parenthetical is red.
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
        // `(N unsatisfied)` when the resolver left unsolvable blockers,
        // `(all satisfied)` otherwise.
        if blocks_unsolvable > 0 {
            out.push_str(&color.c("BAD", &format!(" ({blocks_unsolvable} unsatisfied)")));
        } else {
            out.push_str(&color.c("GOOD", " (all satisfied)"));
        }
    }
    out
}
