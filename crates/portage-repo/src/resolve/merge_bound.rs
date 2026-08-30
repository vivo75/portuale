//! Lookups over the entries a run will merge.

use crate::*;

/// The version a merge-bound outcome would install, or `None` for the outcomes
/// that merge nothing. Exactly the four outcomes `PassState::resolved_slots` indexes.
pub(crate) fn merge_bound_version(outcome: &PretendOutcome) -> Option<&String> {
    match outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The `category/package-version` a merge-bound entry would install, or `None` for
/// entries that never participate in a build-time cycle.
pub(crate) fn merge_bound_cpv(entry: &GraphEntry) -> Option<String> {
    let version = merge_bound_version(&entry.outcome)?;
    Some(format!("{}/{}-{version}", entry.category, entry.package))
}

/// Lowest-index merge-bound entry per `(category, package)` -- the merge list is
/// already in dependency order, so the first is the instance an edge belongs to.
/// Shared by cycle nodes and owner lookup, so the two cannot disagree.
pub(crate) fn merge_bound_index(entries: &[GraphEntry]) -> HashMap<(&str, &str), usize> {
    let mut cp_index: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if merge_bound_cpv(e).is_some() {
            cp_index
                .entry((e.category.as_str(), e.package.as_str()))
                .or_insert(i);
        }
    }
    cp_index
}
